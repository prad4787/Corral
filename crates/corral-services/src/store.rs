//! Service instances (`services.toml`). One kind can have several instances,
//! e.g. two MySQL servers on different ports.

use crate::manifest::Kind;
use corral_core::fsutil::write_atomic;
use corral_core::paths::Paths;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Instance {
    pub kind: Kind,
    /// Full upstream version, e.g. `8.4.11`.
    pub version: String,
    pub port: u16,
    /// Started by the engine when true.
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ServicesFile {
    pub instances: BTreeMap<String, Instance>,
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("a service named {0:?} already exists")]
    Exists(String),
    #[error("no service named {0:?}")]
    NotFound(String),
    #[error("port {port} is already used by service {by:?}")]
    PortTaken { port: u16, by: String },
    #[error("invalid service name {0:?}: use lowercase letters, digits and '-'")]
    InvalidName(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Parse(#[from] toml::de::Error),
    #[error(transparent)]
    Serialize(#[from] toml::ser::Error),
}

pub fn services_file(paths: &Paths) -> PathBuf {
    paths.root.join("services.toml")
}
pub fn dist_dir(paths: &Paths, kind: Kind, version: &str) -> PathBuf {
    paths
        .root
        .join("services/dist")
        .join(format!("{}-{version}", kind.as_str()))
}
pub fn data_dir(paths: &Paths, name: &str) -> PathBuf {
    paths.root.join("services/data").join(name)
}
pub fn conf_file(paths: &Paths, name: &str) -> PathBuf {
    paths.root.join("services/conf").join(format!("{name}.cnf"))
}
pub fn log_file(paths: &Paths, name: &str) -> PathBuf {
    paths.logs_dir().join(format!("service-{name}.log"))
}
pub fn socket(paths: &Paths, name: &str) -> PathBuf {
    paths.run_dir().join(format!("{name}.sock"))
}
/// Supervisor key for an instance.
pub fn unit(name: &str) -> String {
    format!("service-{name}")
}

impl ServicesFile {
    pub fn load(paths: &Paths) -> Result<Self, StoreError> {
        match std::fs::read_to_string(services_file(paths)) {
            Ok(s) => Ok(toml::from_str(&s)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save(&self, paths: &Paths) -> Result<(), StoreError> {
        write_atomic(
            &services_file(paths),
            toml::to_string_pretty(self)?.as_bytes(),
        )?;
        Ok(())
    }

    pub fn add(&mut self, name: &str, inst: Instance) -> Result<(), StoreError> {
        if corral_core::sites::slugify(name).as_deref() != Some(name) {
            return Err(StoreError::InvalidName(name.into()));
        }
        if self.instances.contains_key(name) {
            return Err(StoreError::Exists(name.into()));
        }
        if let Some((by, _)) = self.instances.iter().find(|(_, i)| i.port == inst.port) {
            return Err(StoreError::PortTaken {
                port: inst.port,
                by: by.clone(),
            });
        }
        self.instances.insert(name.into(), inst);
        Ok(())
    }

    pub fn get(&self, name: &str) -> Result<&Instance, StoreError> {
        self.instances
            .get(name)
            .ok_or_else(|| StoreError::NotFound(name.into()))
    }

    pub fn set_enabled(&mut self, name: &str, enabled: bool) -> Result<(), StoreError> {
        self.instances
            .get_mut(name)
            .ok_or_else(|| StoreError::NotFound(name.into()))?
            .enabled = enabled;
        Ok(())
    }

    pub fn remove(&mut self, name: &str) -> Result<Instance, StoreError> {
        self.instances
            .remove(name)
            .ok_or_else(|| StoreError::NotFound(name.into()))
    }

    /// First free port at or after `start` that no instance uses.
    pub fn free_port(&self, start: u16) -> u16 {
        (start..=u16::MAX)
            .find(|p| !self.instances.values().any(|i| i.port == *p))
            .unwrap_or(start)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mysql(port: u16) -> Instance {
        Instance {
            kind: Kind::Mysql,
            version: "8.4.11".into(),
            port,
            enabled: true,
        }
    }

    #[test]
    fn add_validate_persist() {
        let t = tempfile::tempdir().unwrap();
        let p = Paths::at(t.path());
        let mut f = ServicesFile::load(&p).unwrap();
        f.add("mysql", mysql(3306)).unwrap();
        assert!(matches!(
            f.add("mysql", mysql(3307)),
            Err(StoreError::Exists(_))
        ));
        assert!(matches!(
            f.add("db2", mysql(3306)),
            Err(StoreError::PortTaken { .. })
        ));
        assert!(matches!(
            f.add("Bad Name", mysql(1)),
            Err(StoreError::InvalidName(_))
        ));
        assert_eq!(f.free_port(3306), 3307);
        f.add("mysql-old", mysql(3307)).unwrap();
        f.set_enabled("mysql-old", false).unwrap();
        f.save(&p).unwrap();
        let g = ServicesFile::load(&p).unwrap();
        assert_eq!(g, f);
        assert!(!g.get("mysql-old").unwrap().enabled);
        let mut g = g;
        g.remove("mysql").unwrap();
        assert!(matches!(g.get("mysql"), Err(StoreError::NotFound(_))));
    }
}
