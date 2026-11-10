//! `corral`: command-line interface. Every command works on the same files the
//! desktop app uses, and a running `corral start` (or the app) picks up
//! changes automatically.

use anyhow::{Context, bail};
use clap::{Parser, Subcommand};
use corral_core::php::{self, PhpVersion};
use corral_core::sites::Registry;
use corral_core::{Config, Paths};
use corral_engine::install;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "corral",
    version,
    about = "Free, open-source local PHP development environment"
)]
struct Cli {
    /// Print machine-readable JSON where supported.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Download the web server and write initial configuration.
    Setup {
        /// Serve on the internal high ports (no privileged helper needed).
        #[arg(long)]
        high_ports: bool,
    },
    /// Run DNS, PHP-FPM and the web server in the foreground.
    Start,
    /// Regenerate all configuration files and report problems.
    Render,
    /// List all sites.
    Sites,
    /// Serve every folder inside DIR as <folder>.<tld>.
    Park { dir: Option<PathBuf> },
    /// Stop serving the folders inside DIR.
    Unpark { dir: Option<PathBuf> },
    /// Serve DIR (default: current directory) as NAME.<tld>.
    Link {
        name: Option<String>,
        #[arg(long)]
        path: Option<PathBuf>,
    },
    /// Remove a linked site.
    Unlink { name: Option<String> },
    /// Pin a site to a PHP version.
    Isolate {
        version: String,
        #[arg(long)]
        site: Option<String>,
    },
    /// Remove a site's PHP pin.
    Unisolate {
        #[arg(long)]
        site: Option<String>,
    },
    /// Serve a site over HTTPS.
    Secure {
        #[arg(long)]
        site: Option<String>,
    },
    /// Serve a site over plain HTTP.
    Unsecure {
        #[arg(long)]
        site: Option<String>,
    },
    /// Set the global default PHP version.
    Use { version: String },
    /// Manage PHP versions.
    #[command(subcommand)]
    Php(PhpCmd),
    /// Show or change the local TLD.
    Tld { tld: Option<String> },
    /// Print the commands that install DNS and certificate trust manually
    /// (used until the privileged helper is installed).
    Trust,
    /// Check the environment for common problems.
    Doctor,
    /// Manage databases and other background services.
    #[command(subcommand)]
    Service(ServiceCmd),
    /// Manage databases inside a MySQL service.
    #[command(subcommand)]
    Db(DbCmd),
    /// Print where Corral keeps its files.
    Paths,
}

#[derive(Subcommand)]
enum ServiceCmd {
    /// List service instances and the catalog.
    List,
    /// Install and add a service instance (e.g. `corral service add mysql`).
    Add {
        kind: String,
        /// Version prefix, e.g. 8.4 (default: newest in the catalog).
        #[arg(long)]
        version: Option<String>,
        /// Instance name (default: the kind, e.g. "mysql").
        #[arg(long)]
        name: Option<String>,
        /// Port on 127.0.0.1 (default: the standard port, or the next free one).
        #[arg(long)]
        port: Option<u16>,
    },
    /// Enable an instance; a running engine starts it within ~1s.
    Start { name: String },
    /// Disable an instance; a running engine stops it within ~1s.
    Stop { name: String },
    /// Remove an instance. Data is kept unless --delete-data is given.
    Remove {
        name: String,
        #[arg(long)]
        delete_data: bool,
    },
}

#[derive(Subcommand)]
enum DbCmd {
    /// Create a database. Without NAME, uses DB_DATABASE from ./.env.
    Create {
        name: Option<String>,
        #[arg(long, default_value = "mysql")]
        service: String,
        /// Switch ./.env to this dedicated DB_USERNAME (a backup of .env is
        /// kept as .env.corral-backup). The existing DB_PASSWORD is reused.
        #[arg(long)]
        user: Option<String>,
    },
    /// List databases.
    List {
        #[arg(long, default_value = "mysql")]
        service: String,
    },
}

/// Read one key from a dotenv file without printing anything else from it.
fn dotenv_value(file: &std::path::Path, key: &str) -> Option<String> {
    let text = std::fs::read_to_string(file).ok()?;
    text.lines().find_map(|l| {
        let l = l.trim();
        let rest = l.strip_prefix(key)?.trim_start().strip_prefix('=')?;
        let v = rest.split(" #").next().unwrap_or("").trim();
        let v = v.trim_matches(|c| c == '"' || c == '\'');
        (!v.is_empty()).then(|| v.to_string())
    })
}

