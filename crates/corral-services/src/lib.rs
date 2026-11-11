//! Background services (databases etc.). Services are downloaded from
//! upstream with pinned digests, run under the user account via the
//! supervisor, listen on 127.0.0.1 only, and keep their data in
//! `services/data/<name>/`.

pub mod manifest;
pub mod mongodb;
pub mod mysql;
pub mod postgres;
pub mod store;
pub mod valkey;

use anyhow::{Context, bail};
use corral_core::paths::Paths;
use corral_supervisor::{State, Supervisor};
use manifest::{Kind, Release};
use std::path::PathBuf;
use std::time::Duration;
use store::{Instance, ServicesFile};

/// Download and unpack a release into its dist dir (no-op if present).
pub async fn install(paths: &Paths, release: &Release) -> anyhow::Result<PathBuf> {
    let dist = store::dist_dir(paths, release.kind, release.version);
    if dist.join("bin").is_dir() {
        return Ok(dist);
    }
    let art = release.artifact().with_context(|| {
        format!(
            "{} {} has no pinned build for this CPU yet",
            release.kind.label(),
            release.version
        )
    })?;
    let expected = corral_fetch::Expected {
        algo: corral_fetch::Hash::Sha256,
        hex: art.sha256.into(),
    };
    let archive =
        corral_fetch::fetch_verified(art.url, &paths.downloads_dir(), Some(&expected)).await?;
    // Source builds extract into a scratch tree, binary releases straight into dist.
    let target = match art.build {
        Some(_) => paths.root.join("services/build").join(format!(
            "{}-{}",
            release.kind.as_str(),
            release.version
        )),
        None => dist.clone(),
    };
    let extracted = tokio::task::spawn_blocking({
        let (archive, target) = (archive.clone(), target.clone());
        let strip = art.strip;
        move || corral_fetch::extract_tree(&archive, &target, strip)
    })
    .await?;
    let _ = std::fs::remove_file(&archive);
    extracted?;
    if let Some(build) = art.build {
        let result = compile(&target, &dist, build).await;
        let _ = std::fs::remove_dir_all(&target);
        result?;
    }
    Ok(dist)
}

/// Build from source: `./configure --prefix=<dist>` + `make` + `make
/// install` when the recipe has a configure step, otherwise `make` and copy
/// the listed binaries into `dist/bin`.
async fn compile(
    src: &std::path::Path,
    dist: &std::path::Path,
    build: manifest::Build,
) -> anyhow::Result<()> {
    let jobs = std::thread::available_parallelism().map_or(4, |n| n.get());
    tracing::info!(dir = %src.display(), jobs, "compiling from source");
    let make = |extra: &[&str]| {
        let mut a = vec![format!("-j{jobs}")];
        a.extend(build.make_args.iter().map(|s| s.to_string()));
        a.extend(extra.iter().map(|s| s.to_string()));
        a
    };
    if let Some(conf) = build.configure {
        let mut args = vec![format!("--prefix={}", dist.display())];
        args.extend(conf.iter().map(|s| s.to_string()));
        build_step(src, "./configure", &args).await?;
        build_step(src, "/usr/bin/make", &make(&[])).await?;
        build_step(src, "/usr/bin/make", &make(&["install"])).await?;
        return Ok(());
    }
    build_step(src, "/usr/bin/make", &make(&[])).await?;
    let bin = dist.join("bin");
    std::fs::create_dir_all(&bin)?;
    for b in build.binaries {
        let from = src.join(b);
        let name = from.file_name().context("binary path")?;
        std::fs::copy(&from, bin.join(name)).with_context(|| format!("copy {}", from.display()))?;
    }
    Ok(())
}

async fn build_step(src: &std::path::Path, prog: &str, args: &[String]) -> anyhow::Result<()> {
    let out = tokio::process::Command::new(prog)
        .args(args)
        .current_dir(src)
        .output()
        .await
        .with_context(|| {
            format!(
                "run {prog} (are the Xcode command line tools installed? `xcode-select --install`)"
            )
        })?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let tail: Vec<&str> = err.lines().rev().take(15).collect();
        bail!(
            "{prog} failed ({}):\n{}",
            out.status,
            tail.into_iter().rev().collect::<Vec<_>>().join("\n")
        );
    }
    Ok(())
}

