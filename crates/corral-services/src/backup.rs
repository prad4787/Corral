//! Back up and restore service data with each server's own tools.
//!
//! | Kind       | Formats                                  | Scope              |
//! |------------|------------------------------------------|--------------------|
//! | MySQL      | `.sql`, `.sql.gz` (mysqldump)            | one or all DBs     |
//! | PostgreSQL | `.sql`, `.sql.gz` (pg_dump/pg_dumpall),  | one or all DBs     |
//! |            | `.dump` (pg_dump custom format)          | one DB             |
//! | Valkey     | `.rdb` snapshot (valkey-cli --rdb)       | whole server       |
//! | MongoDB    | `.archive.gz` (mongodump, if installed)  | one or all DBs     |
//!
//! Backups are written to a hidden temporary file next to the destination
//! and renamed on success, so a failed backup never leaves a truncated file
//! that looks real. SQL travels through pipes, never argv.

use crate::manifest::Kind;
use crate::store::{self, Instance, ServicesFile};
use crate::{mysql, port_in_use, postgres};
use anyhow::{Context, bail};
use corral_core::paths::Paths;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Format {
    /// Plain SQL text.
    Sql,
    /// Gzip-compressed SQL text.
    SqlGz,
    /// `pg_dump -Fc`: compressed, restorable selectively with `pg_restore`.
    PgCustom,
    /// Valkey/Redis RDB snapshot.
    Rdb,
    /// `mongodump --archive --gzip`.
    MongoArchive,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Format::Sql => "sql",
            Format::SqlGz => "sql.gz",
            Format::PgCustom => "dump",
            Format::Rdb => "rdb",
            Format::MongoArchive => "archive.gz",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Format::Sql => "SQL (.sql)",
            Format::SqlGz => "Compressed SQL (.sql.gz)",
            Format::PgCustom => "PostgreSQL custom (.dump)",
            Format::Rdb => "RDB snapshot (.rdb)",
            Format::MongoArchive => "Mongo archive (.archive.gz)",
        }
    }
    /// Whether this format can hold every database of the server at once.
    pub fn whole_server(self) -> bool {
        !matches!(self, Format::PgCustom)
    }
}

/// What the UI needs to offer a backup or restore for one instance.
#[derive(Debug, Serialize)]
pub struct Options {
    pub formats: Vec<FormatInfo>,
    /// User databases (empty when the kind has no per-database backups).
    pub databases: Vec<String>,
    /// False for Valkey: snapshots always contain the whole server.
    pub per_database: bool,
    /// Why backups are unavailable right now, if they are.
    pub unavailable: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct FormatInfo {
    pub id: Format,
    pub label: &'static str,
    pub extension: &'static str,
    pub whole_server: bool,
}

fn dist(paths: &Paths, inst: &Instance) -> PathBuf {
    store::dist_dir(paths, inst.kind, &inst.version)
}

fn tool(paths: &Paths, inst: &Instance, name: &str) -> PathBuf {
    dist(paths, inst).join("bin").join(name)
}

fn formats_for(kind: Kind) -> &'static [Format] {
    match kind {
        Kind::Mysql => &[Format::Sql, Format::SqlGz],
        Kind::Postgres => &[Format::Sql, Format::SqlGz, Format::PgCustom],
        Kind::Valkey => &[Format::Rdb],
        Kind::Mongodb => &[Format::MongoArchive],
    }
}

fn instance(paths: &Paths, name: &str) -> anyhow::Result<Instance> {
    Ok(ServicesFile::load(paths)?.get(name)?.clone())
}

fn ensure_running(name: &str, inst: &Instance) -> anyhow::Result<()> {
    if !port_in_use(inst.port) {
        bail!("{name} isn’t running. Start Corral and turn the service on first.");
    }
    Ok(())
}

