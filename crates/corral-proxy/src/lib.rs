//! Generates Caddy's native JSON config from resolved sites.
//!
//! We hand Caddy fully explicit routes (no Caddyfile adapter, no automatic
//! HTTPS) so behavior is deterministic and testable. Certificates are issued by
//! Corral's own CA (`corral-tls`) and loaded from disk.
//!
//! PHP routing mirrors the well-known front-controller pattern:
//! 1. a directory request without trailing slash that contains `index.php` is redirected to add the slash;
//! 2. the URI is rewritten to the first existing of `{path}`, `{path}/index.php`, `<front controller>`;
//! 3. `*.php` is sent to the site's PHP-FPM socket over FastCGI;
//! 4. everything else is served as a static file.

use corral_core::paths::Paths;
use corral_core::sites::Site;
use corral_core::{Config, php::PhpVersion};
use serde_json::{Value, json};
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};

/// Where a site's TLS material lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CertFiles {
    pub cert: PathBuf,
    pub key: PathBuf,
}

/// The subset of a site the proxy needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxySite {
    pub host: String,
    pub docroot: PathBuf,
    pub front_controller: Option<String>,
    /// `None` for static sites, or when the site's PHP version is not installed.
    pub fpm_socket: Option<PathBuf>,
    /// Xdebug-enabled pool for the same PHP version, if available. Requests
    /// carrying an Xdebug trigger (cookie or query) are routed here.
    pub debug_socket: Option<PathBuf>,
    /// Send every PHP request to the debug pool (an IDE is listening).
    pub debug_all: bool,
    /// Proxy everything to a local HTTP service (e.g. `127.0.0.1:18082`)
    /// instead of serving files/PHP. Used by built-in tools.
    pub upstream: Option<String>,
    /// Present when the site is served over HTTPS.
    pub tls: Option<CertFiles>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxySettings {
    pub admin_socket: PathBuf,
    pub bind: Ipv4Addr,
    pub http_port: u16,
    pub https_port: u16,
    /// Port shown in HTTP→HTTPS redirects (443 when the helper forwards).
    pub public_https_port: u16,
    pub access_log: Option<PathBuf>,
}

impl ProxySettings {
    pub fn from_config(paths: &Paths, cfg: &Config) -> Self {
        Self {
            admin_socket: paths.run_dir().join("caddy-admin.sock"),
            bind: cfg.loopback,
            http_port: cfg.internal_http_port,
            https_port: cfg.internal_https_port,
            public_https_port: cfg.https_port,
            access_log: Some(paths.logs_dir().join("access.log")),
        }
    }
}

/// Map resolved sites to proxy sites. `installed` decides whether a site's PHP
/// version can actually be served; `cert_for` returns the cert files for a host.
pub fn proxy_sites(
    paths: &Paths,
    sites: &[Site],
    installed: &[PhpVersion],
    debuggable: &[PhpVersion],
    cert_for: impl Fn(&str) -> CertFiles,
) -> Vec<ProxySite> {
    sites
        .iter()
        .map(|s| {
            let version = s.driver.front_controller.as_ref().and_then(|_| {
                s.php
                    .as_deref()
                    .and_then(PhpVersion::parse)
                    .filter(|v| installed.contains(v))
                    .or_else(|| installed.first().copied())
            });
            let fpm_socket = version.map(|v| paths.fpm_socket(&v.to_string()));
            let debug_socket = version
                .filter(|v| debuggable.contains(v))
                .map(|v| paths.fpm_debug_socket(&v.to_string()));
            ProxySite {
                host: s.host.clone(),
                docroot: s.driver.docroot.clone(),
                front_controller: s.driver.front_controller.clone(),
                fpm_socket,
                debug_socket,
                debug_all: false,
                upstream: None,
                tls: s.secure.then(|| cert_for(&s.host)),
            }
        })
        .collect()
}

fn unix(p: &Path) -> String {
    format!("unix/{}", p.display())
}

fn fastcgi(site: &ProxySite, socket: &Path) -> Value {
    json!({
        "handler": "reverse_proxy",
        "transport": {
            "protocol": "fastcgi",
            "root": site.docroot.display().to_string(),
            "split_path": [".php"],
            "env": { "SERVER_NAME": site.host }
        },
        "upstreams": [{ "dial": unix(socket) }]
    })
}

/// Matcher sets (OR-ed) that identify an Xdebug-triggered request.
fn xdebug_triggers() -> Value {
    json!([
        { "path": ["*.php"], "header_regexp": { "Cookie": { "pattern": "(^|;\\s*)XDEBUG_(SESSION|TRIGGER|PROFILE|TRACE)=" } } },
        { "path": ["*.php"], "query": { "XDEBUG_SESSION_START": ["*"] } },
        { "path": ["*.php"], "query": { "XDEBUG_SESSION": ["*"] } },
        { "path": ["*.php"], "query": { "XDEBUG_TRIGGER": ["*"] } }
    ])
}

