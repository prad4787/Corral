//! Background services (databases etc.). Services are downloaded from
//! upstream with pinned digests, run under the user account via the
//! supervisor, listen on 127.0.0.1 only, and keep their data in
//! `services/data/<name>/`.

pub mod manifest;
pub mod mysql;
pub mod store;

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
    let extracted = tokio::task::spawn_blocking({
        let (archive, dist) = (archive.clone(), dist.clone());
        let strip = art.strip;
        move || corral_fetch::extract_tree(&archive, &dist, strip)
    })
    .await?;
    let _ = std::fs::remove_file(&archive);
    extracted?;
    Ok(dist)
}

/// Prepare an instance so it can start (config + data dir).
pub async fn prepare(paths: &Paths, name: &str, inst: &Instance) -> anyhow::Result<()> {
    match inst.kind {
        Kind::Mysql => mysql::ensure_initialized(paths, name, inst).await,
    }
}

async fn after_start(paths: &Paths, name: &str, inst: &Instance) -> anyhow::Result<()> {
    match inst.kind {
        Kind::Mysql => mysql::ensure_dev_accounts(paths, name, inst).await,
    }
}

fn spec(paths: &Paths, name: &str, inst: &Instance) -> corral_supervisor::Spec {
    match inst.kind {
        Kind::Mysql => mysql::spec(paths, name, inst),
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
