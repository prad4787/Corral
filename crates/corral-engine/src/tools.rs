//! Built-in database tools, each served on its own `<name>.<tld>` host and
//! reachable from this Mac only:
//!
//! | Tool            | Kind                         | Source (verified)                          |
//! |-----------------|------------------------------|--------------------------------------------|
//! | Adminer         | PHP app                      | github.com/vrana/adminer, GitHub SHA-256   |
//! | phpMyAdmin      | PHP app (needs mysqli, ≤8.3) | phpmyadmin.net tarball + published .sha256 |
//! | pgweb           | Go binary (PostgreSQL)       | github.com/sosedoff/pgweb, GitHub SHA-256  |
//! | Redis Commander | Node app (Redis/Valkey)      | npm `redis-commander` (MIT)                |

use crate::{adminer, install};
use anyhow::{Context, bail};
use corral_core::Config;
use corral_core::paths::Paths;
use corral_core::php::{self, Installation, PhpVersion};
use corral_services::manifest::Kind;
use corral_services::store::ServicesFile;
use corral_supervisor::Spec;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const PGWEB_PORT: u16 = 18082;
pub const REDIS_COMMANDER_PORT: u16 = 18083;
const REDIS_COMMANDER_VERSION: &str = "0.9.0";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tool {
    Adminer,
    Phpmyadmin,
    Pgweb,
    RedisCommander,
}

impl Tool {
    pub const ALL: [Tool; 4] = [
        Tool::Adminer,
        Tool::Phpmyadmin,
        Tool::Pgweb,
        Tool::RedisCommander,
    ];

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "adminer" => Self::Adminer,
            "phpmyadmin" => Self::Phpmyadmin,
            "pgweb" => Self::Pgweb,
            "redis-commander" | "redis" => Self::RedisCommander,
            _ => return None,
        })
    }
    pub fn id(self) -> &'static str {
        match self {
            Self::Adminer => "adminer",
            Self::Phpmyadmin => "phpmyadmin",
            Self::Pgweb => "pgweb",
            Self::RedisCommander => "redis-commander",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Adminer => "Adminer",
            Self::Phpmyadmin => "phpMyAdmin",
            Self::Pgweb => "pgweb",
            Self::RedisCommander => "Redis Commander",
        }
    }
    /// Host name prefix: `<prefix>.<tld>`.
    pub fn host_prefix(self) -> &'static str {
        match self {
            Self::Adminer => "adminer",
            Self::Phpmyadmin => "phpmyadmin",
            Self::Pgweb => "pgweb",
            Self::RedisCommander => "redis",
        }
    }
    pub fn host(self, tld: &str) -> String {
        format!("{}.{tld}", self.host_prefix())
    }
    pub fn description(self) -> &'static str {
        match self {
            Self::Adminer => "One page for MySQL, PostgreSQL and SQLite.",
            Self::Phpmyadmin => "Full MySQL administration.",
            Self::Pgweb => "PostgreSQL browser and SQL editor.",
            Self::RedisCommander => "Browse Redis/Valkey keys and run commands in its console.",
        }
    }
    pub fn dir(self, paths: &Paths) -> PathBuf {
        match self {
            Self::Adminer => adminer::dir(paths),
            other => install::tools_dir(paths).join(other.id()),
        }
    }
    pub fn installed(self, paths: &Paths) -> bool {
        match self {
            Self::Adminer => adminer::installed(paths),
            Self::Phpmyadmin => self.dir(paths).join("index.php").is_file(),
            Self::Pgweb => self.dir(paths).join("pgweb").is_file(),
            Self::RedisCommander => rc_entry(paths).is_file(),
        }
    }
}

fn service_port(paths: &Paths, kind: Kind) -> Option<u16> {
    ServicesFile::load(paths)
        .ok()?
        .instances
        .values()
        .find(|i| i.kind == kind)
        .map(|i| i.port)
}

fn rc_entry(paths: &Paths) -> PathBuf {
    Tool::RedisCommander
        .dir(paths)
        .join("node_modules/redis-commander/bin/redis-commander.js")
}

// --- PHP capability (cached: `php -m` per binary + mtime) -------------------

type ModCache = HashMap<(PathBuf, Option<std::time::SystemTime>), Vec<String>>;
static MODULES: Mutex<Option<ModCache>> = Mutex::new(None);