pub async fn options(paths: &Paths, name: &str) -> anyhow::Result<Options> {
    let inst = instance(paths, name)?;
    let formats = formats_for(inst.kind)
        .iter()
        .map(|&f| FormatInfo {
            id: f,
            label: f.label(),
            extension: f.extension(),
            whole_server: f.whole_server(),
        })
        .collect();
    let running = port_in_use(inst.port);
    let mut unavailable = (!running)
        .then(|| format!("{name} isn’t running. Start Corral and turn the service on first."));
    if inst.kind == Kind::Mongodb && !tool(paths, &inst, "mongodump").is_file() {
        unavailable = Some(
            "MongoDB backups need the MongoDB Database Tools (mongodump), which Corral doesn’t install yet."
                .into(),
        );
    }
    let databases = if running {
        match inst.kind {
            Kind::Mysql => mysql::list_databases(paths, name, &inst).await?,
            Kind::Postgres => postgres::list_databases(paths, &inst).await?,
            Kind::Valkey | Kind::Mongodb => Vec::new(),
        }
    } else {
        Vec::new()
    };
    Ok(Options {
        formats,
        databases,
        per_database: matches!(inst.kind, Kind::Mysql | Kind::Postgres),
        unavailable,
    })
}

/// Run a pipeline of commands (`a | b | c`). `input` feeds the first, the
/// last writes to `output`. Each stderr is collected on its own thread so a
/// chatty tool can never block the pipe.
fn pipeline(
    cmds: Vec<Command>,
    input: Option<std::fs::File>,
    output: Option<std::fs::File>,
) -> anyhow::Result<()> {
    let n = cmds.len();
    let mut children = Vec::with_capacity(n);
    let mut prev: Option<Stdio> = input.map(Stdio::from);
    for (i, mut c) in cmds.into_iter().enumerate() {
        c.stdin(prev.take().unwrap_or_else(Stdio::null));
        if i + 1 == n {
            match &output {
                Some(f) => c.stdout(Stdio::from(f.try_clone()?)),
                None => c.stdout(Stdio::null()),
            };
        } else {
            c.stdout(Stdio::piped());
        }
        c.stderr(Stdio::piped());
        let prog = c.get_program().to_string_lossy().into_owned();
        let mut child = c.spawn().with_context(|| format!("run {prog}"))?;
        if i + 1 < n {
            prev = Some(Stdio::from(child.stdout.take().expect("piped stdout")));
        }
        let mut err = child.stderr.take().expect("piped stderr");
        let reader = std::thread::spawn(move || {
            let mut s = String::new();
            let _ = err.read_to_string(&mut s);
            s
        });
        children.push((prog, child, reader));
    }
    let mut failure = None;
    for (prog, mut child, reader) in children {
        let status = child.wait()?;
        let stderr = reader.join().unwrap_or_default();
        if !status.success() && failure.is_none() {
            let name = Path::new(&prog)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or(prog);
            let tail: Vec<&str> = stderr.lines().rev().take(10).collect();
            failure = Some(format!(
                "{name} failed ({status}):\n{}",
                tail.into_iter().rev().collect::<Vec<_>>().join("\n")
            ));
        }
    }
    match failure {
        Some(f) => bail!(f),
        None => Ok(()),
    }
}

fn gzip_compress() -> Command {
    let mut c = Command::new("/usr/bin/gzip");
    c.arg("-c");
    c
}

fn gzip_decompress(file: &Path) -> Command {
    let mut c = Command::new("/usr/bin/gzip");
    c.arg("-dc").arg(file);
    c
}

fn mysql_defaults(paths: &Paths, name: &str) -> String {
    format!(
        "--defaults-file={}",
        store::conf_file(paths, name).display()
    )
}

fn pg_conn(c: &mut Command, paths: &Paths, name: &str, inst: &Instance) {
    c.args([
        "-h",
        "127.0.0.1",
        "-p",
        &inst.port.to_string(),
        "-U",
        "postgres",
    ]);
    if let Some(pw) = crate::auth::password(paths, name) {
        c.env("PGPASSWORD", pw);
    }
}

