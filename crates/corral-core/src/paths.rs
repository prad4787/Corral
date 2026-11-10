//! Filesystem layout. Every path Corral touches is derived from here.
//!
//! Set `CORRAL_HOME` to relocate the whole tree (used by tests and dev builds).

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    /// Application data root (`~/Library/Application Support/Corral`).
    pub root: PathBuf,
    /// Directory added to the user's PATH (`~/.corral/bin`).
    pub bin: PathBuf,
}

impl Paths {
    /// Resolve from `CORRAL_HOME` or the platform default.
    pub fn discover() -> anyhow::Result<Self> {
        if let Some(home) = std::env::var_os("CORRAL_HOME") {
            return Ok(Self::at(PathBuf::from(home)));
        }
        let data = dirs::data_dir().ok_or_else(|| anyhow::anyhow!("no data directory"))?;
        let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("no home directory"))?;
        Ok(Self {
            root: data.join("Corral"),
            bin: home.join(".corral").join("bin"),
        })
    }

    /// Put everything (including `bin`) under one directory.
    pub fn at(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            bin: root.join("bin"),
            root,
        }
    }

    pub fn config_file(&self) -> PathBuf {
        self.root.join("config.toml")
    }
    pub fn sites_file(&self) -> PathBuf {
        self.root.join("sites.toml")
    }
    pub fn php_dir(&self) -> PathBuf {
        self.root.join("php")
    }
    pub fn php_version_dir(&self, version: &str) -> PathBuf {
        self.php_dir().join(version)
    }
    pub fn run_dir(&self) -> PathBuf {
        self.root.join("run")
    }
    pub fn fpm_socket(&self, version: &str) -> PathBuf {
        self.run_dir().join(format!("php-fpm-{version}.sock"))
    }
    /// Socket of the Xdebug-enabled pool for `version`.
    pub fn fpm_debug_socket(&self, version: &str) -> PathBuf {
        self.run_dir().join(format!("php-fpm-{version}-debug.sock"))
    }
    pub fn generated_dir(&self) -> PathBuf {
        self.root.join("generated")
    }
    pub fn certs_dir(&self) -> PathBuf {
        self.root.join("certs")
    }
    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }
    pub fn downloads_dir(&self) -> PathBuf {
        self.root.join("downloads")
    }

    /// Create every directory Corral owns. Idempotent.
    pub fn ensure(&self) -> std::io::Result<()> {
        for d in [
            &self.root,
            &self.bin,
            &self.php_dir(),
            &self.run_dir(),
            &self.generated_dir(),
            &self.certs_dir(),
            &self.logs_dir(),
            &self.downloads_dir(),
        ] {
            std::fs::create_dir_all(d)?;
        }
        Ok(())
    }

    /// macOS limits unix socket paths to 104 bytes (including the NUL).
    /// Fail early with a clear message instead of cryptic bind errors.
    pub fn check_socket_paths(&self) -> Result<(), String> {
        const MAX: usize = 103;
        // The longest socket Corral creates (debug pool of a two-digit minor).
        let longest = self.fpm_debug_socket("10.10");
        let len = longest.as_os_str().len();
        if len > MAX {
            return Err(format!(
                "Corral's data directory path is too long for unix sockets ({len} > {MAX} bytes): {}. \
                 Set CORRAL_HOME to a shorter path.",
                self.root.display()
            ));
        }
        Ok(())
    }

    /// True if `p` lives inside Corral's data root. Used to refuse destructive
    /// operations on anything we do not own.
    pub fn owns(&self, p: &Path) -> bool {
        p.starts_with(&self.root) || p.starts_with(&self.bin)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn at_nests_everything() {
        let p = Paths::at("/tmp/c");
        assert_eq!(p.bin, PathBuf::from("/tmp/c/bin"));
        assert_eq!(
            p.fpm_socket("8.4"),
            PathBuf::from("/tmp/c/run/php-fpm-8.4.sock")
        );
        assert!(p.owns(Path::new("/tmp/c/php/8.4")));
        assert!(!p.owns(Path::new("/etc/resolver/test")));
    }

    #[test]
    fn socket_path_limit() {
        assert!(
            Paths::at("/Users/me/Library/Application Support/Corral")
                .check_socket_paths()
                .is_ok()
        );
        let long = format!("/tmp/{}", "x".repeat(90));
        let err = Paths::at(long).check_socket_paths().unwrap_err();
        assert!(err.contains("too long"), "{err}");
    }

    #[test]
    fn ensure_is_idempotent() {
        let t = tempfile::tempdir().unwrap();
        let p = Paths::at(t.path());
        p.ensure().unwrap();
        p.ensure().unwrap();
        assert!(p.run_dir().is_dir());
    }
}