pub fn php_has(inst: &Installation, ext: &str) -> bool {
    let cli = inst.cli();
    let key = (
        cli.clone(),
        std::fs::metadata(&cli).and_then(|m| m.modified()).ok(),
    );
    let mut guard = MODULES.lock().unwrap();
    let cache = guard.get_or_insert_with(HashMap::new);
    let mods = cache.entry(key).or_insert_with(|| {
        std::process::Command::new(&cli)
            .args(["-n", "-m"])
            .output()
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .map(|l| l.trim().to_ascii_lowercase())
                    .collect()
            })
            .unwrap_or_default()
    });
    mods.iter().any(|m| m == ext)
}

/// PHP installation to serve a PHP tool with, or why none fits.
pub fn php_for(paths: &Paths, tool: Tool) -> Result<Installation, String> {
    let installs = php::installed(paths).unwrap_or_default();
    match tool {
        // phpMyAdmin 5.2 supports PHP < 8.4 and requires mysqli.
        Tool::Phpmyadmin => installs
            .iter()
            .find(|i| i.version < PhpVersion { major: 8, minor: 4 } && php_has(i, "mysqli"))
            .cloned()
            .ok_or_else(|| {
                "Needs PHP 8.3 or older with the mysqli extension (Corral’s own PHP builds include it)."
                    .to_string()
            }),
        _ => installs.first().cloned().ok_or_else(|| "Install PHP first.".to_string()),
    }
}

/// Why a tool would not work right now (shown in the UI), if anything.
pub fn note(paths: &Paths, tool: Tool) -> Option<String> {
    match tool {
        Tool::Adminer => php_for(paths, tool).err(),
        Tool::Phpmyadmin => php_for(paths, tool).err().or_else(|| {
            service_port(paths, Kind::Mysql)
                .is_none()
                .then(|| "Add MySQL first.".into())
        }),
        Tool::Pgweb => service_port(paths, Kind::Postgres)
            .is_none()
            .then(|| "Add PostgreSQL to connect automatically.".into()),
        Tool::RedisCommander => service_port(paths, Kind::Valkey)
            .is_none()
            .then(|| "Add Valkey (Redis) to connect.".into()),
    }
}

// --- install / remove ---------------------------------------------------------

async fn github_asset(repo: &str, name: &str) -> anyhow::Result<(String, corral_fetch::Expected)> {
    let body = corral_fetch::client()?
        .get(format!(
            "https://api.github.com/repos/{repo}/releases/latest"
        ))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let rel: serde_json::Value = serde_json::from_str(&body)?;
    let asset = rel["assets"]
        .as_array()
        .and_then(|a| a.iter().find(|x| x["name"] == name))
        .with_context(|| format!("{name} missing from {repo}'s latest release"))?;
    let digest = asset["digest"]
        .as_str()
        .and_then(|d| d.strip_prefix("sha256:"))
        .context("GitHub did not report a SHA-256 for the asset")?;
    Ok((
        asset["browser_download_url"]
            .as_str()
            .context("asset url")?
            .to_string(),
        corral_fetch::Expected {
            algo: corral_fetch::Hash::Sha256,
            hex: digest.into(),
        },
    ))
}

fn random_hex(n: usize) -> anyhow::Result<String> {
    use std::io::Read;
    let mut buf = vec![0u8; n];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut buf)?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

/// Write phpMyAdmin's config: auto-login as root to the first MySQL service.
pub fn write_phpmyadmin_config(paths: &Paths) -> anyhow::Result<()> {
    let dir = Tool::Phpmyadmin.dir(paths);
    if !dir.join("index.php").is_file() {
        return Ok(());
    }
    let secret_file = dir.join(".corral-secret");
    let secret = match std::fs::read_to_string(&secret_file) {
        Ok(s) if s.trim().len() == 32 => s.trim().to_string(),
        _ => {
            let s = random_hex(16)?;
            std::fs::write(&secret_file, &s)?;
            s
        }
    };
    let port = service_port(paths, Kind::Mysql).unwrap_or(3306);
    let tmp = dir.join("tmp");
    std::fs::create_dir_all(&tmp)?;
    let body = format!(
        "<?php\n// Generated by Corral. Local development only (127.0.0.1).\n\
         $cfg['blowfish_secret'] = '{secret}';\n\
         $cfg['TempDir'] = '{tmp}';\n\
         $cfg['VersionCheck'] = false;\n\
         $i = 1;\n\
         $cfg['Servers'][$i]['host'] = '127.0.0.1';\n\
         $cfg['Servers'][$i]['port'] = '{port}';\n\
         $cfg['Servers'][$i]['auth_type'] = 'config';\n\
         $cfg['Servers'][$i]['user'] = 'root';\n\
         $cfg['Servers'][$i]['password'] = '';\n\
         $cfg['Servers'][$i]['AllowNoPassword'] = true;\n",
        tmp = tmp.display().to_string().replace('\'', "\\'"),
    );
    corral_core::fsutil::write_if_changed(&dir.join("config.inc.php"), body.as_bytes())?;
    Ok(())
}

