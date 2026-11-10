//! MongoDB Community Server: data dir, supervisor spec.
//! 127.0.0.1 only, no auth (local development), socket in Corral's run dir.

use crate::store::{self, Instance};
use corral_core::paths::Paths;
use corral_supervisor::Spec;
use std::time::Duration;

pub fn prepare(paths: &Paths, name: &str) -> anyhow::Result<()> {
    std::fs::create_dir_all(store::data_dir(paths, name))?;
    std::fs::create_dir_all(paths.run_dir())?;
    Ok(())
}

pub fn spec(paths: &Paths, name: &str, inst: &Instance) -> Spec {
    let dist = store::dist_dir(paths, inst.kind, &inst.version);
    let mut s = Spec::new(
        store::unit(name),
        dist.join("bin/mongod"),
        store::log_file(paths, name),
    )
    .args([
        "--dbpath".to_string(),
        store::data_dir(paths, name).display().to_string(),
        "--port".into(),
        inst.port.to_string(),
        "--bind_ip".into(),
        "127.0.0.1".into(),
        "--unixSocketPrefix".into(),
        paths.run_dir().display().to_string(),
    ]);
    // mongod shuts down cleanly on SIGTERM; give WiredTiger time to checkpoint.
    s.stop_timeout = Duration::from_secs(60);
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::Kind;

    #[test]
    fn local_only() {
        let t = tempfile::tempdir().unwrap();
        let p = Paths::at(t.path());
        let inst = Instance {
            kind: Kind::Mongodb,
            version: "8.2.12".into(),
            port: 27018,
            enabled: true,
        };
        let s = spec(&p, "mongodb", &inst);
        assert!(
            s.program
                .ends_with("services/dist/mongodb-8.2.12/bin/mongod")
        );
        assert!(
            s.args
                .windows(2)
                .any(|w| w[0] == "--bind_ip" && w[1] == "127.0.0.1")
        );
        assert!(
            s.args
                .windows(2)
                .any(|w| w[0] == "--port" && w[1] == "27018")
        );
    }
}
