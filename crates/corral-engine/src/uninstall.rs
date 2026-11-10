//! Removing Corral's files (shared by `corral uninstall` and the app).
//! System integration (resolver, trust, port daemon) is undone separately
//! by `corral_system::undo`.

use corral_core::paths::Paths;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, serde::Serialize)]
pub struct Plan {
    pub remove: Vec<PathBuf>,
    pub shell_files: Vec<PathBuf>,
    /// Database data that is kept unless `delete_data`.
    pub kept: Option<PathBuf>,
}

/// What would be removed. Service data is kept unless `delete_data`.
pub fn plan(paths: &Paths, home: &Path, delete_data: bool) -> anyhow::Result<Plan> {
    let shell_files = corral_core::shell::rc_files(home)
        .into_iter()
        .filter(|f| corral_core::shell::has_block(f))
        .collect();
    let data = paths.root.join("services/data");
    let mut remove: Vec<PathBuf> = vec![paths.bin.clone()];
    if delete_data {
        remove.push(paths.root.clone());
    } else if let Ok(rd) = std::fs::read_dir(&paths.root) {
        for e in rd.flatten() {
            let p = e.path();
            if p == paths.root.join("services") {
                for sub in ["conf", "dist", "build"] {
                    remove.push(p.join(sub));
                }
            } else {
                remove.push(p);
            }
        }
    }
    remove.retain(|p| p.exists() && *p != data);
    remove.sort();
    remove.dedup();
    let all = remove.clone();
    remove.retain(|p| !all.iter().any(|q| q != p && p.starts_with(q)));
    for p in &remove {
        anyhow::ensure!(paths.owns(p), "refusing to remove {}", p.display());
    }
    Ok(Plan {
        remove,
        shell_files,
        kept: (!delete_data && data.exists()).then_some(data),
    })
}

pub fn apply(paths: &Paths, plan: &Plan) -> anyhow::Result<()> {
    anyhow::ensure!(
        !crate::orphans::engine_running(paths),
        "Corral is running; stop it first"
    );
    for rc in &plan.shell_files {
        corral_core::shell::uninstall(rc)?;
    }
    for p in &plan.remove {
        if p.is_dir() {
            std::fs::remove_dir_all(p)?;
        } else if p.exists() {
            std::fs::remove_file(p)?;
        }
    }
    if let Some(d) = paths.bin.parent().filter(|d| d.ends_with(".corral")) {
        let _ = std::fs::remove_dir(d); // only if empty
    }
    Ok(())
}