async fn install_phpmyadmin(paths: &Paths) -> anyhow::Result<()> {
    let body = corral_fetch::client()?
        .get("https://www.phpmyadmin.net/home_page/version.json")
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let v: serde_json::Value = serde_json::from_str(&body)?;
    let version = v["version"]
        .as_str()
        .context("no phpMyAdmin version")?
        .to_string();
    anyhow::ensure!(
        version.bytes().all(|b| b.is_ascii_digit() || b == b'.'),
        "unexpected version {version:?}"
    );
    let url = format!(
        "https://files.phpmyadmin.net/phpMyAdmin/{version}/phpMyAdmin-{version}-all-languages.tar.gz"
    );
    let sum = corral_fetch::client()?
        .get(format!("{url}.sha256"))
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let hex = sum
        .split_whitespace()
        .next()
        .filter(|h| h.len() == 64)
        .context("unexpected phpMyAdmin checksum file")?;
    let expected = corral_fetch::Expected {
        algo: corral_fetch::Hash::Sha256,
        hex: hex.into(),
    };
    let archive =
        corral_fetch::fetch_verified(&url, &paths.downloads_dir(), Some(&expected)).await?;
    let dir = Tool::Phpmyadmin.dir(paths);
    let result = tokio::task::spawn_blocking({
        let (a, d) = (archive.clone(), dir.clone());
        move || corral_fetch::extract_tree(&a, &d, 1)
    })
    .await?;
    let _ = std::fs::remove_file(&archive);
    result?;
    write_phpmyadmin_config(paths)
}

async fn install_pgweb(paths: &Paths) -> anyhow::Result<()> {
    let arch = if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "amd64"
    };
    let name = format!("pgweb_darwin_{arch}.zip");
    let (url, expected) = github_asset("sosedoff/pgweb", &name).await?;
    let archive =
        corral_fetch::fetch_verified(&url, &paths.downloads_dir(), Some(&expected)).await?;
    let dest = Tool::Pgweb.dir(paths).join("pgweb");
    let entry = format!("pgweb_darwin_{arch}");
    let r = corral_fetch::extract_zip_entry(&archive, &entry, &dest);
    let _ = std::fs::remove_file(&archive);
    r
}

