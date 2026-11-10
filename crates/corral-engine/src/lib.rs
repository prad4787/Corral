//! The engine turns Corral's state (config + site registry + installed PHP)
//! into running processes.
//!
//! `render` is pure file generation and is safe to call at any time; `run`
//! starts DNS, one FPM pool per PHP version in use, and Caddy, then re-renders
//! whenever `config.toml` or `sites.toml` change.

pub mod install;
pub mod node;
pub mod orphans;

use anyhow::Context;
use corral_core::paths::Paths;
use corral_core::php::{self, Installation, PhpVersion};
use corral_core::sites::{Registry, Site};
use corral_core::{Config, fsutil::write_atomic};
use corral_proxy::{CertFiles, ProxySettings};
use corral_supervisor::{Spec, Supervisor};
use corral_tls::LocalCa;
use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

/// Result of a render pass.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Rendered {
    pub sites: Vec<Site>,
    /// PHP versions that need a running FPM pool.
    pub fpm_versions: Vec<String>,
    /// Sites whose PHP version is not installed (served with 503 or fallback).
    pub missing_php: Vec<(String, String)>,
    /// Versions whose generated php.ini / FPM config changed in this render;
    /// their running pools must restart to pick it up.
    pub changed_php: Vec<String>,
    pub caddy_config: PathBuf,
    pub ca_fingerprint: String,
}

pub fn caddy_config_path(paths: &Paths) -> PathBuf {
    paths.generated_dir().join("caddy.json")
}

/// Generate every config file from current state. Idempotent.
pub fn render(paths: &Paths) -> anyhow::Result<Rendered> {
    paths.check_socket_paths().map_err(anyhow::Error::msg)?;
    paths.ensure()?;
    let cfg = Config::load(paths)?;
    let sites = Registry::load(paths)?.resolve(&cfg)?;
    let installs = php::installed(paths)?;
    let versions: Vec<PhpVersion> = installs.iter().map(|i| i.version).collect();
    let debuggable: Vec<PhpVersion> = installs
        .iter()
        .filter(|i| php::xdebug_available(i))
        .map(|i| i.version)
        .collect();
    let default = cfg.default_php.as_deref().and_then(PhpVersion::parse);

    // The prepend file must exist before any ini references it.
    let prepend = corral_dumps::prepend_path(&paths.root);
    let prepend = if cfg.dumps {
        corral_core::fsutil::write_if_changed(
            &prepend,
            corral_dumps::prepend_php(&corral_dumps::socket_path(&paths.root)).as_bytes(),
        )?;
        corral_core::fsutil::write_if_changed(
            &corral_dumps::render_path(&paths.root),
            corral_dumps::render_php().as_bytes(),
        )?;
        corral_dumps::events::write_laravel_package(&paths.root)?;
        Some(prepend)
    } else {
        None
    };
    let mut changed_php = Vec::new();
    for inst in &installs {
        if php::write_configs(paths, inst, prepend.as_deref())? {
            changed_php.push(inst.version.to_string());
        }
    }
    php::write_shims(paths, default)?;

    let ca = LocalCa::load_or_create(paths, &cfg.tld)?;
    let mut leaves = std::collections::HashMap::new();
    for s in sites.iter().filter(|s| s.secure) {
        let f = ca.ensure_leaf(&s.host)?;
        leaves.insert(s.host.clone(), f);
    }
    let keep: Vec<&str> = leaves.keys().map(String::as_str).collect();
    ca.prune_leaves(&keep)?;

    let proxy_sites = corral_proxy::proxy_sites(paths, &sites, &versions, &debuggable, |h| {
        let f = &leaves[h];
        CertFiles {
            cert: f.cert.clone(),
            key: f.key.clone(),
        }
    });
    let settings = ProxySettings::from_config(paths, &cfg);
    let caddy = corral_proxy::caddy_config(&settings, &proxy_sites);
    let caddy_path = caddy_config_path(paths);
    write_atomic(
        &caddy_path,
        serde_json::to_string_pretty(&caddy)?.as_bytes(),
    )?;

    let mut fpm: BTreeSet<String> = BTreeSet::new();
    let mut missing = Vec::new();
    for (site, ps) in sites.iter().zip(&proxy_sites) {
        if let Some(sock) = &ps.fpm_socket {
            let v = sock
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_prefix("php-fpm-"))
                .and_then(|n| n.strip_suffix(".sock"))
                .unwrap_or_default();
            fpm.insert(v.to_string());
        }
        if let Some(want) = site.php.as_deref()
            && site.driver.front_controller.is_some()
            && !PhpVersion::parse(want).is_some_and(|v| versions.contains(&v))
        {
            missing.push((site.name.clone(), want.to_string()));
        }
    }

    Ok(Rendered {
        sites,
        fpm_versions: fpm.into_iter().collect(),
        missing_php: missing,
        changed_php,
        caddy_config: caddy_path,
        ca_fingerprint: ca.fingerprint()?,
    })
}

