//! Admin passwords for services, set from the app.
//!
//! By default every service is open to local connections (MySQL `root`
//! without a password, PostgreSQL `trust`, Valkey without `requirepass`),
//! which is what framework defaults expect. Setting a password locks the
//! server down and stores the password in `services/secrets/<name>` (0600,
//! directory 0700) so Corral's own tools, shims and the engine can still
//! connect. It is never passed on a command line.
//!
//! | Kind       | Accounts protected                    | Mechanism                         |
//! |------------|----------------------------------------|-----------------------------------|
//! | MySQL      | `root`@localhost and @127.0.0.1        | `ALTER USER`, `[client]` password |
//! | PostgreSQL | `postgres`, `root`                     | role passwords + scram pg_hba     |
//! | Valkey     | default user                           | `requirepass`                     |

use crate::manifest::Kind;
use crate::store::{self, Instance, ServicesFile};
use crate::{mysql, port_in_use, postgres};
use anyhow::{Context, bail};
use corral_core::paths::Paths;
use serde::Serialize;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

pub fn secrets_dir(paths: &Paths) -> PathBuf {
    paths.root.join("services/secrets")
}

pub fn secret_file(paths: &Paths, name: &str) -> PathBuf {
    secrets_dir(paths).join(name)
}

/// The admin password of an instance, if one is set.
pub fn password(paths: &Paths, name: &str) -> Option<String> {
    std::fs::read_to_string(secret_file(paths, name))
        .ok()
        .map(|s| s.trim_end_matches(['\n', '\r']).to_string())
        .filter(|s| !s.is_empty())
}

fn store_password(paths: &Paths, name: &str, pw: Option<&str>) -> anyhow::Result<()> {
    let dir = secrets_dir(paths);
    let file = secret_file(paths, name);
    match pw {
        Some(pw) => {
            std::fs::create_dir_all(&dir)?;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
            // Create private before writing, so the secret is never world-readable.
            let tmp = dir.join(format!(".{name}.tmp"));
            {
                use std::io::Write;
                use std::os::unix::fs::OpenOptionsExt;
                let mut f = std::fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .mode(0o600)
                    .open(&tmp)?;
                f.write_all(pw.as_bytes())?;
                f.sync_all()?;
            }
            std::fs::rename(&tmp, &file)?;
        }
        None => {
            if file.exists() {
                std::fs::remove_file(&file)?;
            }
        }
    }
    Ok(())
}