fn php_routes(site: &ProxySite, socket: &Path, fc: &str) -> Vec<Value> {
    let mut v = vec![
        json!({
            "match": [{
                "file": { "try_files": ["{http.request.uri.path}/index.php"] },
                "not": [{ "path": ["*/"] }]
            }],
            "handle": [{
                "handler": "static_response",
                "status_code": 308,
                "headers": { "Location": ["{http.request.orig_uri.path}/{http.request.orig_uri.prefixed_query}"] }
            }]
        }),
        json!({
            "match": [{
                "file": {
                    "try_files": ["{http.request.uri.path}", "{http.request.uri.path}/index.php", fc],
                    "split_path": [".php"]
                }
            }],
            "handle": [{ "handler": "rewrite", "uri": "{http.matchers.file.relative}" }]
        }),
    ];
    if let Some(dbg) = &site.debug_socket {
        let matcher = if site.debug_all {
            json!([{ "path": ["*.php"] }])
        } else {
            xdebug_triggers()
        };
        v.push(json!({
            "match": matcher,
            "handle": [fastcgi(site, dbg)],
            "terminal": true
        }));
    }
    v.push(json!({
        "match": [{ "path": ["*.php"] }],
        "handle": [fastcgi(site, socket)]
    }));
    v
}

fn site_route(site: &ProxySite) -> Value {
    if let Some(up) = &site.upstream {
        return json!({
            "match": [{ "host": [site.host] }],
            "handle": [{
                "handler": "reverse_proxy",
                "upstreams": [{ "dial": up }]
            }],
            "terminal": true
        });
    }
    let mut routes = vec![json!({
        "handle": [{ "handler": "vars", "root": site.docroot.display().to_string() }]
    })];
    match (&site.front_controller, &site.fpm_socket) {
        (Some(fc), Some(sock)) => routes.extend(php_routes(site, sock, fc)),
        (Some(_), None) => routes.push(json!({
            "match": [{ "path": ["*.php"] }],
            "handle": [{
                "handler": "static_response",
                "status_code": 503,
                "body": "Corral: the PHP version for this site is not installed."
            }],
            "terminal": true
        })),
        (None, _) => {}
    }
    routes.push(json!({
        "handle": [{ "handler": "file_server", "hide": [".env", ".git"] }]
    }));
    json!({
        "match": [{ "host": [site.host] }],
        "handle": [{ "handler": "subroute", "routes": routes }],
        "terminal": true
    })
}

fn not_found_route() -> Value {
    json!({
        "handle": [{
            "handler": "static_response",
            "status_code": 404,
            "headers": { "Content-Type": ["text/plain; charset=utf-8"] },
            "body": "Corral: no site is configured for {http.request.host}.\n"
        }]
    })
}

