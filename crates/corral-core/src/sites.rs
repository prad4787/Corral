//! Site registry: parked directories plus explicitly linked sites.
//!
//! Linked sites and per-site overrides live in `sites.toml`. Parked sites are
//! discovered by listing parked directories at resolve time, so creating a
//! folder is enough to get a new site.

use crate::config::Config;
use crate::drivers::{self, Driver};
use crate::fsutil::write_atomic;
use crate::paths::Paths;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Persisted per-site settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SiteEntry {
    /// Set for linked sites; `None` for overrides on parked sites.
    pub path: Option<PathBuf>,
    /// Isolated PHP version (`major.minor`).
    pub php: Option<String>,
    /// Override the global HTTPS default.
    pub secure: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SitesFile {
    /// Keyed by site name (the part before `.tld`).
    pub sites: BTreeMap<String, SiteEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    Parked,
    Linked,
}

/// A fully resolved site, ready for config generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Site {
    pub name: String,
    pub host: String,
    pub path: PathBuf,
    pub origin: Origin,
    pub driver: Driver,
    /// Effective PHP version (isolated or global default).
    pub php: Option<String>,
    pub isolated: bool,
    pub secure: bool,
}

impl Site {
    pub fn url(&self, cfg: &Config) -> String {
        let (scheme, port, default) = if self.secure {
            ("https", cfg.https_port, 443)
        } else {
            ("http", cfg.http_port, 80)
        };
        if port == default {
            format!("{scheme}://{}", self.host)
        } else {
            format!("{scheme}://{}:{port}", self.host)
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SiteError {
    #[error("invalid site name {0:?}: use lowercase letters, digits and '-'")]
    InvalidName(String),
    #[error("{0} is not a directory")]
    NotADirectory(PathBuf),
    #[error("no site named {0:?}")]
    NotFound(String),
    #[error("site {0:?} is parked; remove the folder or unpark its parent instead")]
    IsParked(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Parse(#[from] toml::de::Error),
    #[error(transparent)]
    Serialize(#[from] toml::ser::Error),
}

/// Turn an arbitrary folder name into a valid DNS label, if possible.
pub fn slugify(name: &str) -> Option<String> {
    let mut out = String::with_capacity(name.len());
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_string();
    (!out.is_empty() && out.len() <= 63).then_some(out)
}

fn validate_name(name: &str) -> Result<(), SiteError> {
    if slugify(name).as_deref() == Some(name) {
        Ok(())
    } else {
        Err(SiteError::InvalidName(name.into()))
    }
}

pub struct Registry<'a> {
    paths: &'a Paths,
    file: SitesFile,
}

impl<'a> Registry<'a> {
    pub fn load(paths: &'a Paths) -> Result<Self, SiteError> {
        let file = match std::fs::read_to_string(paths.sites_file()) {
            Ok(s) => toml::from_str(&s)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => SitesFile::default(),
            Err(e) => return Err(e.into()),
        };
        Ok(Self { paths, file })
    }

    pub fn save(&self) -> Result<(), SiteError> {
        write_atomic(
            &self.paths.sites_file(),
            toml::to_string_pretty(&self.file)?.as_bytes(),
        )?;
        Ok(())
    }

    pub fn entries(&self) -> &SitesFile {
        &self.file
    }

    /// Link `path` as `name` (defaults to the slugified folder name). Returns the name.
    pub fn link(&mut self, path: &Path, name: Option<&str>) -> Result<String, SiteError> {
        let path = path.canonicalize()?;
        if !path.is_dir() {
            return Err(SiteError::NotADirectory(path));
        }
        let name = match name {
            Some(n) => n.to_owned(),
            None => path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(slugify)
                .ok_or_else(|| SiteError::InvalidName(path.display().to_string()))?,
        };
        validate_name(&name)?;
        self.file.sites.entry(name.clone()).or_default().path = Some(path);
        Ok(name)
    }

    /// Remove a linked site (and its overrides).
    pub fn unlink(&mut self, name: &str) -> Result<(), SiteError> {
        match self.file.sites.get(name) {
            Some(e) if e.path.is_some() => {
                self.file.sites.remove(name);
                Ok(())
            }
            Some(_) => Err(SiteError::IsParked(name.into())),
            None => Err(SiteError::NotFound(name.into())),
        }
    }

    fn entry_mut(&mut self, cfg: &Config, name: &str) -> Result<&mut SiteEntry, SiteError> {
        if !self.resolve(cfg)?.iter().any(|s| s.name == name) {
            return Err(SiteError::NotFound(name.into()));
        }
        Ok(self.file.sites.entry(name.into()).or_default())
    }

    pub fn isolate(
        &mut self,
        cfg: &Config,
        name: &str,
        php: Option<String>,
    ) -> Result<(), SiteError> {
        self.entry_mut(cfg, name)?.php = php;
        self.prune();
        Ok(())
    }

    pub fn set_secure(
        &mut self,
        cfg: &Config,
        name: &str,
        secure: Option<bool>,
    ) -> Result<(), SiteError> {
        self.entry_mut(cfg, name)?.secure = secure;
        self.prune();
        Ok(())
    }

    /// Drop override entries that no longer carry any information.
    fn prune(&mut self) {
        self.file.sites.retain(|_, e| *e != SiteEntry::default());
    }

    /// Resolve all sites. Linked sites win over parked folders with the same name.
    pub fn resolve(&self, cfg: &Config) -> Result<Vec<Site>, SiteError> {
        let mut found: BTreeMap<String, (PathBuf, Origin)> = BTreeMap::new();
        for dir in &cfg.parked {
            let Ok(rd) = std::fs::read_dir(dir) else {
                tracing::warn!(dir = %dir.display(), "parked directory not readable");
                continue;
            };
            for ent in rd.flatten() {
                let p = ent.path();
                let hidden = ent.file_name().to_string_lossy().starts_with('.');
                if hidden || !p.is_dir() {
                    continue;
                }
                if let Some(name) = ent.file_name().to_str().and_then(slugify) {
                    found.entry(name).or_insert((p, Origin::Parked));
                }
            }
        }
        for (name, e) in &self.file.sites {
            if let Some(p) = &e.path {
                found.insert(name.clone(), (p.clone(), Origin::Linked));
            }
        }
        Ok(found
            .into_iter()
            .map(|(name, (path, origin))| {
                let entry = self.file.sites.get(&name);
                let isolated = entry.and_then(|e| e.php.clone());
                Site {
                    host: format!("{name}.{}", cfg.tld),
                    driver: drivers::detect(&path),
                    php: isolated.clone().or_else(|| cfg.default_php.clone()),
                    isolated: isolated.is_some(),
                    secure: entry
                        .and_then(|e| e.secure)
                        .unwrap_or(cfg.secure_by_default),
                    name,
                    path,
                    origin,
                }
            })
            .collect())
    }

    /// Find the site serving `dir` (or one of its parents), e.g. for `corral isolate` in a project.
    pub fn find_by_path(&self, cfg: &Config, dir: &Path) -> Result<Option<Site>, SiteError> {
        let dir = dir.canonicalize()?;
        Ok(self
            .resolve(cfg)?
            .into_iter()
            .filter(|s| dir.starts_with(s.path.canonicalize().unwrap_or_else(|_| s.path.clone())))
            .max_by_key(|s| s.path.components().count()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, Paths, Config) {
        let t = tempfile::tempdir().unwrap();
        let paths = Paths::at(t.path().join("corral"));
        paths.ensure().unwrap();
        let parked = t.path().join("Sites");
        for d in ["blog", "My Shop", ".hidden"] {
            std::fs::create_dir_all(parked.join(d)).unwrap();
        }
        std::fs::write(parked.join("notes.txt"), "").unwrap();
        let cfg = Config {
            parked: vec![parked],
            default_php: Some("8.4".into()),
            ..Default::default()
        };
        (t, paths, cfg)
    }

    #[test]
    fn slugs() {
        assert_eq!(slugify("My Shop").as_deref(), Some("my-shop"));
        assert_eq!(slugify("api_v2").as_deref(), Some("api-v2"));
        assert_eq!(slugify("---"), None);
    }

    #[test]
    fn parked_sites_are_discovered() {
        let (_t, paths, cfg) = setup();
        let reg = Registry::load(&paths).unwrap();
        let names: Vec<_> = reg
            .resolve(&cfg)
            .unwrap()
            .into_iter()
            .map(|s| s.host)
            .collect();
        assert_eq!(names, ["blog.test", "my-shop.test"]);
    }

    #[test]
    fn link_isolate_secure_persist() {
        let (t, paths, cfg) = setup();
        let proj = t.path().join("elsewhere/api");
        std::fs::create_dir_all(&proj).unwrap();

        let mut reg = Registry::load(&paths).unwrap();
        assert_eq!(reg.link(&proj, None).unwrap(), "api");
        reg.isolate(&cfg, "api", Some("8.2".into())).unwrap();
        reg.isolate(&cfg, "blog", Some("8.3".into())).unwrap();
        reg.set_secure(&cfg, "blog", Some(false)).unwrap();
        reg.save().unwrap();

        let reg = Registry::load(&paths).unwrap();
        let sites = reg.resolve(&cfg).unwrap();
        let api = sites.iter().find(|s| s.name == "api").unwrap();
        assert_eq!(
            (api.origin, api.php.as_deref(), api.isolated),
            (Origin::Linked, Some("8.2"), true)
        );
        let blog = sites.iter().find(|s| s.name == "blog").unwrap();
        assert_eq!((blog.php.as_deref(), blog.secure), (Some("8.3"), false));
        assert_eq!(blog.url(&cfg), "http://blog.test");
        let shop = sites.iter().find(|s| s.name == "my-shop").unwrap();
        assert_eq!(
            (shop.php.as_deref(), shop.isolated, shop.secure),
            (Some("8.4"), false, true)
        );
    }

    #[test]
    fn unisolate_prunes_entry_and_unlink_rules() {
        let (_t, paths, cfg) = setup();
        let mut reg = Registry::load(&paths).unwrap();
        reg.isolate(&cfg, "blog", Some("8.3".into())).unwrap();
        reg.isolate(&cfg, "blog", None).unwrap();
        assert!(reg.entries().sites.is_empty());
        assert!(matches!(
            reg.isolate(&cfg, "nope", None),
            Err(SiteError::NotFound(_))
        ));
        reg.set_secure(&cfg, "blog", Some(true)).unwrap();
        assert!(matches!(reg.unlink("blog"), Err(SiteError::IsParked(_))));
    }

    #[test]
    fn link_overrides_parked_and_find_by_path() {
        let (t, paths, cfg) = setup();
        let other = t.path().join("blog-v2");
        std::fs::create_dir_all(other.join("src")).unwrap();
        let mut reg = Registry::load(&paths).unwrap();
        reg.link(&other, Some("blog")).unwrap();
        let blog = reg
            .resolve(&cfg)
            .unwrap()
            .into_iter()
            .find(|s| s.name == "blog")
            .unwrap();
        assert_eq!(blog.origin, Origin::Linked);
        let found = reg.find_by_path(&cfg, &other.join("src")).unwrap().unwrap();
        assert_eq!(found.name, "blog");
        assert!(matches!(
            reg.link(&other, Some("Bad Name")),
            Err(SiteError::InvalidName(_))
        ));
    }

    #[test]
    fn url_includes_nonstandard_port() {
        let (_t, paths, mut cfg) = setup();
        cfg.https_port = 18443;
        let reg = Registry::load(&paths).unwrap();
        let s = &reg.resolve(&cfg).unwrap()[0];
        assert_eq!(s.url(&cfg), "https://blog.test:18443");
    }
}