/// Prepare an instance so it can start (config + data dir).
pub async fn prepare(paths: &Paths, name: &str, inst: &Instance) -> anyhow::Result<()> {
    match inst.kind {
        Kind::Mysql => mysql::ensure_initialized(paths, name, inst).await,
        Kind::Valkey => valkey::prepare(paths, name, inst),
        Kind::Postgres => postgres::ensure_initialized(paths, name, inst).await,
        Kind::Mongodb => mongodb::prepare(paths, name),
    }
}

async fn after_start(paths: &Paths, name: &str, inst: &Instance) -> anyhow::Result<()> {
    match inst.kind {
        Kind::Mysql => mysql::ensure_dev_accounts(paths, name, inst).await,
        Kind::Valkey => Ok(()),
        Kind::Postgres => postgres::ensure_dev_roles(paths, inst).await,
        Kind::Mongodb => Ok(()),
    }
}

fn spec(paths: &Paths, name: &str, inst: &Instance) -> corral_supervisor::Spec {
    match inst.kind {
        Kind::Mysql => mysql::spec(paths, name, inst),
        Kind::Valkey => valkey::spec(paths, name, inst),
        Kind::Postgres => postgres::spec(paths, name, inst),
        Kind::Mongodb => mongodb::spec(paths, name, inst),
    }
}

/// Wait until something accepts TCP connections on 127.0.0.1:`port`.
pub async fn wait_for_port(port: u16, timeout: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_ok()
        {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// True when `port` is already bound by something else on loopback.
pub fn port_in_use(port: u16) -> bool {
    std::net::TcpListener::bind(("127.0.0.1", port)).is_err()
}

/// Bring the supervisor in line with `services.toml`: start enabled instances,
/// restart those whose config changed, stop disabled or removed ones.
pub async fn sync(paths: &Paths, sup: &Supervisor) -> anyhow::Result<()> {
    let file = ServicesFile::load(paths)?;
    let running: Vec<String> = sup
        .states()
        .await
        .into_iter()
        .filter_map(|(n, _)| n.strip_prefix("service-").map(str::to_owned))
        .collect();

    for name in &running {
        if !file.instances.get(name).is_some_and(|i| i.enabled) {
            tracing::info!(service = %name, "stopping");
            sup.stop(&store::unit(name)).await.ok();
        }
    }
    for (name, inst) in file.instances.iter().filter(|(_, i)| i.enabled) {
        let changed = match inst.kind {
            Kind::Mysql => mysql::write_option_file(paths, name, inst)?,
            Kind::Valkey => valkey::write_config(paths, name, inst)?,
            Kind::Postgres | Kind::Mongodb => false, // configured with command-line flags
        };
        let state = sup.state(&store::unit(name)).await;
        let healthy = matches!(
            state,
            Some(State::Running { .. } | State::Starting | State::Backoff { .. })
        );
        if healthy && !changed {
            continue;
        }
        if healthy {
            tracing::info!(service = %name, "configuration changed; restarting");
            sup.stop(&store::unit(name)).await.ok();
        }
        if let Err(e) = prepare(paths, name, inst).await {
            tracing::error!(service = %name, error = %e, "cannot start service");
            continue;
        }
        if port_in_use(inst.port) {
            tracing::error!(service = %name, port = inst.port, "port already in use; not starting");
            continue;
        }
        sup.restart(spec(paths, name, inst)).await?;
        if wait_for_port(inst.port, Duration::from_secs(30)).await {
            if let Err(e) = after_start(paths, name, inst).await {
                tracing::error!(service = %name, error = %e, "post-start setup failed");
            }
            tracing::info!(service = %name, port = inst.port, "ready");
        } else {
            tracing::error!(service = %name, "did not open its port within 30s; see its log");
        }
    }
    write_shims(paths, &file)?;
    Ok(())
}

/// `mysql`/`mysqldump` shims for the first enabled MySQL instance, so CLI
/// tools talk to Corral's server without flags.
pub fn write_shims(paths: &Paths, file: &ServicesFile) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let first = file
        .instances
        .iter()
        .find(|(_, i)| i.kind == Kind::Mysql && i.enabled);
    write_valkey_shims(paths, file)?;
    for tool in ["mysql", "mysqldump", "mysqladmin"] {
        let shim = paths.bin.join(tool);
        match first {
            Some((name, inst)) => {
                let dist = store::dist_dir(paths, inst.kind, &inst.version);
                let body = format!(
                    "#!/bin/sh\n# Generated by Corral (service {name}).\nexec \"{}\" --defaults-extra-file=\"{}\" \"$@\"\n",
                    dist.join("bin").join(tool).display(),
                    store::conf_file(paths, name).display()
                );
                corral_core::fsutil::write_if_changed(&shim, body.as_bytes())?;
                std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755))?;
            }
            None => {
                if std::fs::read_to_string(&shim).is_ok_and(|s| s.contains("Generated by Corral")) {
                    std::fs::remove_file(&shim)?;
                }
            }
        }
    }
    Ok(())
}