/// Replace (or append) one `KEY=value` line in a dotenv file, keeping every
/// other byte as-is. The first change writes `<file>.corral-backup`.
fn set_dotenv_value(file: &std::path::Path, key: &str, value: &str) -> anyhow::Result<()> {
    let text = std::fs::read_to_string(file).with_context(|| format!("read {}", file.display()))?;
    let backup = file.with_file_name(format!(
        "{}.corral-backup",
        file.file_name().and_then(|n| n.to_str()).unwrap_or(".env")
    ));
    if !backup.exists() {
        std::fs::copy(file, &backup)?;
    }
    let mut found = false;
    let mut out: Vec<String> = text
        .lines()
        .map(|l| {
            let is_key = l
                .trim_start()
                .strip_prefix(key)
                .is_some_and(|r| r.trim_start().starts_with('='));
            if is_key && !found {
                found = true;
                format!("{key}={value}")
            } else {
                l.to_string()
            }
        })
        .collect();
    if !found {
        out.push(format!("{key}={value}"));
    }
    let mut body = out.join("\n");
    if text.ends_with('\n') || !found {
        body.push('\n');
    }
    let perms = std::fs::metadata(file)?.permissions();
    corral_core::fsutil::write_atomic(file, body.as_bytes())?;
    std::fs::set_permissions(file, perms)?;
    Ok(())
}

#[derive(Subcommand)]
enum PhpCmd {
    /// List installed versions.
    List,
    /// Install a PHP version (e.g. 8.4).
    Install {
        version: String,
        /// Install without checksum verification (upstream publishes none yet).
        #[arg(long)]
        allow_unverified: bool,
    },
    /// Remove an installed version.
    Remove { version: String },
}

fn cwd() -> anyhow::Result<PathBuf> {
    Ok(std::env::current_dir()?)
}

fn parse_php(v: &str) -> anyhow::Result<PhpVersion> {
    PhpVersion::parse(v).with_context(|| format!("invalid PHP version {v:?} (expected e.g. 8.4)"))
}

/// Resolve `--site` or the site serving the current directory.
fn site_name(paths: &Paths, cfg: &Config, explicit: Option<String>) -> anyhow::Result<String> {
    if let Some(s) = explicit {
        return Ok(s);
    }
    let reg = Registry::load(paths)?;
    match reg.find_by_path(cfg, &cwd()?)? {
        Some(site) => Ok(site.name),
        None => bail!("the current directory is not a Corral site; pass a site name"),
    }
}

fn print_sites(paths: &Paths, cfg: &Config, json: bool) -> anyhow::Result<()> {
    let sites = Registry::load(paths)?.resolve(cfg)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&sites)?);
        return Ok(());
    }
    if sites.is_empty() {
        println!("No sites yet. Try `corral park ~/Sites` or `corral link` inside a project.");
        return Ok(());
    }
    let w = sites.iter().map(|s| s.url(cfg).len()).max().unwrap_or(0);
    for s in &sites {
        let php = match (&s.php, s.isolated) {
            (Some(v), true) => format!("PHP {v} (isolated)"),
            (Some(v), false) => format!("PHP {v}"),
            (None, _) => "PHP -".into(),
        };
        println!(
            "{:w$}  {:<18} {:<14} {:?}  {}",
            s.url(cfg),
            php,
            s.driver.kind.label(),
            s.origin,
            s.path.display()
        );
    }
    Ok(())
}