/// Write a backup to `dest`. `database: None` backs up every user database.
/// Returns the size of the written file in bytes.
pub async fn backup(
    paths: &Paths,
    name: &str,
    database: Option<&str>,
    format: Format,
    dest: &Path,
) -> anyhow::Result<u64> {
    let inst = instance(paths, name)?;
    ensure_running(name, &inst)?;
    if !formats_for(inst.kind).contains(&format) {
        bail!(
            "{} can’t be backed up as {}",
            inst.kind.label(),
            format.label()
        );
    }
    if let Some(db) = database
        && !mysql::valid_db_name(db)
    {
        bail!("invalid database name {db:?}");
    }
    if database.is_none() && !format.whole_server() {
        bail!("{} holds a single database; choose one", format.label());
    }
    // MySQL: name the databases explicitly so system schemas (and their
    // accounts) are never part of a backup.
    let mysql_all = match (inst.kind, database) {
        (Kind::Mysql, None) => {
            let dbs = mysql::list_databases(paths, name, &inst).await?;
            if dbs.is_empty() {
                bail!("{name} has no databases to back up");
            }
            dbs
        }
        _ => Vec::new(),
    };

    let mut cmds: Vec<Command> = Vec::new();
    match inst.kind {
        Kind::Mysql => {
            let mut c = Command::new(tool(paths, &inst, "mysqldump"));
            c.arg(mysql_defaults(paths, name)).args([
                "--single-transaction",
                "--routines",
                "--triggers",
                "--events",
                "--hex-blob",
                "--default-character-set=utf8mb4",
                "--set-gtid-purged=OFF",
            ]);
            match database {
                // No CREATE DATABASE / USE: restorable into any database name.
                Some(db) => {
                    c.arg(db);
                }
                None => {
                    c.arg("--databases").args(&mysql_all);
                }
            }
            cmds.push(c);
        }
        Kind::Postgres => {
            let mut c = match database {
                Some(db) => {
                    let mut c = Command::new(tool(paths, &inst, "pg_dump"));
                    pg_conn(&mut c, paths, name, &inst);
                    c.args(["-d", db]);
                    if format == Format::PgCustom {
                        c.arg("-Fc");
                    } else {
                        // Plain SQL that loads into any server and role setup.
                        c.args(["--no-owner", "--no-privileges"]);
                    }
                    c
                }
                None => {
                    let mut c = Command::new(tool(paths, &inst, "pg_dumpall"));
                    pg_conn(&mut c, paths, name, &inst);
                    c
                }
            };
            c.arg("--no-password");
            cmds.push(c);
        }
        Kind::Valkey => {
            // valkey-cli writes the snapshot to a path itself.
            let tmp = partial_path(dest);
            let mut cli = tokio::process::Command::new(tool(paths, &inst, "valkey-cli"));
            if let Some(pw) = crate::auth::password(paths, name) {
                cli.env("REDISCLI_AUTH", pw);
            }
            let out = cli
                .args(["-h", "127.0.0.1", "-p", &inst.port.to_string(), "--rdb"])
                .arg(&tmp)
                .output()
                .await
                .context("run valkey-cli")?;
            if !out.status.success() || !tmp.is_file() {
                let _ = std::fs::remove_file(&tmp);
                bail!(
                    "valkey-cli failed: {}{}",
                    String::from_utf8_lossy(&out.stderr).trim(),
                    String::from_utf8_lossy(&out.stdout).trim()
                );
            }
            return finish(&tmp, dest);
        }
        Kind::Mongodb => {
            let bin = tool(paths, &inst, "mongodump");
            if !bin.is_file() {
                bail!("mongodump isn’t installed with this MongoDB");
            }
            let mut c = Command::new(bin);
            c.arg(format!("--uri=mongodb://127.0.0.1:{}", inst.port))
                .args(["--archive", "--gzip", "--quiet"]);
            if let Some(db) = database {
                c.arg(format!("--db={db}"));
            }
            cmds.push(c);
        }
    }
    if format == Format::SqlGz {
        cmds.push(gzip_compress());
    }

    let tmp = partial_path(dest);
    let file = std::fs::File::create(&tmp).with_context(|| format!("create {}", tmp.display()))?;
    let result = tokio::task::spawn_blocking(move || pipeline(cmds, None, Some(file))).await?;
    if let Err(e) = result {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    finish(&tmp, dest)
}

fn partial_path(dest: &Path) -> PathBuf {
    let name = dest
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "backup".into());
    dest.with_file_name(format!(".{name}.partial"))
}

