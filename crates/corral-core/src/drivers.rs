//! Project-type detection. A driver decides the document root and how
//! requests that do not match a static file are handled.
//!
//! Detection is ordered from most to least specific; the first match wins.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DriverKind {
    Laravel,
    Statamic,
    Symfony,
    Craft,
    WordPress,
    Drupal,
    /// Any project with `public/index.php`.
    GenericPublic,
    /// `index.php` at the project root.
    BasicPhp,
    /// Static files only.
    Static,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Driver {
    pub kind: DriverKind,
    /// Absolute document root.
    pub docroot: PathBuf,
    /// Script that receives requests not matching a file, relative to docroot.
    /// `None` means static-only (404 on miss).
    pub front_controller: Option<String>,
}

impl DriverKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Laravel => "Laravel",
            Self::Statamic => "Statamic",
            Self::Symfony => "Symfony",
            Self::Craft => "Craft CMS",
            Self::WordPress => "WordPress",
            Self::Drupal => "Drupal",
            Self::GenericPublic => "PHP (public/)",
            Self::BasicPhp => "PHP",
            Self::Static => "Static",
        }
    }
}

fn composer_requires(root: &Path, package: &str) -> bool {
    std::fs::read_to_string(root.join("composer.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .map(|v| {
            ["require", "require-dev"]
                .iter()
                .any(|k| v.get(k).and_then(|r| r.get(package)).is_some())
        })
        .unwrap_or(false)
}

/// Detect the driver for a project directory.
pub fn detect(root: &Path) -> Driver {
    let has = |p: &str| root.join(p).exists();
    let make = |kind, docroot: PathBuf, fc: Option<&str>| Driver {
        kind,
        docroot,
        front_controller: fc.map(str::to_owned),
    };
    let public = root.join("public");

    if has("artisan") && has("public/index.php") {
        let kind = if composer_requires(root, "statamic/cms") {
            DriverKind::Statamic
        } else {
            DriverKind::Laravel
        };
        return make(kind, public, Some("index.php"));
    }
    if has("bin/console")
        && has("public/index.php")
        && composer_requires(root, "symfony/framework-bundle")
    {
        return make(DriverKind::Symfony, public, Some("index.php"));
    }
    if has("craft") && has("web/index.php") {
        return make(DriverKind::Craft, root.join("web"), Some("index.php"));
    }
    if has("wp-config.php") || has("wp-load.php") {
        return make(DriverKind::WordPress, root.to_path_buf(), Some("index.php"));
    }
    if has("web/wp/wp-load.php") || has("web/wp-config.php") {
        // Bedrock layout.
        return make(DriverKind::WordPress, root.join("web"), Some("index.php"));
    }
    if has("web/core/lib/Drupal.php") {
        return make(DriverKind::Drupal, root.join("web"), Some("index.php"));
    }
    if has("core/lib/Drupal.php") {
        return make(DriverKind::Drupal, root.to_path_buf(), Some("index.php"));
    }
    if has("public/index.php") {
        return make(DriverKind::GenericPublic, public, Some("index.php"));
    }
    if has("index.php") {
        return make(DriverKind::BasicPhp, root.to_path_buf(), Some("index.php"));
    }
    if public.join("index.html").exists() {
        return make(DriverKind::Static, public, None);
    }
    make(DriverKind::Static, root.to_path_buf(), None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(files: &[(&str, &str)]) -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        for (f, c) in files {
            let p = t.path().join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, c).unwrap();
        }
        t
    }

    #[test]
    fn laravel() {
        let t = tree(&[("artisan", ""), ("public/index.php", "")]);
        let d = detect(t.path());
        assert_eq!(d.kind, DriverKind::Laravel);
        assert_eq!(d.docroot, t.path().join("public"));
        assert_eq!(d.front_controller.as_deref(), Some("index.php"));
    }

    #[test]
    fn statamic_beats_laravel() {
        let t = tree(&[
            ("artisan", ""),
            ("public/index.php", ""),
            ("composer.json", r#"{"require":{"statamic/cms":"^5"}}"#),
        ]);
        assert_eq!(detect(t.path()).kind, DriverKind::Statamic);
    }

    #[test]
    fn symfony_needs_framework_bundle() {
        let files = [("bin/console", ""), ("public/index.php", "")];
        assert_eq!(detect(tree(&files).path()).kind, DriverKind::GenericPublic);
        let t = tree(&[
            files[0],
            files[1],
            (
                "composer.json",
                r#"{"require":{"symfony/framework-bundle":"^7"}}"#,
            ),
        ]);
        assert_eq!(detect(t.path()).kind, DriverKind::Symfony);
    }

    #[test]
    fn wordpress_and_bedrock() {
        let t = tree(&[("wp-config.php", ""), ("index.php", "")]);
        let d = detect(t.path());
        assert_eq!(
            (d.kind, d.docroot),
            (DriverKind::WordPress, t.path().to_path_buf())
        );
        let b = tree(&[("web/wp-config.php", ""), ("web/index.php", "")]);
        assert_eq!(detect(b.path()).docroot, b.path().join("web"));
    }

    #[test]
    fn craft_and_drupal() {
        assert_eq!(
            detect(tree(&[("craft", ""), ("web/index.php", "")]).path()).kind,
            DriverKind::Craft
        );
        assert_eq!(
            detect(tree(&[("web/core/lib/Drupal.php", "")]).path()).kind,
            DriverKind::Drupal
        );
    }

    #[test]
    fn fallbacks() {
        assert_eq!(
            detect(tree(&[("index.php", "")]).path()).kind,
            DriverKind::BasicPhp
        );
        let s = tree(&[("public/index.html", "")]);
        let d = detect(s.path());
        assert_eq!((d.kind, d.front_controller), (DriverKind::Static, None));
        assert_eq!(detect(tree(&[]).path()).kind, DriverKind::Static);
    }
}