async fn run(cli: Cli) -> anyhow::Result<()> {
    let paths = Paths::discover()?;
    paths.ensure()?;
    let mut cfg = Config::load(&paths)?;

    match cli.cmd {
        Cmd::Setup { high_ports } => {
            if high_ports {
                cfg.http_port = cfg.internal_http_port;
                cfg.https_port = cfg.internal_https_port;
                cfg.save(&paths)?;
            }
            if install::caddy_bin(&paths).exists() {
                println!("Web server already installed.");
            } else {
                let bin = install::install_caddy(&paths).await?;
                println!(
                    "Installed Caddy {} to {}",
                    install::CADDY_VERSION,
                    bin.display()
                );
            }
            let r = corral_engine::render(&paths)?;
            println!("Configuration written ({} sites).", r.sites.len());
            println!(
                "\nNext steps:\n  corral php install 8.4 --allow-unverified\n  corral park ~/Sites\n  corral trust   # one-time DNS + HTTPS trust\n  corral start"
            );
        }
        Cmd::Start => {
            use tokio::signal::unix::{SignalKind, signal};
            let mut term = signal(SignalKind::terminate())?;
            let mut hup = signal(SignalKind::hangup())?;
            let shutdown = async move {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = term.recv() => {}
                    _ = hup.recv() => {}
                }
                tracing::info!("shutting down");
            };
            println!(
                "Corral running: DNS on {}:{}, HTTP {}, HTTPS {}. Press Ctrl-C to stop.",
                cfg.loopback, cfg.dns_port, cfg.internal_http_port, cfg.internal_https_port
            );
            corral_engine::run(paths, shutdown).await?;
        }
        Cmd::Render => {
            let r = corral_engine::render(&paths)?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&r)?);
            } else {
                println!(
                    "Rendered {} sites; FPM pools: {:?}",
                    r.sites.len(),
                    r.fpm_versions
                );
                for (site, v) in &r.missing_php {
                    println!("warning: {site} wants PHP {v}, which is not installed");
                }
            }
        }
        Cmd::Sites => print_sites(&paths, &cfg, cli.json)?,
        Cmd::Park { dir } => {
            let dir = dir.map_or_else(cwd, Ok)?.canonicalize()?;
            if !cfg.parked.contains(&dir) {
                cfg.parked.push(dir.clone());
                cfg.save(&paths)?;
            }
            println!("Parked {}", dir.display());
            print_sites(&paths, &cfg, false)?;
        }
        Cmd::Unpark { dir } => {
            let dir = dir.map_or_else(cwd, Ok)?.canonicalize()?;
            let before = cfg.parked.len();
            cfg.parked.retain(|p| p != &dir);
            if cfg.parked.len() == before {
                bail!("{} is not parked", dir.display());
            }
            cfg.save(&paths)?;
            println!("Unparked {}", dir.display());
        }
        Cmd::Link { name, path } => {
            let path = path.map_or_else(cwd, Ok)?;
            let mut reg = Registry::load(&paths)?;
            let name = reg.link(&path, name.as_deref())?;
            reg.save()?;
            let site = reg
                .resolve(&cfg)?
                .into_iter()
                .find(|s| s.name == name)
                .context("linked site vanished")?;
            println!(
                "Linked {} → {} ({})",
                site.url(&cfg),
                site.path.display(),
                site.driver.kind.label()
            );
        }
        Cmd::Unlink { name } => {
            let name = site_name(&paths, &cfg, name)?;
            let mut reg = Registry::load(&paths)?;
            reg.unlink(&name)?;
            reg.save()?;
            println!("Unlinked {name}");
        }
        Cmd::Isolate { version, site } => {
            let v = parse_php(&version)?;
            let name = site_name(&paths, &cfg, site)?;
            let mut reg = Registry::load(&paths)?;
            reg.isolate(&cfg, &name, Some(v.to_string()))?;
            reg.save()?;
            if php::find(&paths, v)?.is_none() {
                println!("note: PHP {v} is not installed yet; run `corral php install {v}`");
            }
            println!("{name} now uses PHP {v}");
        }
        Cmd::Unisolate { site } => {
            let name = site_name(&paths, &cfg, site)?;
            let mut reg = Registry::load(&paths)?;
            reg.isolate(&cfg, &name, None)?;
            reg.save()?;
            println!("{name} now uses the default PHP version");
        }
        Cmd::Secure { site } => {
            let name = site_name(&paths, &cfg, site)?;
            let mut reg = Registry::load(&paths)?;
            let val = (!cfg.secure_by_default).then_some(true);
            reg.set_secure(&cfg, &name, val)?;
            reg.save()?;
            println!("{name} is served over HTTPS");
        }
        Cmd::Unsecure { site } => {
            let name = site_name(&paths, &cfg, site)?;
            let mut reg = Registry::load(&paths)?;
            reg.set_secure(&cfg, &name, Some(false))?;
            reg.save()?;
            println!("{name} is served over HTTP");
        }
        Cmd::Use { version } => {
            let v = parse_php(&version)?;
            if php::find(&paths, v)?.is_none() {
                bail!("PHP {v} is not installed; run `corral php install {v}`");
            }
            cfg.default_php = Some(v.to_string());
            cfg.save(&paths)?;
            php::write_shims(&paths, Some(v))?;
            println!("Default PHP is now {v}");
        }
        Cmd::Php(PhpCmd::List) => {
            let installed = php::installed(&paths)?;
            if installed.is_empty() {
                println!("No PHP versions installed. Try `corral php install 8.4`.");
            }
            for i in installed {
                let mark = if cfg.default_php.as_deref() == Some(&i.version.to_string()) {
                    "*"
                } else {
                    " "
                };
                println!("{mark} {}  {}", i.version, i.dir.display());
            }
        }
        Cmd::Php(PhpCmd::Install {
            version,
            allow_unverified,
        }) => {
            let v = parse_php(&version)?;
            let full = install::latest_static_php(v).await?;
            let src = install::PhpSource::static_php(&full);
            install::install_php(&paths, v, &src, allow_unverified).await?;
            if cfg.default_php.is_none() {
                cfg.default_php = Some(v.to_string());
                cfg.save(&paths)?;
            }
            corral_engine::render(&paths)?;
            println!("Installed PHP {full}");
        }
        Cmd::Php(PhpCmd::Remove { version }) => {
            let v = parse_php(&version)?;
            let inst =
                php::find(&paths, v)?.with_context(|| format!("PHP {v} is not installed"))?;
            anyhow::ensure!(
                paths.owns(&inst.dir),
                "refusing to delete {}",
                inst.dir.display()
            );
            std::fs::remove_dir_all(&inst.dir)?;
            if cfg.default_php.as_deref() == Some(&v.to_string()) {
                cfg.default_php = php::installed(&paths)?
                    .first()
                    .map(|i| i.version.to_string());
                cfg.save(&paths)?;
            }
            corral_engine::render(&paths)?;
            println!("Removed PHP {v}");
        }
        Cmd::Tld { tld: None } => println!("{}", cfg.tld),
        Cmd::Tld { tld: Some(t) } => {
            let old = std::mem::replace(&mut cfg.tld, t);
            cfg.save(&paths)?;
            corral_engine::render(&paths)?;
            println!(
                "TLD changed from .{old} to .{}. Run `corral trust` again to update DNS and HTTPS trust.",
                cfg.tld
            );
        }
        Cmd::Trust => {
            let r = corral_engine::render(&paths)?;
            let ca = corral_tls::LocalCa::cert_path(&paths.certs_dir());
            let resolver = corral_dns::resolver_file(cfg.dns_port);
            println!(
                "Until the privileged helper ships, run these once (they need your password):\n"
            );
            println!("  sudo mkdir -p /etc/resolver");
            println!(
                "  printf '{}' | sudo tee /etc/resolver/{} >/dev/null",
                resolver.replace('\n', "\\n"),
                cfg.tld
            );
            println!(
                "  sudo security add-trusted-cert -d -r trustRoot -k /Library/Keychains/System.keychain \"{}\"",
                ca.display()
            );
            println!("\nCA SHA-256 fingerprint: {}", r.ca_fingerprint);
            println!(
                "\nTo undo:\n  sudo rm /etc/resolver/{}\n  sudo security remove-trusted-cert -d \"{}\"",
                cfg.tld,
                ca.display()
            );
        }
        Cmd::Doctor => doctor(&paths, &cfg)?,
        Cmd::Service(ServiceCmd::List) => {
            use corral_services::store::ServicesFile;
            let file = ServicesFile::load(&paths)?;
            if file.instances.is_empty() {
                println!("No services yet. Try `corral service add mysql`.");
            }
            for (name, i) in &file.instances {
                let up = corral_services::port_in_use(i.port);
                println!(
                    "{name:<14} {} {:<8} 127.0.0.1:{:<6} {:<9} {}",
                    i.kind.label(),
                    i.version,
                    i.port,
                    if i.enabled { "enabled" } else { "disabled" },
                    if up { "listening" } else { "not running" }
                );
            }
            println!("\nCatalog:");
            for r in corral_services::manifest::RELEASES {
                println!(
                    "  {} {}{}  license: {}",
                    r.kind.as_str(),
                    r.version,
                    if r.artifact().is_some() {
                        ""
                    } else {
                        " (not available for this CPU)"
                    },
                    r.kind.license()
                );
            }
        }
        Cmd::Service(ServiceCmd::Add {
            kind,
            version,
            name,
            port,
        }) => {
            let k = corral_services::manifest::Kind::parse(&kind)
                .with_context(|| format!("unknown service {kind:?}; see `corral service list`"))?;
            println!("Installing {} (license: {})…", k.label(), k.license());
            let (name, inst) =
                corral_services::add(&paths, k, version.as_deref(), name.as_deref(), port).await?;
            corral_services::write_shims(
                &paths,
                &corral_services::store::ServicesFile::load(&paths)?,
            )?;
            println!(
                "Added {name}: {} {} on 127.0.0.1:{}",
                k.label(),
                inst.version,
                inst.port
            );
            println!("It starts with `corral start` (or within ~1s if Corral is already running).");
            println!(
                "\nLaravel .env:\n  DB_CONNECTION=mysql\n  DB_HOST=127.0.0.1\n  DB_PORT={}\n  DB_USERNAME=root\n  DB_PASSWORD=",
                inst.port
            );
        }
        Cmd::Service(ServiceCmd::Start { name }) => {
            let mut f = corral_services::store::ServicesFile::load(&paths)?;
            f.set_enabled(&name, true)?;
            f.save(&paths)?;
            println!("{name} enabled");
        }
        Cmd::Service(ServiceCmd::Stop { name }) => {
            let mut f = corral_services::store::ServicesFile::load(&paths)?;
            f.set_enabled(&name, false)?;
            f.save(&paths)?;
            println!("{name} disabled");
        }
        Cmd::Service(ServiceCmd::Remove { name, delete_data }) => {
            corral_services::remove(&paths, &name, delete_data)?;
            if delete_data {
                println!("Removed {name} and deleted its data");
            } else {
                println!(
                    "Removed {name}; data kept in {}",
                    corral_services::store::data_dir(&paths, &name).display()
                );
            }
        }
        Cmd::Db(DbCmd::Create {
            name,
            service,
            user,
        }) => {
            if let Some(u) = &user {
                anyhow::ensure!(
                    corral_services::mysql::valid_user_name(u) && u != "root",
                    "invalid --user {u:?}"
                );
                let env = cwd()?.join(".env");
                set_dotenv_value(&env, "DB_USERNAME", u)?;
                println!("Set DB_USERNAME={u} in .env (backup: .env.corral-backup)");
            }
            let f = corral_services::store::ServicesFile::load(&paths)?;
            let inst = f.get(&service)?;
            let db = match name {
                Some(n) => n,
                None => dotenv_value(&cwd()?.join(".env"), "DB_DATABASE")
                    .context("no NAME given and no DB_DATABASE in ./.env")?,
            };
            anyhow::ensure!(
                corral_services::port_in_use(inst.port),
                "{service} is not running; start Corral first"
            );
            corral_services::mysql::create_database(&paths, &service, inst, &db).await?;
            println!(
                "Database `{db}` ready on {service} (127.0.0.1:{})",
                inst.port
            );
            // Provision the app's own account from .env (values never printed).
            let env = cwd()?.join(".env");
            if let Some(user) = dotenv_value(&env, "DB_USERNAME").filter(|u| u != "root") {
                let password = dotenv_value(&env, "DB_PASSWORD").unwrap_or_default();
                corral_services::mysql::ensure_app_user(
                    &paths, &service, inst, &db, &user, &password,
                )
                .await?;
                println!("Account `{user}` (password from .env) has full access to `{db}`");
            } else if dotenv_value(&env, "DB_PASSWORD").is_some() {
                println!(
                    "note: .env uses root with a password, but Corral's root has none. \
                     Set DB_PASSWORD= (empty) or use a dedicated DB_USERNAME."
                );
            }
        }
        Cmd::Db(DbCmd::List { service }) => {
            let f = corral_services::store::ServicesFile::load(&paths)?;
            let inst = f.get(&service)?;
            for db in corral_services::mysql::list_databases(&paths, &service, inst).await? {
                println!("{db}");
            }
        }
        Cmd::Paths => {
            println!("data:   {}", paths.root.display());
            println!("bin:    {}", paths.bin.display());
            println!("config: {}", paths.config_file().display());
            println!("sites:  {}", paths.sites_file().display());
            println!("logs:   {}", paths.logs_dir().display());
        }
    }
    Ok(())
}

