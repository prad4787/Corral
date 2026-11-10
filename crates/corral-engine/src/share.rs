//! Share a local site on a public URL with a Cloudflare quick tunnel
//! (`cloudflared tunnel --url …`): no account, random trycloudflare.com host.
//!
//! cloudflared is downloaded from github.com/cloudflare/cloudflared and
//! verified against the SHA-256 digest GitHub records for the asset. The
//! tunnel only exists while the returned [`Share`] is alive.

use anyhow::{Context, bail};
use corral_core::paths::Paths;
use corral_core::{Config, sites::Site};
use std::path::PathBuf;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

pub fn bin(paths: &Paths) -> PathBuf {
    crate::install::tools_dir(paths).join("cloudflared")
}

pub async fn install(paths: &Paths) -> anyhow::Result<PathBuf> {
    let asset_name = if cfg!(target_arch = "aarch64") {
        "cloudflared-darwin-arm64.tgz"
    } else {
        "cloudflared-darwin-amd64.tgz"
    };
    let body = corral_fetch::client()?
        .get("https://api.github.com/repos/cloudflare/cloudflared/releases/latest")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let rel: serde_json::Value = serde_json::from_str(&body)?;
    let asset = rel["assets"]
        .as_array()
        .and_then(|a| a.iter().find(|x| x["name"] == asset_name))
        .with_context(|| format!("{asset_name} missing from the latest cloudflared release"))?;
    let digest = asset["digest"]
        .as_str()
        .and_then(|d| d.strip_prefix("sha256:"))
        .context("GitHub did not report a SHA-256 for cloudflared")?;
    let url = asset["browser_download_url"]
        .as_str()
        .context("asset url")?;
    let expected = corral_fetch::Expected {
        algo: corral_fetch::Hash::Sha256,
        hex: digest.into(),
    };
    let archive =
        corral_fetch::fetch_verified(url, &paths.downloads_dir(), Some(&expected)).await?;
    let dest = bin(paths);
    corral_fetch::extract_entry(&archive, "cloudflared", &dest)?;
    let _ = std::fs::remove_file(&archive);
    Ok(dest)
}

/// Find the public URL in a cloudflared log line.
pub fn find_url(line: &str) -> Option<String> {
    let start = line.find("https://")?;
    let rest = &line[start..];
    let end = rest
        .find(|c: char| c.is_whitespace() || c == '|' || c == '"')
        .unwrap_or(rest.len());
    let url = &rest[..end];
    url.trim_end_matches('/')
        .ends_with(".trycloudflare.com")
        .then(|| url.to_string())
}

/// cloudflared arguments for `site` (pure, for tests).
pub fn args(cfg: &Config, site: &Site) -> Vec<String> {
    let mut a = vec!["tunnel".into(), "--no-autoupdate".into()];
    if site.secure {
        a.extend([
            "--url".into(),
            format!("https://127.0.0.1:{}", cfg.internal_https_port),
            // SNI picks the site's certificate; Corral's CA is local-only.
            "--origin-server-name".into(),
            site.host.clone(),
            "--no-tls-verify".into(),
        ]);
    } else {
        a.extend([
            "--url".into(),
            format!("http://127.0.0.1:{}", cfg.internal_http_port),
        ]);
    }
    a.extend(["--http-host-header".into(), site.host.clone()]);
    a
}

/// A running tunnel. Dropping it stops cloudflared.
pub struct Share {
    pub url: String,
    pub site: String,
    child: tokio::process::Child,
}

impl Share {
    pub async fn stop(mut self) {
        let _ = self.child.kill().await;
    }

    /// True while cloudflared is still running.
    pub fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

/// Start a tunnel and wait (up to 30s) for its public URL.
pub async fn start(paths: &Paths, cfg: &Config, site: &Site) -> anyhow::Result<Share> {
    let exe = bin(paths);
    anyhow::ensure!(exe.exists(), "cloudflared is not installed");
    let log_path = paths.logs_dir().join(format!("share-{}.log", site.name));
    let mut child = tokio::process::Command::new(&exe)
        .args(args(cfg, site))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("start cloudflared")?;
    let stderr = child.stderr.take().expect("piped");
    let stdout = child.stdout.take().expect("piped");
    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(1);
    // cloudflared logs to stderr; keep draining both streams into a log file.
    for stream in [
        Box::new(stderr) as Box<dyn tokio::io::AsyncRead + Unpin + Send>,
        Box::new(stdout),
    ] {
        let tx = tx.clone();
        let log_path = log_path.clone();
        tokio::spawn(async move {
            let mut log = tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_path)
                .await
                .ok();
            let mut lines = tokio::io::BufReader::new(stream).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if let Some(l) = log.as_mut() {
                    let _ = l.write_all(format!("{line}\n").as_bytes()).await;
                }
                if let Some(u) = find_url(&line) {
                    let _ = tx.try_send(u);
                }
            }
        });
    }
    drop(tx);
    match tokio::time::timeout(Duration::from_secs(30), rx.recv()).await {
        Ok(Some(url)) => Ok(Share {
            url,
            site: site.name.clone(),
            child,
        }),
        Ok(None) => bail!(
            "cloudflared exited before creating a tunnel; see {}",
            log_path.display()
        ),
        Err(_) => {
            let _ = child.kill().await;
            bail!("no tunnel URL after 30s; see {}", log_path.display())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_tunnel_url() {
        let line = "2026-11-11T10:00:00Z INF |  https://brave-otter-sample.trycloudflare.com                                   |";
        assert_eq!(
            find_url(line).as_deref(),
            Some("https://brave-otter-sample.trycloudflare.com")
        );
        assert_eq!(find_url("see https://developers.cloudflare.com/docs"), None);
        assert_eq!(find_url("no url here"), None);
    }

    #[test]
    fn args_route_by_host_and_sni() {
        let cfg = Config::default();
        let t = tempfile::tempdir().unwrap();
        let paths = Paths::at(t.path().join("c"));
        paths.ensure().unwrap();
        let dir = t.path().join("Sites/shop");
        std::fs::create_dir_all(&dir).unwrap();
        let cfg2 = Config {
            parked: vec![t.path().join("Sites")],
            ..cfg.clone()
        };
        let mut site = corral_core::sites::Registry::load(&paths)
            .unwrap()
            .resolve(&cfg2)
            .unwrap()
            .remove(0);
        site.secure = true;
        let a = args(&cfg, &site).join(" ");
        assert!(a.contains("--url https://127.0.0.1:18443"));
        assert!(a.contains("--origin-server-name shop.test"));
        assert!(a.contains("--http-host-header shop.test"));
        site.secure = false;
        let a = args(&cfg, &site).join(" ");
        assert!(a.contains("--url http://127.0.0.1:18080") && !a.contains("no-tls-verify"));
    }
}