/// `valkey-cli` and `redis-cli` shims for the first enabled Valkey instance.
fn write_valkey_shims(paths: &Paths, file: &ServicesFile) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let first = file
        .instances
        .iter()
        .find(|(_, i)| i.kind == Kind::Valkey && i.enabled);
    for tool in ["valkey-cli", "redis-cli"] {
        let shim = paths.bin.join(tool);
        match first {
            Some((_, inst)) => {
                let dist = store::dist_dir(paths, inst.kind, &inst.version);
                let body = format!(
                    "#!/bin/sh\n# Generated by Corral.\nexec \"{}\" -h 127.0.0.1 -p {} \"$@\"\n",
                    dist.join("bin/valkey-cli").display(),
                    inst.port
                );
                corral_core::fsutil::write_if_changed(&shim, body.as_bytes())?;
                std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755))?;
            }
            None => {
                if std::fs::read_to_string(&shim).is_ok_and(|s| s.contains("Generated by Corral")) {
                    std::fs::remove_file(&shim)?;
                }
            }
        }
    }
    Ok(())
}

/// Add a new instance: install the release if needed, initialize data, save.
pub async fn add(
    paths: &Paths,
    kind: Kind,
    version: Option<&str>,
    name: Option<&str>,
    port: Option<u16>,
) -> anyhow::Result<(String, Instance)> {
    let release = manifest::find(kind, version).with_context(|| {
        format!(
            "no {} release matching {:?} in the catalog",
            kind.label(),
            version.unwrap_or("latest")
        )
    })?;
    let mut file = ServicesFile::load(paths)?;
    let name = name.unwrap_or(kind.as_str()).to_string();
    let port = port.unwrap_or_else(|| file.free_port(kind.default_port()));
    let inst = Instance {
        kind,
        version: release.version.into(),
        port,
        enabled: true,
    };
    // Validate before the (large) download.
    file.add(&name, inst.clone())?;
    if port_in_use(port) {
        bail!("port {port} is already in use on 127.0.0.1; pass --port");
    }
    install(paths, release).await?;
    prepare(paths, &name, &inst).await?;
    file.save(paths)?;
    Ok((name, inst))
}

/// What [`uninstall`] removed, for the UI.
#[derive(Debug, Default, serde::Serialize)]
pub struct Removed {
    pub runtime: bool,
    pub data: bool,
    pub logs: usize,
}