/// Restrict a generated config that may contain a password.
pub(crate) fn make_private(path: &std::path::Path) -> std::io::Result<()> {
    if path.exists() {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// Passwords Corral accepts: printable ASCII without quotes, backslashes or
/// spaces, so they survive every config format and `.env` unquoted.
pub fn valid_password(pw: &str) -> bool {
    (8..=128).contains(&pw.len())
        && pw.bytes().all(|b| {
            b.is_ascii_graphic() && !matches!(b, b'\'' | b'"' | b'\\' | b'`' | b'$' | b'#')
        })
}

/// A random 24-character password from an unambiguous alphabet.
pub fn generate() -> anyhow::Result<String> {
    use std::io::Read;
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789";
    let mut out = String::with_capacity(24);
    let mut buf = [0u8; 64];
    let mut f = std::fs::File::open("/dev/urandom")?;
    while out.len() < 24 {
        f.read_exact(&mut buf)?;
        // Rejection sampling keeps the distribution uniform.
        let limit = 256 - (256 % ALPHABET.len());
        for &b in &buf {
            if (b as usize) < limit && out.len() < 24 {
                out.push(ALPHABET[b as usize % ALPHABET.len()] as char);
            }
        }
    }
    Ok(out)
}

/// How to connect to one instance, for the Connection sheet.
#[derive(Debug, Serialize)]
pub struct Connection {
    pub host: &'static str,
    pub port: u16,
    pub user: Option<&'static str>,
    pub password: Option<String>,
    /// Whether a password can be set from Corral for this kind.
    pub can_secure: bool,
    pub url: String,
    /// Laravel `.env` lines.
    pub env: String,
    pub note: Option<&'static str>,
}

pub fn connection(paths: &Paths, name: &str) -> anyhow::Result<Connection> {
    let inst = ServicesFile::load(paths)?.get(name)?.clone();
    let pw = password(paths, name);
    let p = inst.port;
    let pw_s = pw.clone().unwrap_or_default();
    Ok(match inst.kind {
        Kind::Mysql => Connection {
            host: "127.0.0.1",
            port: p,
            user: Some("root"),
            url: match &pw {
                Some(pw) => format!("mysql://root:{pw}@127.0.0.1:{p}"),
                None => format!("mysql://root@127.0.0.1:{p}"),
            },
            env: format!(
                "DB_CONNECTION=mysql\nDB_HOST=127.0.0.1\nDB_PORT={p}\nDB_DATABASE=laravel\nDB_USERNAME=root\nDB_PASSWORD={pw_s}"
            ),
            password: pw,
            can_secure: true,
            note: None,
        },
        Kind::Postgres => Connection {
            host: "127.0.0.1",
            port: p,
            user: Some("postgres"),
            url: match &pw {
                Some(pw) => format!("postgresql://postgres:{pw}@127.0.0.1:{p}/postgres"),
                None => format!("postgresql://postgres@127.0.0.1:{p}/postgres"),
            },
            env: format!(
                "DB_CONNECTION=pgsql\nDB_HOST=127.0.0.1\nDB_PORT={p}\nDB_DATABASE=laravel\nDB_USERNAME=postgres\nDB_PASSWORD={pw_s}"
            ),
            note: Some(if pw.is_some() {
                "The same password works for the root role."
            } else {
                "Any password is accepted; root works as the user name too."
            }),
            password: pw,
            can_secure: true,
        },
        Kind::Valkey => Connection {
            host: "127.0.0.1",
            port: p,
            user: None,
            url: match &pw {
                Some(pw) => format!("redis://:{pw}@127.0.0.1:{p}"),
                None => format!("redis://127.0.0.1:{p}"),
            },
            env: format!(
                "REDIS_CLIENT=phpredis\nREDIS_HOST=127.0.0.1\nREDIS_PORT={p}\nREDIS_PASSWORD={}",
                if pw.is_some() { pw_s.as_str() } else { "null" }
            ),
            password: pw,
            can_secure: true,
            note: None,
        },
        Kind::Mongodb => Connection {
            host: "127.0.0.1",
            port: p,
            user: None,
            password: None,
            url: format!("mongodb://127.0.0.1:{p}"),
            env: format!("MONGODB_URI=mongodb://127.0.0.1:{p}"),
            can_secure: false,
            note: Some(
                "No authentication. Password protection for MongoDB isn’t available in Corral yet.",
            ),
        },
    })
}

/// Set (`Some`) or remove (`None`) the admin password. MySQL and
/// PostgreSQL must be running, since the change is made inside the server;
/// Valkey takes it from its config file and restarts.
pub async fn set_password(paths: &Paths, name: &str, new: Option<&str>) -> anyhow::Result<()> {
    let inst = ServicesFile::load(paths)?.get(name)?.clone();
    if let Some(pw) = new
        && !valid_password(pw)
    {
        bail!("Use 8–128 characters without spaces, quotes, backslashes, $, # or `.");
    }
    match inst.kind {
        Kind::Mysql => set_mysql(paths, name, &inst, new).await,
        Kind::Postgres => set_postgres(paths, name, &inst, new).await,
        Kind::Valkey => {
            if port_in_use(inst.port) {
                set_valkey_live(paths, name, &inst, new).await?;
            }
            store_password(paths, name, new)?;
            // Persist it for the next start (no restart needed now).
            crate::valkey::write_config(paths, name, &inst)?;
            crate::write_shims(paths, &ServicesFile::load(paths)?)?;
            touch_services(paths)
        }
        Kind::Mongodb => bail!("Password protection for MongoDB isn’t available yet"),
    }
}

/// `CONFIG SET requirepass` on the running server. The command goes over
/// stdin and the current password over the environment, never argv.
async fn set_valkey_live(
    paths: &Paths,
    name: &str,
    inst: &Instance,
    new: Option<&str>,
) -> anyhow::Result<()> {
    use tokio::io::AsyncWriteExt;
    let cli = store::dist_dir(paths, inst.kind, &inst.version).join("bin/valkey-cli");
    let mut cmd = tokio::process::Command::new(cli);
    cmd.args(["-h", "127.0.0.1", "-p", &inst.port.to_string()])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    if let Some(old) = password(paths, name) {
        cmd.env("REDISCLI_AUTH", old);
    }
    let mut child = cmd.spawn().context("run valkey-cli")?;
    let mut stdin = child.stdin.take().expect("piped");
    stdin
        .write_all(format!("CONFIG SET requirepass \"{}\"\n", new.unwrap_or("")).as_bytes())
        .await?;
    drop(stdin);
    let out = child.wait_with_output().await?;
    let reply = String::from_utf8_lossy(&out.stdout);
    if !out.status.success() || !reply.trim().eq_ignore_ascii_case("OK") {
        bail!(
            "valkey-cli: {}{}",
            reply.trim(),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

fn ensure_running(name: &str, inst: &Instance) -> anyhow::Result<()> {
    if !port_in_use(inst.port) {
        bail!("{name} isn’t running. Start Corral and turn the service on, then try again.");
    }
    Ok(())
}

/// Bump services.toml's mtime so a running engine re-syncs (restarting
/// services whose config changed and rewriting tool configs).
fn touch_services(paths: &Paths) -> anyhow::Result<()> {
    let f = ServicesFile::load(paths)?;
    f.save(paths)?;
    Ok(())
}

async fn set_mysql(
    paths: &Paths,
    name: &str,
    inst: &Instance,
    new: Option<&str>,
) -> anyhow::Result<()> {
    ensure_running(name, inst)?;
    let lit = mysql::sql_string(new.unwrap_or(""));
    // Runs with the current credentials (option file), then switches them.
    mysql::run_sql(
        paths,
        name,
        inst,
        &format!(
            "ALTER USER 'root'@'localhost' IDENTIFIED BY {lit}; \
             CREATE USER IF NOT EXISTS 'root'@'127.0.0.1' IDENTIFIED BY {lit}; \
             ALTER USER 'root'@'127.0.0.1' IDENTIFIED BY {lit}; \
             GRANT ALL PRIVILEGES ON *.* TO 'root'@'127.0.0.1' WITH GRANT OPTION;"
        ),
    )
    .await
    .context("changing the root password")?;
    store_password(paths, name, new)?;
    // The [client] section now carries the password; the server restarts
    // once to pick up the rewritten option file.
    mysql::write_option_file(paths, name, inst)?;
    crate::write_shims(paths, &ServicesFile::load(paths)?)?;
    touch_services(paths)
}

async fn set_postgres(
    paths: &Paths,
    name: &str,
    inst: &Instance,
    new: Option<&str>,
) -> anyhow::Result<()> {
    ensure_running(name, inst)?;
    let roles = match new {
        Some(pw) => {
            let l = format!("'{}'", pw.replace('\'', "''"));
            format!(
                "ALTER ROLE postgres PASSWORD {l};\n\
                 DO $$ BEGIN IF EXISTS (SELECT FROM pg_roles WHERE rolname = 'root') THEN \
                 ALTER ROLE root PASSWORD {l}; END IF; END $$;\n"
            )
        }
        None => "ALTER ROLE postgres PASSWORD NULL;\n\
                 DO $$ BEGIN IF EXISTS (SELECT FROM pg_roles WHERE rolname = 'root') THEN \
                 ALTER ROLE root PASSWORD NULL; END IF; END $$;\n"
            .into(),
    };
    postgres::run_sql(paths, name, inst, &roles)
        .await
        .context("changing role passwords")?;
    postgres::write_hba(paths, name, new.is_some())?;
    // A signal, not SQL: needs no login, so it works in both directions.
    postgres::reload(paths, name, inst)
        .await
        .context("reloading pg_hba.conf")?;
    // Only now does the server use the new rules; store what it expects.
    store_password(paths, name, new)?;
    touch_services(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_passwords_are_valid_and_differ() {
        let a = generate().unwrap();
        let b = generate().unwrap();
        assert_eq!(a.len(), 24);
        assert!(valid_password(&a));
        assert_ne!(a, b);
    }

    #[test]
    fn rejects_unsafe_passwords() {
        assert!(!valid_password("short"));
        assert!(!valid_password("has space in it"));
        assert!(!valid_password("quote'quote'"));
        assert!(!valid_password("dollar$sign1"));
        assert!(valid_password("Good-Pass_123!"));
    }

    #[test]
    fn secret_is_private() {
        let t = tempfile::tempdir().unwrap();
        let p = Paths::at(t.path());
        store_password(&p, "mysql", Some("hunter2hunter2")).unwrap();
        assert_eq!(password(&p, "mysql").as_deref(), Some("hunter2hunter2"));
        let mode = std::fs::metadata(secret_file(&p, "mysql"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        let dmode = std::fs::metadata(secrets_dir(&p))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(dmode & 0o777, 0o700);
        store_password(&p, "mysql", None).unwrap();
        assert!(password(&p, "mysql").is_none());
    }
}