async fn install_redis_commander(paths: &Paths) -> anyhow::Result<()> {
    if !paths.bin.join("node").exists() {
        let (major, _) = crate::node::install(paths, None).await?;
        let mut cfg = Config::load(paths)?;
        if cfg.default_node.is_none() {
            cfg.default_node = Some(major);
            cfg.save(paths)?;
        }
        crate::node::write_shims(paths, cfg.default_node)?;
    }
    let dir = Tool::RedisCommander.dir(paths);
    std::fs::create_dir_all(&dir)?;
    let out = tokio::process::Command::new(paths.bin.join("npm"))
        .args(["install", "--prefix"])
        .arg(&dir)
        .args([
            &format!("redis-commander@{REDIS_COMMANDER_VERSION}"),
            "--omit=dev",
            "--no-audit",
            "--no-fund",
            "--loglevel=error",
        ])
        .env("PATH", format!("{}:/usr/bin:/bin", paths.bin.display()))
        .output()
        .await
        .context("run npm")?;
    if !out.status.success() || !rc_entry(paths).is_file() {
        bail!(
            "npm install failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

pub async fn install(paths: &Paths, tool: Tool) -> anyhow::Result<()> {
    match tool {
        Tool::Adminer => adminer::install(paths).await.map(|_| ()),
        Tool::Phpmyadmin => install_phpmyadmin(paths).await,
        Tool::Pgweb => install_pgweb(paths).await,
        Tool::RedisCommander => install_redis_commander(paths).await,
    }
}

pub fn remove(paths: &Paths, tool: Tool) -> anyhow::Result<()> {
    let dir = tool.dir(paths);
    anyhow::ensure!(paths.owns(&dir), "refusing to remove {}", dir.display());
    if dir.exists() {
        std::fs::remove_dir_all(dir)?;
    }
    Ok(())
}

// --- runtime --------------------------------------------------------------------

/// Background process for tools that are servers themselves.
pub fn spec(paths: &Paths, tool: Tool) -> Option<Spec> {
    if !tool.installed(paths) {
        return None;
    }
    let log = paths.logs_dir().join(format!("tool-{}.log", tool.id()));
    match tool {
        Tool::Pgweb => {
            let mut args = vec![
                "--bind=127.0.0.1".to_string(),
                format!("--listen={PGWEB_PORT}"),
                "--skip-open".into(),
            ];
            match service_port(paths, Kind::Postgres) {
                Some(p) => args.push(format!(
                    "--url=postgres://postgres@127.0.0.1:{p}/postgres?sslmode=disable"
                )),
                None => args.push("--sessions".into()),
            }
            Some(Spec::new("tool-pgweb", tool.dir(paths).join("pgweb"), log).args(args))
        }
        Tool::RedisCommander => {
            let port = service_port(paths, Kind::Valkey).unwrap_or(6379);
            let mut s = Spec::new("tool-redis-commander", paths.bin.join("node"), log).args([
                rc_entry(paths).display().to_string(),
                "--address".into(),
                "127.0.0.1".into(),
                "--port".into(),
                REDIS_COMMANDER_PORT.to_string(),
                "--redis-host".into(),
                "127.0.0.1".into(),
                "--redis-port".into(),
                port.to_string(),
                "--nosave".into(),
            ]);
            // Keep its config out of the user's home directory.
            s.env
                .push(("HOME".into(), tool.dir(paths).display().to_string()));
            Some(s)
        }
        _ => None,
    }
}

/// How the proxy should serve an installed tool.
pub enum Serve {
    Php { docroot: PathBuf, php: PhpVersion },
    Upstream(String),
}

pub fn serve(paths: &Paths, tool: Tool) -> Option<Serve> {
    if !tool.installed(paths) {
        return None;
    }
    Some(match tool {
        Tool::Adminer | Tool::Phpmyadmin => {
            // phpMyAdmin falls back to the newest PHP so it can show its own
            // requirement message rather than a 404.
            let inst = php_for(paths, tool)
                .ok()
                .or_else(|| php::installed(paths).ok()?.into_iter().next())?;
            Serve::Php {
                docroot: tool.dir(paths),
                php: inst.version,
            }
        }
        Tool::Pgweb => Serve::Upstream(format!("127.0.0.1:{PGWEB_PORT}")),
        Tool::RedisCommander => Serve::Upstream(format!("127.0.0.1:{REDIS_COMMANDER_PORT}")),
    })
}

pub fn dir_size(p: &Path) -> u64 {
    walk_size(p)
}

fn walk_size(p: &Path) -> u64 {
    std::fs::read_dir(p)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => walk_size(&e.path()),
            Ok(_) => e.metadata().map(|m| m.len()).unwrap_or(0),
            Err(_) => 0,
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_and_parse() {
        assert_eq!(Tool::RedisCommander.host("test"), "redis.test");
        assert_eq!(Tool::parse("redis"), Some(Tool::RedisCommander));
        assert_eq!(Tool::parse("pgweb"), Some(Tool::Pgweb));
        assert_eq!(Tool::parse("x"), None);
    }

    #[test]
    fn phpmyadmin_config_targets_mysql_service() {
        let t = tempfile::tempdir().unwrap();
        let p = Paths::at(t.path());
        p.ensure().unwrap();
        let dir = Tool::Phpmyadmin.dir(&p);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.php"), "").unwrap();
        std::fs::write(
            p.root.join("services.toml"),
            "[instances.mysql]\nkind = \"mysql\"\nversion = \"8.4.11\"\nport = 3307\n",
        )
        .unwrap();
        write_phpmyadmin_config(&p).unwrap();
        let c = std::fs::read_to_string(dir.join("config.inc.php")).unwrap();
        assert!(c.contains("['port'] = '3307'"));
        assert!(c.contains("['host'] = '127.0.0.1'"));
        let secret1 = std::fs::read_to_string(dir.join(".corral-secret")).unwrap();
        write_phpmyadmin_config(&p).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join(".corral-secret")).unwrap(),
            secret1,
            "secret is stable"
        );
    }

    #[test]
    fn server_tools_bind_loopback() {
        let t = tempfile::tempdir().unwrap();
        let p = Paths::at(t.path());
        let d = Tool::Pgweb.dir(&p);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("pgweb"), "").unwrap();
        let s = spec(&p, Tool::Pgweb).unwrap();
        assert!(s.args.contains(&"--bind=127.0.0.1".to_string()));
        assert!(
            s.args.contains(&"--sessions".to_string()),
            "no PostgreSQL yet: let the user connect"
        );
        assert!(spec(&p, Tool::RedisCommander).is_none(), "not installed");
    }
}