/// Completely remove an instance: stop it (the engine stops disabled
/// services), wait until its port is free so nothing is deleted while the
/// database is still writing, then delete its config, logs, the downloaded
/// runtime (if no other instance uses that version) and, with `delete_data`,
/// its data directory.
pub async fn uninstall(paths: &Paths, name: &str, delete_data: bool) -> anyhow::Result<Removed> {
    let mut file = ServicesFile::load(paths)?;
    let inst = file.get(name)?.clone();
    if inst.enabled {
        file.set_enabled(name, false)?;
        file.save(paths)?;
    }
    // A running Corral stops it within ~1s; databases may take a while to flush.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(90);
    while port_in_use(inst.port) {
        if tokio::time::Instant::now() >= deadline {
            bail!(
                "{name} is still running on port {}; stop it and try again",
                inst.port
            );
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let mut file = ServicesFile::load(paths)?;
    file.remove(name)?;
    file.save(paths)?;

    let mut out = Removed::default();
    let _ = std::fs::remove_file(store::conf_file(paths, name));
    let _ = std::fs::remove_file(store::socket(paths, name));
    for e in std::fs::read_dir(paths.logs_dir())
        .into_iter()
        .flatten()
        .flatten()
    {
        let n = e.file_name().to_string_lossy().into_owned();
        let ours = n.starts_with(&format!("service-{name}."))
            || n.starts_with(&format!("service-{name}-"));
        if ours && std::fs::remove_file(e.path()).is_ok() {
            out.logs += 1;
        }
    }
    let shared = file
        .instances
        .values()
        .any(|i| i.kind == inst.kind && i.version == inst.version);
    if !shared {
        let dist = store::dist_dir(paths, inst.kind, &inst.version);
        anyhow::ensure!(paths.owns(&dist), "refusing to delete {}", dist.display());
        if dist.exists() {
            std::fs::remove_dir_all(&dist)?;
            out.runtime = true;
        }
    }
    if delete_data {
        let data = store::data_dir(paths, name);
        anyhow::ensure!(paths.owns(&data), "refusing to delete {}", data.display());
        if data.exists() {
            std::fs::remove_dir_all(&data)?;
            out.data = true;
        }
    }
    write_shims(paths, &file)?;
    Ok(out)
}

/// Remove an instance; optionally delete its data directory.
pub fn remove(paths: &Paths, name: &str, delete_data: bool) -> anyhow::Result<()> {
    let mut file = ServicesFile::load(paths)?;
    file.remove(name)?;
    file.save(paths)?;
    let _ = std::fs::remove_file(store::conf_file(paths, name));
    if delete_data {
        let data = store::data_dir(paths, name);
        anyhow::ensure!(paths.owns(&data), "refusing to delete {}", data.display());
        if data.exists() {
            std::fs::remove_dir_all(&data)?;
        }
    }
    write_shims(paths, &file)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use manifest::Kind;

    #[tokio::test]
    async fn uninstall_removes_everything_but_shared_runtime() {
        let t = tempfile::tempdir().unwrap();
        let p = Paths::at(t.path());
        p.ensure().unwrap();
        let mut f = ServicesFile::default();
        // Ports nothing listens on, so no waiting.
        for (n, port) in [("db1", 39111u16), ("db2", 39112)] {
            f.add(
                n,
                Instance {
                    kind: Kind::Mysql,
                    version: "8.4.11".into(),
                    port,
                    enabled: true,
                },
            )
            .unwrap();
            std::fs::create_dir_all(store::data_dir(&p, n)).unwrap();
            std::fs::write(store::data_dir(&p, n).join("ibdata1"), "x").unwrap();
            std::fs::write(p.logs_dir().join(format!("service-{n}.err.log")), "x").unwrap();
        }
        f.save(&p).unwrap();
        let dist = store::dist_dir(&p, Kind::Mysql, "8.4.11");
        std::fs::create_dir_all(dist.join("bin")).unwrap();

        let r = uninstall(&p, "db1", true).await.unwrap();
        assert!(r.data && !r.runtime && r.logs == 1, "{r:?}");
        assert!(dist.exists(), "db2 still uses the runtime");
        assert!(!store::data_dir(&p, "db1").exists());

        let r = uninstall(&p, "db2", false).await.unwrap();
        assert!(r.runtime && !r.data, "{r:?}");
        assert!(!dist.exists());
        assert!(
            store::data_dir(&p, "db2").exists(),
            "data kept when not asked to delete"
        );
        assert!(ServicesFile::load(&p).unwrap().instances.is_empty());
    }
}
