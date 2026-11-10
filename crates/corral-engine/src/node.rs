//! Node.js versions from nodejs.org, verified against each release's
//! `SHASUMS256.txt`. Installed per major version under `node/<major>/`, with
//! `node`/`npm`/`npx`/`corepack` shims for the default.

use anyhow::{Context, bail};
use corral_core::fsutil::write_if_changed;
use corral_core::paths::Paths;
use serde::Deserialize;
use std::path::PathBuf;

const DIST: &str = "https://nodejs.org/dist";
pub const TOOLS: &[&str] = &["node", "npm", "npx", "corepack"];

#[derive(Debug, Clone, Deserialize)]
pub struct Release {
    pub version: String,
    #[serde(default)]
    pub files: Vec<String>,
    /// `false` or the LTS codename.
    #[serde(default)]
    pub lts: serde_json::Value,
}

impl Release {
    pub fn major(&self) -> Option<u32> {
        self.version
            .trim_start_matches('v')
            .split('.')
            .next()?
            .parse()
            .ok()
    }
    pub fn is_prerelease(&self) -> bool {
        self.version.contains('-')
    }
    pub fn lts_name(&self) -> Option<&str> {
        self.lts.as_str()
    }
}

fn platform() -> (&'static str, &'static str) {
    if cfg!(target_arch = "aarch64") {
        ("darwin-arm64", "osx-arm64-tar")
    } else {
        ("darwin-x64", "osx-x64-tar")
    }
}

pub fn node_dir(paths: &Paths) -> PathBuf {
    paths.root.join("node")
}
pub fn major_dir(paths: &Paths, major: u32) -> PathBuf {
    node_dir(paths).join(major.to_string())
}

/// Newest stable release for `major` (or the newest LTS when `None`).
pub fn pick(index: &[Release], major: Option<u32>) -> Option<&Release> {
    let (_, file_key) = platform();
    index.iter().find(|r| {
        !r.is_prerelease()
            && r.files.iter().any(|f| f == file_key)
            && match major {
                Some(m) => r.major() == Some(m),
                None => r.lts_name().is_some(),
            }
    })
}

pub async fn fetch_index() -> anyhow::Result<Vec<Release>> {
    let body = corral_fetch::client()?
        .get(format!("{DIST}/index.json"))
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    Ok(serde_json::from_str(&body)?)
}

/// Install the newest release of `major` (newest LTS if `None`). Returns (major, version).
pub async fn install(paths: &Paths, major: Option<u32>) -> anyhow::Result<(u32, String)> {
    let index = fetch_index().await?;
    let rel = pick(&index, major).with_context(|| match major {
        Some(m) => format!("no stable Node {m} release for this Mac"),
        None => "no LTS release found".into(),
    })?;
    let m = rel.major().context("bad version")?;
    let (plat, _) = platform();
    let file = format!("node-{}-{plat}.tar.gz", rel.version);
    let base = format!("{DIST}/{}", rel.version);
    let sums = corral_fetch::client()?
        .get(format!("{base}/SHASUMS256.txt"))
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let expected = corral_fetch::Expected {
        algo: corral_fetch::Hash::Sha256,
        hex: corral_fetch::find_checksum(&sums, &file)
            .with_context(|| format!("{file} missing from SHASUMS256.txt"))?,
    };
    let archive = corral_fetch::fetch_verified(
        &format!("{base}/{file}"),
        &paths.downloads_dir(),
        Some(&expected),
    )
    .await?;
    let dest = major_dir(paths, m);
    let result = tokio::task::spawn_blocking({
        let (a, d) = (archive.clone(), dest.clone());
        move || corral_fetch::extract_tree(&a, &d, 1)
    })
    .await?;
    let _ = std::fs::remove_file(&archive);
    result?;
    std::fs::write(dest.join(".corral-version"), &rel.version)?;
    Ok((m, rel.version.clone()))
}

/// Installed majors with their exact version, newest first.
pub fn installed(paths: &Paths) -> Vec<(u32, String)> {
    let mut v: Vec<(u32, String)> = std::fs::read_dir(node_dir(paths))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let m: u32 = e.file_name().to_str()?.parse().ok()?;
            e.path().join("bin/node").is_file().then_some(())?;
            let ver = std::fs::read_to_string(e.path().join(".corral-version")).unwrap_or_default();
            Some((m, ver.trim().to_string()))
        })
        .collect();
    v.sort_by(|a, b| b.0.cmp(&a.0));
    v
}

/// Point the node/npm/npx/corepack shims at `major` (or remove them).
pub fn write_shims(paths: &Paths, major: Option<u32>) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(&paths.bin)?;
    let target = match major {
        Some(m) => {
            let dir = major_dir(paths, m);
            if !dir.join("bin/node").is_file() {
                bail!("Node {m} is not installed");
            }
            Some(dir.join("bin"))
        }
        None => None,
    };
    for tool in TOOLS {
        let shim = paths.bin.join(tool);
        match &target {
            Some(bin) => {
                // npm/npx/corepack are `#!/usr/bin/env node` scripts, so the
                // matching node must be first on PATH.
                let body = format!(
                    "#!/bin/sh\n# Generated by Corral.\nPATH=\"{bin}:$PATH\" exec \"{bin}/{tool}\" \"$@\"\n",
                    bin = bin.display()
                );
                write_if_changed(&shim, body.as_bytes())?;
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

pub fn remove(paths: &Paths, major: u32) -> anyhow::Result<()> {
    let dir = major_dir(paths, major);
    anyhow::ensure!(
        paths.owns(&dir) && dir.exists(),
        "Node {major} is not installed"
    );
    std::fs::remove_dir_all(dir)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(v: &str, lts: bool, files: &[&str]) -> Release {
        Release {
            version: v.into(),
            files: files.iter().map(|s| s.to_string()).collect(),
            lts: if lts {
                serde_json::json!("Jod")
            } else {
                serde_json::json!(false)
            },
        }
    }

    #[test]
    fn picks_stable_matching_major_and_lts() {
        let (_, key) = platform();
        let idx = vec![
            rel("v27.0.0-alpha.0", false, &[key]),
            rel("v26.13.0", true, &[key]),
            rel("v25.9.0", false, &[key]),
            rel("v22.23.4", true, &[key]),
            rel("v22.23.3", true, &[key]),
            rel("v20.0.0", true, &["linux-x64"]),
        ];
        assert_eq!(pick(&idx, None).unwrap().version, "v26.13.0");
        assert_eq!(pick(&idx, Some(22)).unwrap().version, "v22.23.4");
        assert_eq!(pick(&idx, Some(25)).unwrap().version, "v25.9.0");
        assert!(pick(&idx, Some(27)).is_none(), "prereleases are skipped");
        assert!(pick(&idx, Some(20)).is_none(), "no build for this platform");
    }

    #[test]
    fn shims_point_at_default_and_clean_up() {
        let t = tempfile::tempdir().unwrap();
        let p = Paths::at(t.path());
        let bin = major_dir(&p, 22).join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("node"), "").unwrap();
        std::fs::write(major_dir(&p, 22).join(".corral-version"), "v22.23.4").unwrap();
        assert_eq!(installed(&p), [(22, "v22.23.4".to_string())]);
        write_shims(&p, Some(22)).unwrap();
        let npm = std::fs::read_to_string(p.bin.join("npm")).unwrap();
        assert!(npm.contains(&format!("PATH=\"{}:$PATH\"", bin.display())));
        assert!(write_shims(&p, Some(20)).is_err());
        write_shims(&p, None).unwrap();
        assert!(!p.bin.join("node").exists());
    }
}