/// The FPM pools for one installation: the normal pool, plus an Xdebug pool
/// when the build ships xdebug.so. Returns (spec, socket) pairs.
fn pool_specs(paths: &Paths, inst: &Installation) -> Vec<(Spec, std::path::PathBuf)> {
    let v = inst.version.to_string();
    let log = |suffix: &str| {
        paths
            .logs_dir()
            .join(format!("php-fpm-{v}{suffix}.out.log"))
    };
    let mut normal =
        Spec::new(format!("php-fpm-{v}"), inst.fpm(), log("")).args(php::fpm_args(inst));
    normal.env = php::php_env(inst);
    let mut out = vec![(normal, paths.fpm_socket(&v))];
    if php::xdebug_available(inst) {
        let mut dbg = Spec::new(format!("php-fpm-{v}-debug"), inst.fpm(), log("-debug"))
            .args(php::fpm_debug_args(inst));
        dbg.env = php::php_env(inst);
        out.push((dbg, paths.fpm_debug_socket(&v)));
    }
    out
}

async fn start_pool(sup: &Supervisor, spec: Spec, sock: &std::path::Path) -> anyhow::Result<()> {
    // A stale socket from a crash would make FPM refuse to start.
    let _ = std::fs::remove_file(sock);
    let name = spec.name.clone();
    sup.start(spec).await?;
    if !wait_for_socket(sock, Duration::from_secs(10)).await {
        tracing::error!(pool = %name, "PHP-FPM pool did not become ready; see its log");
    }
    Ok(())
}

fn caddy_spec(paths: &Paths) -> Spec {
    let data = paths.root.join("caddy");
    Spec::new(
        "caddy",
        install::caddy_bin(paths),
        paths.logs_dir().join("caddy.log"),
    )
    .args([
        "run".to_string(),
        "--config".into(),
        caddy_config_path(paths).display().to_string(),
        "--watch".into(),
    ])
    .env("XDG_DATA_HOME", data.join("data").display().to_string())
    .env("XDG_CONFIG_HOME", data.join("config").display().to_string())
}

fn mtimes(paths: &Paths) -> Vec<Option<SystemTime>> {
    let parked = Config::load(paths).map(|c| c.parked).unwrap_or_default();
    [
        paths.config_file(),
        paths.sites_file(),
        corral_services::store::services_file(paths),
    ]
    .into_iter()
    .chain(parked)
    .map(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
    .collect()
}

/// Per installed PHP version: (file name, mtime, size) of every conf.d entry.
fn user_ini_fingerprints(
    paths: &Paths,
) -> std::collections::BTreeMap<String, Vec<(String, Option<SystemTime>, u64)>> {
    php::installed(paths)
        .unwrap_or_default()
        .into_iter()
        .map(|inst| {
            let mut files: Vec<_> = std::fs::read_dir(inst.conf_d())
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| {
                    let m = e.metadata().ok();
                    (
                        e.file_name().to_string_lossy().into_owned(),
                        m.as_ref().and_then(|m| m.modified().ok()),
                        m.map_or(0, |m| m.len()),
                    )
                })
                .collect();
            files.sort();
            (inst.version.to_string(), files)
        })
        .collect()
}

/// Restart running pools for `versions` (their config changed).
async fn restart_fpm(paths: &Paths, sup: &Supervisor, versions: &[String]) -> anyhow::Result<()> {
    for v in versions {
        let Some(inst) = PhpVersion::parse(v).and_then(|pv| php::find(paths, pv).ok().flatten())
        else {
            continue;
        };
        for (spec, sock) in pool_specs(paths, &inst) {
            if sup.state(&spec.name).await.is_none() {
                continue;
            }
            tracing::info!(pool = %spec.name, "PHP config changed; restarting pool");
            sup.stop(&spec.name).await.ok();
            start_pool(sup, spec, &sock).await?;
        }
    }
    Ok(())
}