fn doctor(paths: &Paths, cfg: &Config) -> anyhow::Result<()> {
    let mut problems = 0;
    let mut check = |ok: bool, good: &str, bad: String| {
        if ok {
            println!("ok    {good}");
        } else {
            problems += 1;
            println!("FAIL  {bad}");
        }
    };
    check(
        install::caddy_bin(paths).exists(),
        "web server installed",
        "web server missing: run `corral setup`".into(),
    );
    let installed = php::installed(paths)?;
    check(
        !installed.is_empty(),
        &format!("{} PHP version(s) installed", installed.len()),
        "no PHP installed: run `corral php install 8.4`".into(),
    );
    let resolver = PathBuf::from("/etc/resolver").join(&cfg.tld);
    let expected = corral_dns::resolver_file(cfg.dns_port);
    check(
        std::fs::read_to_string(&resolver).is_ok_and(|s| s == expected),
        &format!("{} points .{} at Corral", resolver.display(), cfg.tld),
        format!(
            "{} missing or outdated: run `corral trust`",
            resolver.display()
        ),
    );
    let path_var = std::env::var("PATH").unwrap_or_default();
    let first_php = path_var
        .split(':')
        .map(|d| PathBuf::from(d).join("php"))
        .find(|p| p.is_file());
    let shim = paths.bin.join("php");
    check(
        first_php.as_deref() == Some(shim.as_path()),
        &format!("`php` on PATH is Corral's ({})", shim.display()),
        format!(
            "`php` on PATH is {}; add `export PATH=\"{}:$PATH\"` to your shell profile",
            first_php.map_or("not found".into(), |p| p.display().to_string()),
            paths.bin.display()
        ),
    );
    for port in [cfg.internal_http_port, cfg.internal_https_port] {
        let free = std::net::TcpListener::bind((cfg.loopback, port)).is_ok();
        check(
            free,
            &format!("port {port} is free"),
            format!("port {port} is in use (is `corral start` already running?)"),
        );
    }
    if problems > 0 {
        bail!("{problems} problem(s) found");
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("CORRAL_LOG")
                .unwrap_or_else(|_| "info".into()),
        )
        .with_target(false)
        .init();
    if let Err(e) = run(Cli::parse()).await {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::dotenv_value;

    #[test]
    fn set_dotenv_value_replaces_only_that_line() {
        let t = tempfile::tempdir().unwrap();
        let f = t.path().join(".env");
        let orig = "APP_KEY=k\nDB_USERNAME=root\nDB_USERNAME_X=keep\nDB_PASSWORD=p\n";
        std::fs::write(&f, orig).unwrap();
        super::set_dotenv_value(&f, "DB_USERNAME", "elbilling").unwrap();
        assert_eq!(
            std::fs::read_to_string(&f).unwrap(),
            "APP_KEY=k\nDB_USERNAME=elbilling\nDB_USERNAME_X=keep\nDB_PASSWORD=p\n"
        );
        let backup = t.path().join(".env.corral-backup");
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), orig);
        // A second change keeps the original backup.
        super::set_dotenv_value(&f, "DB_USERNAME", "other").unwrap();
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), orig);
        super::set_dotenv_value(&f, "NEW_KEY", "1").unwrap();
        assert!(
            std::fs::read_to_string(&f)
                .unwrap()
                .ends_with("NEW_KEY=1\n")
        );
    }

    #[test]
    fn reads_single_dotenv_key() {
        let t = tempfile::tempdir().unwrap();
        let f = t.path().join(".env");
        std::fs::write(
            &f,
            "APP_KEY=base64:secret\nDB_DATABASE=\"el_billing\" # comment\nDB_DATABASE_TEST=x\n",
        )
        .unwrap();
        assert_eq!(
            dotenv_value(&f, "DB_DATABASE").as_deref(),
            Some("el_billing")
        );
        assert_eq!(dotenv_value(&f, "DB_DATABASE_TEST").as_deref(), Some("x"));
        assert_eq!(dotenv_value(&f, "MISSING"), None);
    }
}
