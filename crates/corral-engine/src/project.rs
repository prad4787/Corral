//! Per-project actions shared by the CLI and the desktop app: Laravel
//! watchers and databases from `.env`. Secret values (passwords) are read
//! from `.env` and passed straight to the database; they are never returned,
//! logged or printed.

use crate::install;
use anyhow::{Context, bail};
use corral_core::dotenv;
use corral_core::paths::Paths;
use corral_services::manifest::Kind;
use corral_services::store::ServicesFile;
use serde::Serialize;
use std::path::Path;

pub fn is_laravel(project: &Path) -> bool {
    project.join("artisan").is_file()
}

pub fn watchers_enabled(project: &Path) -> bool {
    project.join("vendor/corral/laravel").exists()
}

fn composer(paths: &Paths, project: &Path, args: &[&str]) -> anyhow::Result<()> {
    let out = std::process::Command::new(paths.bin.join("composer"))
        .args(args)
        .args(["--no-interaction", "--no-ansi"])
        .current_dir(project)
        .env(
            "PATH",
            format!(
                "{}:{}",
                paths.bin.display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .output()?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let tail: Vec<&str> = err.lines().rev().take(8).collect();
        bail!(
            "composer {} failed:\n{}",
            args.join(" "),
            tail.into_iter().rev().collect::<Vec<_>>().join("\n")
        );
    }
    Ok(())
}

/// Add (or remove) the corral/laravel dev package via a Composer path repository.
pub fn set_watchers(paths: &Paths, project: &Path, enable: bool) -> anyhow::Result<()> {
    anyhow::ensure!(
        is_laravel(project),
        "{} is not a Laravel project",
        project.display()
    );
    anyhow::ensure!(
        install::composer_phar(paths).exists(),
        "Composer is not installed yet"
    );
    if enable {
        let dir = corral_dumps::events::write_laravel_package(&paths.root)?;
        let repo = serde_json::json!({
            "type": "path",
            "url": dir.display().to_string(),
            "options": { "symlink": true }
        })
        .to_string();
        composer(
            paths,
            project,
            &["config", "repositories.corral-laravel", &repo],
        )?;
        composer(paths, project, &["require", "--dev", "corral/laravel:@dev"])?;
    } else {
        composer(paths, project, &["remove", "--dev", "corral/laravel"])?;
        composer(
            paths,
            project,
            &["config", "--unset", "repositories.corral-laravel"],
        )?;
    }
    Ok(())
}

/// What a project's `.env` asks for (no secrets).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EnvDb {
    /// `mysql`, `mariadb`, `pgsql`, `sqlite`, …
    pub connection: String,
    pub database: Option<String>,
    pub username: Option<String>,
    pub has_password: bool,
    /// Corral service instance that would host it, if any.
    pub service: Option<String>,
    /// root with a password: Corral's root has none, so a dedicated user is needed.
    pub needs_dedicated_user: bool,
}

fn service_for(paths: &Paths, connection: &str) -> anyhow::Result<Option<(String, Kind)>> {
    let kind = match connection {
        "mysql" | "mariadb" => Kind::Mysql,
        "pgsql" | "postgres" => Kind::Postgres,
        _ => return Ok(None),
    };
    Ok(ServicesFile::load(paths)?
        .instances
        .into_iter()
        .find(|(_, i)| i.kind == kind)
        .map(|(n, _)| (n, kind)))
}

pub fn env_db(paths: &Paths, project: &Path) -> anyhow::Result<Option<EnvDb>> {
    let env = project.join(".env");
    let Some(connection) = dotenv::value(&env, "DB_CONNECTION") else {
        return Ok(None);
    };
    let username = dotenv::value(&env, "DB_USERNAME");
    let has_password = dotenv::value(&env, "DB_PASSWORD").is_some();
    let service = service_for(paths, &connection)?;
    let needs_dedicated_user = matches!(service, Some((_, Kind::Mysql)))
        && has_password
        && username.as_deref().is_none_or(|u| u == "root");
    Ok(Some(EnvDb {
        database: dotenv::value(&env, "DB_DATABASE"),
        service: service.map(|(n, _)| n),
        connection,
        username,
        has_password,
        needs_dedicated_user,
    }))
}

/// Create the `.env` database on the matching Corral service and provision
/// the `.env` user. With `dedicated_user`, first switch DB_USERNAME to it
/// (keeping `.env.corral-backup`). Returns a human-readable summary.
pub async fn create_db_from_env(
    paths: &Paths,
    project: &Path,
    dedicated_user: Option<&str>,
) -> anyhow::Result<String> {
    let env = project.join(".env");
    let info = env_db(paths, project)?.context("no DB_CONNECTION in .env")?;
    if let Some(u) = dedicated_user {
        anyhow::ensure!(
            corral_services::mysql::valid_user_name(u) && u != "root" && u != "postgres",
            "invalid user name {u:?}"
        );
        dotenv::set_value(&env, "DB_USERNAME", u)?;
    }
    let (service, kind) = service_for(paths, &info.connection)?.with_context(|| {
        format!(
            "add a {} service first",
            if info.connection == "pgsql" {
                "PostgreSQL"
            } else {
                "MySQL"
            }
        )
    })?;
    let file = ServicesFile::load(paths)?;
    let inst = file.get(&service)?;
    anyhow::ensure!(
        corral_services::port_in_use(inst.port),
        "{service} is not running; start Corral first"
    );
    let db = info.database.context("no DB_DATABASE in .env")?;
    match kind {
        Kind::Mysql => corral_services::mysql::create_database(paths, &service, inst, &db).await?,
        Kind::Postgres => corral_services::postgres::create_database(paths, inst, &db).await?,
        Kind::Valkey => unreachable!(),
    }
    let mut msg = format!(
        "Database `{db}` is ready on {service} (127.0.0.1:{}).",
        inst.port
    );
    let user = dotenv::value(&env, "DB_USERNAME").filter(|u| u != "root" && u != "postgres");
    if let Some(user) = user {
        let password = dotenv::value(&env, "DB_PASSWORD").unwrap_or_default();
        match kind {
            Kind::Mysql => {
                corral_services::mysql::ensure_app_user(
                    paths, &service, inst, &db, &user, &password,
                )
                .await?
            }
            Kind::Postgres => {
                corral_services::postgres::ensure_app_user(paths, inst, &db, &user, &password)
                    .await?
            }
            Kind::Valkey => unreachable!(),
        }
        msg.push_str(&format!(
            " User `{user}` can use it with the password from .env."
        ));
    }
    Ok(msg)
}