fn finish(tmp: &Path, dest: &Path) -> anyhow::Result<u64> {
    let size = std::fs::metadata(tmp)?.len();
    if size == 0 {
        let _ = std::fs::remove_file(tmp);
        bail!("the backup came out empty");
    }
    std::fs::rename(tmp, dest).with_context(|| format!("save {}", dest.display()))?;
    Ok(size)
}

/// What a backup file holds, from its first bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Detected {
    Sql,
    SqlGz,
    PgCustom,
    Rdb,
    /// gzip that is not SQL text, e.g. a Mongo archive.
    OtherGz,
}

pub fn detect(file: &Path) -> anyhow::Result<Detected> {
    let mut head = [0u8; 5];
    let n = std::fs::File::open(file)
        .with_context(|| format!("open {}", file.display()))?
        .read(&mut head)?;
    let head = &head[..n];
    let name = file.to_string_lossy().to_lowercase();
    Ok(if head.starts_with(b"PGDMP") {
        Detected::PgCustom
    } else if head.starts_with(b"REDIS") {
        Detected::Rdb
    } else if head.starts_with(&[0x1f, 0x8b]) {
        if !name.contains(".archive") {
            Detected::SqlGz
        } else {
            Detected::OtherGz
        }
    } else {
        Detected::Sql
    })
}

/// Restore `file` into the instance. For MySQL and PostgreSQL, `database`
/// names the target (created when missing); `None` replays a whole-server
/// backup that creates its own databases.
pub async fn restore(
    paths: &Paths,
    name: &str,
    database: Option<&str>,
    file: &Path,
) -> anyhow::Result<()> {
    let inst = instance(paths, name)?;
    if let Some(db) = database
        && !mysql::valid_db_name(db)
    {
        bail!("invalid database name {db:?}: use letters, digits, '_' and '-'");
    }
    let kind = detect(file)?;
    match inst.kind {
        Kind::Valkey => {
            if kind != Detected::Rdb {
                bail!("that file isn’t an RDB snapshot");
            }
            return restore_valkey(paths, name, &inst, file).await;
        }
        Kind::Mongodb => {
            ensure_running(name, &inst)?;
            let bin = tool(paths, &inst, "mongorestore");
            if !bin.is_file() {
                bail!("mongorestore isn’t installed with this MongoDB");
            }
            let mut c = Command::new(bin);
            c.arg(format!("--uri=mongodb://127.0.0.1:{}", inst.port))
                .arg(format!("--archive={}", file.display()))
                .args(["--gzip", "--drop", "--quiet"]);
            if let Some(db) = database {
                c.arg(format!("--nsInclude={db}.*"));
            }
            return run_blocking(vec![c], None).await;
        }
        Kind::Mysql | Kind::Postgres => ensure_running(name, &inst)?,
    }
    if matches!(kind, Detected::Rdb | Detected::OtherGz) {
        bail!("that file isn’t a SQL backup");
    }
    if let Some(db) = database {
        match inst.kind {
            Kind::Mysql => mysql::create_database(paths, name, &inst, db).await?,
            _ => postgres::create_database(paths, &inst, db).await?,
        }
    }

    let mut cmds = Vec::new();
    let mut input = None;
    match kind {
        Detected::SqlGz => cmds.push(gzip_decompress(file)),
        Detected::Sql => input = Some(std::fs::File::open(file)?),
        _ => {}
    }
    match (inst.kind, kind) {
        (Kind::Mysql, Detected::PgCustom) => bail!("that’s a PostgreSQL backup"),
        (Kind::Mysql, _) => {
            let mut c = Command::new(mysql::client(&dist(paths, &inst)));
            c.arg(mysql_defaults(paths, name))
                .arg("--default-character-set=utf8mb4");
            if let Some(db) = database {
                c.arg(db);
            }
            cmds.push(c);
        }
        (_, Detected::PgCustom) => {
            let db = database.context("choose the database to restore into")?;
            let mut c = Command::new(tool(paths, &inst, "pg_restore"));
            pg_conn(&mut c, paths, name, &inst);
            c.args(["-d", db, "--no-password", "--no-owner", "--no-privileges"])
                .args(["--clean", "--if-exists", "--exit-on-error"])
                .arg(file);
            cmds.push(c);
        }
        (_, _) => {
            let mut c = Command::new(tool(paths, &inst, "psql"));
            pg_conn(&mut c, paths, name, &inst);
            c.args(["-X", "-q", "--no-password", "-v", "ON_ERROR_STOP=1"]);
            match database {
                // One transaction: a failed restore leaves the database as it was.
                Some(db) => c.args(["-d", db, "--single-transaction"]),
                // pg_dumpall output runs CREATE DATABASE, which can't be in a transaction.
                None => c.args(["-d", "postgres"]),
            };
            cmds.push(c);
        }
    }
    run_blocking(cmds, input).await
}