/// Build the complete Caddy JSON document.
pub fn caddy_config(settings: &ProxySettings, sites: &[ProxySite]) -> Value {
    let mut sites: Vec<&ProxySite> = sites.iter().collect();
    sites.sort_by(|a, b| a.host.cmp(&b.host)); // by reference: no clone

    let redirect_port = if settings.public_https_port == 443 {
        String::new()
    } else {
        format!(":{}", settings.public_https_port)
    };
    let mut http_routes: Vec<Value> = Vec::new();
    let mut https_routes: Vec<Value> = Vec::new();
    let mut certs: Vec<Value> = Vec::new();

    for s in &sites {
        if let Some(tls) = &s.tls {
            http_routes.push(json!({
                "match": [{ "host": [s.host] }],
                "handle": [{
                    "handler": "static_response",
                    "status_code": 308,
                    "headers": { "Location": [format!("https://{{http.request.host}}{redirect_port}{{http.request.uri}}")] }
                }],
                "terminal": true
            }));
            https_routes.push(site_route(s));
            certs.push(json!({
                "certificate": tls.cert.display().to_string(),
                "key": tls.key.display().to_string(),
                "tags": [s.host]
            }));
        } else {
            http_routes.push(site_route(s));
        }
    }
    http_routes.push(not_found_route());
    https_routes.push(not_found_route());

    let logs = settings
        .access_log
        .as_ref()
        .map(|_| json!({ "default_logger_name": "access" }));
    let mut http_server = json!({
        "listen": [format!("{}:{}", settings.bind, settings.http_port)],
        "routes": http_routes,
        "automatic_https": { "disable": true }
    });
    let mut https_server = json!({
        "listen": [format!("{}:{}", settings.bind, settings.https_port)],
        "routes": https_routes,
        "automatic_https": { "disable": true },
        "tls_connection_policies": [{}]
    });
    if let Some(l) = logs {
        http_server["logs"] = l.clone();
        https_server["logs"] = l;
    }

    let mut logging = json!({ "logs": { "default": { "level": "WARN" } } });
    if let Some(path) = &settings.access_log {
        logging["logs"]["access"] = json!({
            "writer": { "output": "file", "filename": path.display().to_string(), "roll_size_mb": 20, "roll_keep": 3 },
            "encoder": { "format": "json" },
            "include": ["http.log.access.access"]
        });
        logging["logs"]["default"]["exclude"] = json!(["http.log.access.access"]);
    }

    json!({
        "admin": { "listen": unix(&settings.admin_socket) },
        "logging": logging,
        "apps": {
            "http": { "servers": { "corral_http": http_server, "corral_https": https_server } },
            "tls": { "certificates": { "load_files": certs } }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> ProxySettings {
        ProxySettings {
            admin_socket: "/c/run/caddy-admin.sock".into(),
            bind: Ipv4Addr::LOCALHOST,
            http_port: 18080,
            https_port: 18443,
            public_https_port: 443,
            access_log: None,
        }
    }

    fn php_site(host: &str, tls: bool) -> ProxySite {
        ProxySite {
            host: host.into(),
            docroot: format!("/sites/{host}/public").into(),
            front_controller: Some("index.php".into()),
            fpm_socket: Some("/c/run/php-fpm-8.4.sock".into()),
            debug_socket: None,
            debug_all: false,
            upstream: None,
            tls: tls.then(|| CertFiles {
                cert: format!("/c/certs/{host}.crt").into(),
                key: format!("/c/certs/{host}.key").into(),
            }),
        }
    }

    fn routes<'a>(cfg: &'a Value, server: &str) -> &'a Vec<Value> {
        cfg["apps"]["http"]["servers"][server]["routes"]
            .as_array()
            .unwrap()
    }

    #[test]
    fn insecure_php_site_is_on_http() {
        let cfg = caddy_config(&settings(), &[php_site("blog.test", false)]);
        let http = routes(&cfg, "corral_http");
        assert_eq!(http.len(), 2, "site + 404 fallback");
        assert_eq!(http[0]["match"][0]["host"][0], "blog.test");
        let sub = http[0]["handle"][0]["routes"].as_array().unwrap();
        let rp = sub
            .iter()
            .find(|r| r["handle"][0]["handler"] == "reverse_proxy")
            .unwrap();
        assert_eq!(
            rp["handle"][0]["upstreams"][0]["dial"],
            "unix//c/run/php-fpm-8.4.sock"
        );
        assert_eq!(
            rp["handle"][0]["transport"]["root"],
            "/sites/blog.test/public"
        );
        assert_eq!(routes(&cfg, "corral_https").len(), 1);
        assert_eq!(cfg["admin"]["listen"], "unix//c/run/caddy-admin.sock");
        assert_eq!(
            cfg["apps"]["http"]["servers"]["corral_http"]["listen"][0],
            "127.0.0.1:18080"
        );
    }

    #[test]
    fn secure_site_redirects_and_loads_cert() {
        let mut s = settings();
        s.public_https_port = 18443;
        let cfg = caddy_config(&s, &[php_site("shop.test", true)]);
        let redirect = &routes(&cfg, "corral_http")[0]["handle"][0];
        assert_eq!(redirect["status_code"], 308);
        assert_eq!(
            redirect["headers"]["Location"][0],
            "https://{http.request.host}:18443{http.request.uri}"
        );
        assert_eq!(
            routes(&cfg, "corral_https")[0]["match"][0]["host"][0],
            "shop.test"
        );
        let certs = &cfg["apps"]["tls"]["certificates"]["load_files"];
        assert_eq!(certs[0]["certificate"], "/c/certs/shop.test.crt");
    }

    #[test]
    fn front_controller_is_last_try_file() {
        let mut site = php_site("craft.test", false);
        site.front_controller = Some("index.php".into());
        let cfg = caddy_config(&settings(), &[site]);
        let sub = routes(&cfg, "corral_http")[0]["handle"][0]["routes"]
            .as_array()
            .unwrap();
        let rw = sub
            .iter()
            .find(|r| r["handle"][0]["handler"] == "rewrite")
            .unwrap();
        let tf = rw["match"][0]["file"]["try_files"].as_array().unwrap();
        assert_eq!(tf.last().unwrap(), "index.php");
    }

    #[test]
    fn missing_php_gives_503_and_static_has_no_fastcgi() {
        let mut missing = php_site("old.test", false);
        missing.fpm_socket = None;
        let stat = ProxySite {
            host: "docs.test".into(),
            docroot: "/sites/docs".into(),
            front_controller: None,
            fpm_socket: None,
            debug_socket: None,
            debug_all: false,
            upstream: None,
            tls: None,
        };
        let cfg = caddy_config(&settings(), &[missing, stat]);
        let text = cfg.to_string();
        assert!(text.contains("\"status_code\":503"));
        let http = routes(&cfg, "corral_http");
        // Sorted by host: docs.test first.
        let docs = http[0]["handle"][0]["routes"].to_string();
        assert!(!docs.contains("fastcgi"));
        assert!(docs.contains("file_server"));
    }

    #[test]
    fn xdebug_trigger_routes_to_debug_pool_first() {
        let mut site = php_site("app.test", false);
        site.debug_socket = Some("/c/run/php-fpm-8.4-debug.sock".into());
        let cfg = caddy_config(&settings(), &[site]);
        let sub = routes(&cfg, "corral_http")[0]["handle"][0]["routes"]
            .as_array()
            .unwrap();
        let dials: Vec<&str> = sub
            .iter()
            .filter(|r| r["handle"][0]["handler"] == "reverse_proxy")
            .map(|r| r["handle"][0]["upstreams"][0]["dial"].as_str().unwrap())
            .collect();
        assert_eq!(
            dials,
            [
                "unix//c/run/php-fpm-8.4-debug.sock",
                "unix//c/run/php-fpm-8.4.sock"
            ]
        );
        let dbg = sub.iter().find(|r| r["terminal"] == true).unwrap();
        let m = dbg["match"].as_array().unwrap();
        assert_eq!(m.len(), 4, "cookie + three query triggers");
        assert!(
            m[0]["header_regexp"]["Cookie"]["pattern"]
                .as_str()
                .unwrap()
                .contains("XDEBUG_")
        );
        assert_eq!(m[1]["query"]["XDEBUG_SESSION_START"][0], "*");
        // While an IDE listens, every PHP request goes to the debug pool.
        let mut all = php_site("app.test", false);
        all.debug_socket = Some("/c/run/php-fpm-8.4-debug.sock".into());
        all.debug_all = true;
        let cfg_all = caddy_config(&settings(), &[all]);
        let sub = routes(&cfg_all, "corral_http")[0]["handle"][0]["routes"]
            .as_array()
            .unwrap();
        let dbg = sub.iter().find(|r| r["terminal"] == true).unwrap();
        assert_eq!(dbg["match"], json!([{ "path": ["*.php"] }]));
        // Without a debug pool there is no trigger route.
        let plain = caddy_config(&settings(), &[php_site("b.test", false)]);
        assert!(!plain.to_string().contains("XDEBUG"));
    }

    #[test]
    fn upstream_sites_reverse_proxy() {
        let mut site = php_site("redis.test", false);
        site.upstream = Some("127.0.0.1:18083".into());
        let cfg = caddy_config(&settings(), &[site]);
        let r = &routes(&cfg, "corral_http")[0];
        assert_eq!(r["handle"][0]["handler"], "reverse_proxy");
        assert_eq!(r["handle"][0]["upstreams"][0]["dial"], "127.0.0.1:18083");
        assert!(!r.to_string().contains("fastcgi"));
    }

    #[test]
    fn access_log_wiring() {
        let mut s = settings();
        s.access_log = Some("/c/logs/access.log".into());
        let cfg = caddy_config(&s, &[]);
        assert_eq!(
            cfg["logging"]["logs"]["access"]["writer"]["filename"],
            "/c/logs/access.log"
        );
        assert_eq!(
            cfg["apps"]["http"]["servers"]["corral_http"]["logs"]["default_logger_name"],
            "access"
        );
    }

    #[test]
    fn maps_sites_to_installed_php() {
        use corral_core::sites::Registry;
        let t = tempfile::tempdir().unwrap();
        let paths = Paths::at(t.path().join("c"));
        paths.ensure().unwrap();
        let parked = t.path().join("Sites");
        std::fs::create_dir_all(parked.join("app/public")).unwrap();
        std::fs::write(parked.join("app/public/index.php"), "").unwrap();
        std::fs::create_dir_all(parked.join("docs")).unwrap();
        let cfg = Config {
            parked: vec![parked],
            default_php: Some("8.1".into()), // not installed → falls back to newest
            ..Default::default()
        };
        let sites = Registry::load(&paths).unwrap().resolve(&cfg).unwrap();
        let installed = [
            PhpVersion::parse("8.4").unwrap(),
            PhpVersion::parse("8.3").unwrap(),
        ];
        let ps = proxy_sites(&paths, &sites, &installed, &installed[..1], |h| CertFiles {
            cert: format!("{h}.crt").into(),
            key: format!("{h}.key").into(),
        });
        assert_eq!(
            ps[0].fpm_socket.as_deref(),
            Some(paths.fpm_socket("8.4").as_path())
        );
        assert_eq!(
            ps[0].tls.as_ref().unwrap().cert,
            PathBuf::from("app.test.crt")
        );
        assert_eq!(ps[1].fpm_socket, None);
    }
}