/// Wait until a unix socket accepts connections.
pub async fn wait_for_socket(path: &std::path::Path, timeout: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if tokio::net::UnixStream::connect(path).await.is_ok() {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

/// Start FPM pools for `want` and stop pools no longer needed.
async fn sync_fpm(paths: &Paths, sup: &Supervisor, want: &[String]) -> anyhow::Result<()> {
    let mut desired: Vec<(Spec, std::path::PathBuf)> = Vec::new();
    for v in want {
        let pv = PhpVersion::parse(v).context("bad version")?;
        if let Some(inst) = php::find(paths, pv)? {
            desired.extend(pool_specs(paths, &inst));
        }
    }
    let running: Vec<String> = sup
        .states()
        .await
        .into_iter()
        .map(|(n, _)| n)
        .filter(|n| n.starts_with("php-fpm-"))
        .collect();
    for name in running
        .iter()
        .filter(|n| !desired.iter().any(|(s, _)| &s.name == *n))
    {
        sup.stop(name).await.ok();
    }
    for (spec, sock) in desired {
        if !running.contains(&spec.name) {
            start_pool(sup, spec, &sock).await?;
        }
    }
    Ok(())
}

/// Run everything until `shutdown` resolves.
pub async fn run(
    paths: Paths,
    shutdown: impl std::future::Future<Output = ()>,
) -> anyhow::Result<()> {
    let rendered = render(&paths)?;
    let cfg = Config::load(&paths)?;
    if !install::caddy_bin(&paths).exists() {
        anyhow::bail!("web server not installed; run `corral setup` first");
    }

    let _lock = orphans::EngineLock::acquire(&paths)?;
    let reaped = orphans::reap(&paths);
    if !reaped.is_empty() {
        tracing::warn!(
            count = reaped.len(),
            "cleaned up processes left by a previous run"
        );
    }

    let dns_addr = SocketAddr::from((cfg.loopback, cfg.dns_port));
    let sock = corral_dns::bind(dns_addr)
        .await
        .with_context(|| format!("bind DNS on {dns_addr}"))?;
    let zone = corral_dns::Zone::new(&cfg.tld, cfg.loopback)?;
    let dns = tokio::spawn(corral_dns::serve(sock, zone));
    tracing::info!(%dns_addr, tld = %cfg.tld, "dns ready");

    let mail = if cfg.mail_port == 0 {
        None
    } else {
        match tokio::net::TcpListener::bind((cfg.loopback, cfg.mail_port)).await {
            Ok(l) => {
                tracing::info!(port = cfg.mail_port, "mail catcher ready");
                Some(tokio::spawn(corral_mail::serve(
                    l,
                    corral_mail::Store::new(corral_mail::store_dir(&paths.root)),
                )))
            }
            Err(e) => {
                tracing::warn!(port = cfg.mail_port, error = %e, "mail catcher disabled: port unavailable");
                None
            }
        }
    };

    let dumps = if cfg.dumps {
        match corral_dumps::bind(&corral_dumps::socket_path(&paths.root)) {
            Ok(l) => Some(tokio::spawn(corral_dumps::serve(
                l,
                corral_dumps::Store::new(corral_dumps::store_file(&paths.root)),
                corral_dumps::Renderer {
                    php: paths.bin.join("php"),
                    script: corral_dumps::render_path(&paths.root),
                },
            ))),
            Err(e) => {
                tracing::warn!(error = %e, "dump viewer disabled: cannot bind socket");
                None
            }
        }
    } else {
        None
    };

    let events = if cfg.dumps {
        match corral_dumps::bind(&corral_dumps::events::socket_path(&paths.root)) {
            Ok(l) => Some(tokio::spawn(corral_dumps::events::serve(
                l,
                corral_dumps::events::store(&paths.root),
            ))),
            Err(e) => {
                tracing::warn!(error = %e, "watchers disabled: cannot bind events socket");
                None
            }
        }
    } else {
        None
    };

    let sup = Supervisor::new();
    sync_fpm(&paths, &sup, &rendered.fpm_versions).await?;
    sup.start(caddy_spec(&paths)).await?;
    if let Err(e) = corral_services::sync(&paths, &sup).await {
        tracing::error!(error = %e, "service sync failed");
    }

    let watcher = {
        let paths = paths.clone();
        let sup = sup.clone();
        async move {
            let mut last = mtimes(&paths);
            let mut last_user_ini = user_ini_fingerprints(&paths);
            let mut last_pids = Vec::new();
            loop {
                let pids: Vec<u32> = sup
                    .states()
                    .await
                    .into_iter()
                    .filter_map(|(_, s)| match s {
                        corral_supervisor::State::Running { pid } => Some(pid),
                        _ => None,
                    })
                    .collect();
                if pids != last_pids {
                    if let Err(e) = orphans::record(&paths, &pids) {
                        tracing::warn!(error = %e, "could not record child pids");
                    }
                    last_pids = pids;
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
                // User php.ini overrides (conf.d) only need their pool restarted.
                let user_ini = user_ini_fingerprints(&paths);
                if user_ini != last_user_ini {
                    let changed: Vec<String> = user_ini
                        .iter()
                        .filter(|(v, fp)| last_user_ini.get(*v) != Some(fp))
                        .map(|(v, _)| v.clone())
                        .collect();
                    last_user_ini = user_ini;
                    if let Err(e) = restart_fpm(&paths, &sup, &changed).await {
                        tracing::error!(error = %e, "fpm restart failed");
                    }
                }
                let now = mtimes(&paths);
                if now == last {
                    continue;
                }
                last = now;
                match render(&paths) {
                    Ok(r) => {
                        tracing::info!(sites = r.sites.len(), "configuration changed; re-rendered");
                        if let Err(e) = sync_fpm(&paths, &sup, &r.fpm_versions).await {
                            tracing::error!(error = %e, "fpm sync failed");
                        }
                        if let Err(e) = restart_fpm(&paths, &sup, &r.changed_php).await {
                            tracing::error!(error = %e, "fpm restart failed");
                        }
                        if let Err(e) = corral_services::sync(&paths, &sup).await {
                            tracing::error!(error = %e, "service sync failed");
                        }
                    }
                    Err(e) => tracing::error!(error = %e, "render failed"),
                }
            }
        }
    };

    tokio::select! {
        _ = shutdown => {}
        _ = watcher => {}
    }
    dns.abort();
    if let Some(m) = mail {
        m.abort();
    }
    if let Some(d) = dumps {
        d.abort();
    }
    if let Some(e) = events {
        e.abort();
    }
    let _ = std::fs::remove_file(corral_dumps::events::socket_path(&paths.root));
    // No socket means the prepend file falls back to normal dump() output.
    let _ = std::fs::remove_file(corral_dumps::socket_path(&paths.root));
    sup.stop_all().await;
    let _ = std::fs::remove_file(orphans::pid_file(&paths));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use corral_core::sites::Registry;

    fn fake_php(paths: &Paths, v: &str) {
        let dir = paths.php_version_dir(v);
        for b in ["bin/php", "sbin/php-fpm"] {
            let p = dir.join(b);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, "#!/bin/sh\n").unwrap();
        }
    }

    #[tokio::test]
    async fn wait_for_socket_sees_late_listener() {
        let t = tempfile::tempdir().unwrap();
        let sock = t.path().join("s.sock");
        assert!(!wait_for_socket(&sock, Duration::from_millis(100)).await);
        let s2 = sock.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(150)).await;
            let l = tokio::net::UnixListener::bind(&s2).unwrap();
            loop {
                let _ = l.accept().await;
            }
        });
        assert!(wait_for_socket(&sock, Duration::from_secs(3)).await);
    }

    #[test]
    fn render_end_to_end() {
        let t = tempfile::tempdir().unwrap();
        let paths = Paths::at(t.path().join("corral"));
        let sites_dir = t.path().join("Sites");
        for (d, f) in [
            ("shop/public", "index.php"),
            ("legacy", "index.php"),
            ("docs", "index.html"),
        ] {
            std::fs::create_dir_all(sites_dir.join(d)).unwrap();
            std::fs::write(sites_dir.join(d).join(f), "").unwrap();
        }
        fake_php(&paths, "8.4");
        fake_php(&paths, "8.3");
        let cfg = Config {
            parked: vec![sites_dir],
            default_php: Some("8.4".into()),
            ..Default::default()
        };
        paths.ensure().unwrap();
        cfg.save(&paths).unwrap();
        let mut reg = Registry::load(&paths).unwrap();
        reg.isolate(&cfg, "legacy", Some("7.4".into())).unwrap();
        reg.set_secure(&cfg, "docs", Some(false)).unwrap();
        reg.save().unwrap();

        let r = render(&paths).unwrap();
        assert_eq!(r.sites.len(), 3);
        assert_eq!(
            r.fpm_versions,
            ["8.4"],
            "legacy falls back to newest installed"
        );
        assert_eq!(r.missing_php, [("legacy".to_string(), "7.4".to_string())]);

        let caddy: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&r.caddy_config).unwrap()).unwrap();
        let https = caddy["apps"]["http"]["servers"]["corral_https"]["routes"]
            .as_array()
            .unwrap();
        assert_eq!(https.len(), 3, "legacy + shop + 404");
        assert!(paths.certs_dir().join("shop.test.crt").exists());
        assert!(!paths.certs_dir().join("docs.test.crt").exists());
        assert!(paths.bin.join("php").exists());
        assert!(
            paths
                .php_version_dir("8.3")
                .join("etc/php-fpm.conf")
                .exists()
        );

        // Re-render is stable and keeps the CA.
        let r2 = render(&paths).unwrap();
        assert_eq!(r.ca_fingerprint, r2.ca_fingerprint);
        assert_eq!(r.changed_php, ["8.4", "8.3"], "first render writes configs");
        assert!(
            r2.changed_php.is_empty(),
            "unchanged configs must not restart pools"
        );
    }
}
