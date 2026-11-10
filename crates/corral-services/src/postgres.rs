//! PostgreSQL: data-directory initialization, supervisor spec, databases.
//!
//! Development defaults: 127.0.0.1 only, `trust` authentication for local
//! connections (any password is accepted, like Laravel's default empty
//! DB_PASSWORD), UTF-8, superusers `postgres` and `root` (Laravel's default
//! DB_USERNAME), unix socket in Corral's run dir.

use crate::store::{self, Instance};
use anyhow::{Context, bail};
use corral_core::paths::Paths;
use corral_supervisor::Spec;
use std::path::PathBuf;
use std::time::Duration;

fn bin(paths: &Paths, inst: &Instance, tool: &str) -> PathBuf {
    store::dist_dir(paths, inst.kind, &inst.version)
        .join("bin")
        .join(tool)
}

pub fn initialized(paths: &Paths, name: &str) -> bool {
    store::data_dir(paths, name).join("PG_VERSION").exists()
}

pub async fn ensure_initialized(paths: &Paths, name: &str, inst: &Instance) -> anyhow::Result<()> {
    if initialized(paths, name) {
        return Ok(());
    }
    let data = store::data_dir(paths, name);
    if data.exists() && std::fs::read_dir(&data)?.next().is_some() {
        bail!(
            "data directory {} exists but is not a PostgreSQL cluster; move it away first",
            data.display()
        );
    }
    if let Some(parent) = data.parent() {
        std::fs::create_dir_all(parent)?;
    }
    tracing::info!(service = name, "initializing PostgreSQL cluster");
    let out = tokio::process::Command::new(bin(paths, inst, "initdb"))
        .arg("-D")
        .arg(&data)
        .args([
            "-U",
            "postgres",
            "--auth=trust",
            "--encoding=UTF8",
            "--no-locale",
        ])
        .output()
        .await
        .context("run initdb")?;
    if !out.status.success() || !initialized(paths, name) {
        bail!(
            "initdb failed ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

pub fn spec(paths: &Paths, name: &str, inst: &Instance) -> Spec {
    let mut s = Spec::new(
        store::unit(name),
        bin(paths, inst, "postgres"),
        store::log_file(paths, name),
    )
    .args([
        "-D".to_string(),
        store::data_dir(paths, name).display().to_string(),
        "-p".into(),
        inst.port.to_string(),
        "-c".into(),
        "listen_addresses=127.0.0.1".into(),
        "-c".into(),
        format!("unix_socket_directories={}", paths.run_dir().display()),
        "-c".into(),
        "logging_collector=off".into(),
    ]);
    // SIGINT = "fast" shutdown: disconnects clients and checkpoints cleanly.
    // SIGTERM ("smart") would wait for every client to leave first.
    s.stop_signal = SIGINT;
    s.stop_timeout = Duration::from_secs(60);
    s
}

const SIGINT: i32 = 2;

/// Quote an identifier ("…") for PostgreSQL.
fn ident(v: &str) -> String {
    format!("\"{}\"", v.replace('"', "\"\""))
}

/// Quote a string literal for PostgreSQL (standard_conforming_strings on).
fn lit(v: &str) -> String {
    format!("'{}'", v.replace('\'', "''"))
}

async fn psql(paths: &Paths, inst: &Instance, sql: &str) -> anyhow::Result<String> {
    use tokio::io::AsyncWriteExt;
    let mut child = tokio::process::Command::new(bin(paths, inst, "psql"))
        .args([
            "-h",
            "127.0.0.1",
            "-p",
            &inst.port.to_string(),
            "-U",
            "postgres",
            "-d",
            "postgres",
        ])
        .args(["-X", "-q", "-t", "-A", "-v", "ON_ERROR_STOP=1"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    let mut stdin = child.stdin.take().expect("piped");
    stdin.write_all(sql.as_bytes()).await?;
    drop(stdin);
    let out = child.wait_with_output().await?;
    if !out.status.success() {
        bail!("psql: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `root` superuser so Laravel's default DB_USERNAME works. Idempotent.
pub async fn ensure_dev_roles(paths: &Paths, inst: &Instance) -> anyhow::Result<()> {
    psql(
        paths,
        inst,
        "DO $$ BEGIN IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'root') THEN \
         CREATE ROLE root SUPERUSER LOGIN; END IF; END $$;",
    )
    .await?;
    Ok(())
}

pub async fn create_database(paths: &Paths, inst: &Instance, db: &str) -> anyhow::Result<()> {
    if !crate::mysql::valid_db_name(db) {
        bail!("invalid database name {db:?}: use letters, digits, '_' and '-'");
    }
    // CREATE DATABASE has no IF NOT EXISTS; check first.
    let exists = psql(
        paths,
        inst,
        &format!("SELECT 1 FROM pg_database WHERE datname = {};", lit(db)),
    )
    .await?;
    if exists.trim() != "1" {
        psql(
            paths,
            inst,
            &format!("CREATE DATABASE {} ENCODING 'UTF8';", ident(db)),
        )
        .await?;
    }
    Ok(())
}

pub async fn ensure_app_user(
    paths: &Paths,
    inst: &Instance,
    db: &str,
    user: &str,
    password: &str,
) -> anyhow::Result<()> {
    if !crate::mysql::valid_user_name(user) || user == "postgres" || user == "root" {
        bail!("refusing to manage PostgreSQL role {user:?}");
    }
    let (u, p) = (ident(user), lit(password));
    psql(
        paths,
        inst,
        &format!(
            "DO $$ BEGIN IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = {ul}) THEN \
             CREATE ROLE {u} LOGIN PASSWORD {p}; ELSE ALTER ROLE {u} LOGIN PASSWORD {p}; END IF; END $$;\n\
             ALTER DATABASE {d} OWNER TO {u};\n",
            ul = lit(user),
            d = ident(db),
        ),
    )
    .await?;
    Ok(())
}

pub async fn list_databases(paths: &Paths, inst: &Instance) -> anyhow::Result<Vec<String>> {
    Ok(psql(
        paths,
        inst,
        "SELECT datname FROM pg_database WHERE NOT datistemplate AND datname <> 'postgres' ORDER BY 1;",
    )
    .await?
    .lines()
    .map(str::trim)
    .filter(|l| !l.is_empty())
    .map(str::to_owned)
    .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::Kind;

    #[test]
    fn spec_is_local_only_with_fast_shutdown() {
        let t = tempfile::tempdir().unwrap();
        let p = Paths::at(t.path());
        let inst = Instance {
            kind: Kind::Postgres,
            version: "18.6".into(),
            port: 5433,
            enabled: true,
        };
        let s = spec(&p, "postgres", &inst);
        assert!(
            s.program
                .ends_with("services/dist/postgres-18.6/bin/postgres")
        );
        assert!(s.args.contains(&"listen_addresses=127.0.0.1".to_string()));
        assert!(s.args.windows(2).any(|w| w[0] == "-p" && w[1] == "5433"));
        assert_eq!(s.stop_signal, 2);
    }

    #[test]
    fn quoting() {
        assert_eq!(ident(r#"we"ird"#), r#""we""ird""#);
        assert_eq!(lit("it's"), "'it''s'");
        assert_eq!(
            lit("x'; DROP ROLE postgres; --"),
            "'x''; DROP ROLE postgres; --'"
        );
    }
}