async fn run_blocking(cmds: Vec<Command>, input: Option<std::fs::File>) -> anyhow::Result<()> {
    tokio::task::spawn_blocking(move || pipeline(cmds, input, None))
        .await?
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("extension \"") && msg.contains("is not available") {
                anyhow::anyhow!(
                    "{msg}\n\nThe backup uses a PostgreSQL extension that isn’t installed. \
                     Choose Install Extensions under PostgreSQL in Services, then restore again."
                )
            } else {
                e
            }
        })
}

/// Stop Valkey, swap in the snapshot (keeping the old one as
/// `dump.rdb.before-restore`), then start it again if it was on.
async fn restore_valkey(
    paths: &Paths,
    name: &str,
    inst: &Instance,
    file: &Path,
) -> anyhow::Result<()> {
    let was_enabled = inst.enabled;
    if was_enabled {
        let mut f = ServicesFile::load(paths)?;
        f.set_enabled(name, false)?;
        f.save(paths)?;
    }
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    while port_in_use(inst.port) {
        if tokio::time::Instant::now() >= deadline {
            bail!("{name} didn’t stop; try again");
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    // Valkey writes its own snapshot while exiting; let it finish.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let data = store::data_dir(paths, name);
    std::fs::create_dir_all(&data)?;
    let target = data.join("dump.rdb");
    let swap = (|| -> anyhow::Result<()> {
        if target.exists() {
            std::fs::rename(&target, data.join("dump.rdb.before-restore"))?;
        }
        std::fs::copy(file, &target).with_context(|| format!("copy {}", file.display()))?;
        Ok(())
    })();
    if was_enabled {
        let mut f = ServicesFile::load(paths)?;
        f.set_enabled(name, true)?;
        f.save(paths)?;
    }
    swap
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_formats() {
        let t = tempfile::tempdir().unwrap();
        let w = |n: &str, b: &[u8]| {
            let p = t.path().join(n);
            std::fs::write(&p, b).unwrap();
            p
        };
        assert_eq!(
            detect(&w("a.sql", b"-- MySQL dump")).unwrap(),
            Detected::Sql
        );
        assert_eq!(
            detect(&w("a.dump", b"PGDMP\x01")).unwrap(),
            Detected::PgCustom
        );
        assert_eq!(detect(&w("a.rdb", b"REDIS0011")).unwrap(), Detected::Rdb);
        assert_eq!(
            detect(&w("a.sql.gz", &[0x1f, 0x8b, 8])).unwrap(),
            Detected::SqlGz
        );
        assert_eq!(
            detect(&w("m.archive.gz", &[0x1f, 0x8b, 8])).unwrap(),
            Detected::OtherGz
        );
    }

    #[test]
    fn pipeline_pipes_and_reports_failures() {
        let t = tempfile::tempdir().unwrap();
        let out = t.path().join("out.gz");
        let mut echo = Command::new("/bin/echo");
        echo.arg("hello");
        pipeline(
            vec![echo, gzip_compress()],
            None,
            Some(std::fs::File::create(&out).unwrap()),
        )
        .unwrap();
        let back = Command::new("/usr/bin/gzip")
            .arg("-dc")
            .arg(&out)
            .output()
            .unwrap();
        assert_eq!(back.stdout, b"hello\n");

        let e = pipeline(vec![Command::new("/usr/bin/false")], None, None).unwrap_err();
        assert!(e.to_string().contains("false failed"), "{e}");
    }

    #[test]
    fn partial_is_hidden_sibling() {
        let p = partial_path(Path::new("/x/y/db.sql.gz"));
        assert_eq!(p, Path::new("/x/y/.db.sql.gz.partial"));
    }
}
