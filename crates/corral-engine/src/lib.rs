//! The engine turns Corral's state (config + site registry + installed PHP)
//! into running processes.
//!
//! `render` is pure file generation and is safe to call at any time; `run`
//! starts DNS, one FPM pool per PHP version in use, and Caddy, then re-renders
//! whenever `config.toml` or `sites.toml` change.

pub mod install;
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
    paths.ensure()?;
    let cfg = Config::load(paths)?;
    let sites = Registry::load(paths)?.resolve(&cfg)?;
    let installs = php::installed(paths)?;
    let versions: Vec<PhpVersion> = installs.iter().map(|i| i.version).collect();
    let default = cfg.default_php.as_deref().and_then(PhpVersion::parse);

    let mut changed_php = Vec::new();
    for inst in &installs {
        if php::write_configs(paths, inst)? {
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

    let proxy_sites = corral_proxy::proxy_sites(paths, &sites, &versions, |h| {
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

fn fpm_spec(paths: &Paths, inst: &Installation) -> Spec {
    let mut spec = Spec::new(
        format!("php-fpm-{}", inst.version),
        inst.fpm(),
        paths
            .logs_dir()
            .join(format!("php-fpm-{}.out.log", inst.version)),
    )
    .args(php::fpm_args(inst));
    spec.env = php::php_env(inst);
    spec
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
    [paths.config_file(), paths.sites_file()]
        .into_iter()
        .chain(parked)
        .map(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
        .collect()
}

/// Restart running pools for `versions` (their config changed).
async fn restart_fpm(paths: &Paths, sup: &Supervisor, versions: &[String]) -> anyhow::Result<()> {
    for v in versions {
        let name = format!("php-fpm-{v}");
        if sup.state(&name).await.is_none() {
            continue;
        }
        let Some(inst) = PhpVersion::parse(v).and_then(|pv| php::find(paths, pv).ok().flatten())
        else {
            continue;
        };
        tracing::info!(version = %v, "PHP config changed; restarting pool");
        sup.stop(&name).await.ok();
        let _ = std::fs::remove_file(paths.fpm_socket(v));
        sup.start(fpm_spec(paths, &inst)).await?;
        wait_for_socket(&paths.fpm_socket(v), Duration::from_secs(10)).await;
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
    let running: Vec<String> = sup
        .states()
        .await
        .into_iter()
        .filter_map(|(n, _)| n.strip_prefix("php-fpm-").map(str::to_owned))
        .collect();
    for v in running.iter().filter(|v| !want.contains(v)) {
        sup.stop(&format!("php-fpm-{v}")).await.ok();
    }
    for v in want.iter().filter(|v| !running.contains(v)) {
        let pv = PhpVersion::parse(v).context("bad version")?;
        if let Some(inst) = php::find(paths, pv)? {
            // A stale socket from a crash would make FPM refuse to start.
            let _ = std::fs::remove_file(paths.fpm_socket(v));
            sup.start(fpm_spec(paths, &inst)).await?;
            let sock = paths.fpm_socket(v);
            if !wait_for_socket(&sock, Duration::from_secs(10)).await {
                tracing::error!(socket = %sock.display(), "PHP-FPM {v} did not become ready; see its log");
            }
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

    let sup = Supervisor::new();
    sync_fpm(&paths, &sup, &rendered.fpm_versions).await?;
    sup.start(caddy_spec(&paths)).await?;

    let watcher = {
        let paths = paths.clone();
        let sup = sup.clone();
        async move {
            let mut last = mtimes(&paths);
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
