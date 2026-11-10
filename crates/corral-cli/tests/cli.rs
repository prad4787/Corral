//! End-to-end tests of the `corral` binary against a sandboxed CORRAL_HOME.
//! No network, no root, no system files touched.

use std::path::Path;
use std::process::{Command, Output};

fn corral(home: &Path, cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_corral"))
        .args(args)
        .env("CORRAL_HOME", home)
        .env("CORRAL_LOG", "error")
        .current_dir(cwd)
        .output()
        .unwrap()
}

fn ok(o: &Output) -> String {
    assert!(
        o.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn touch(p: &Path) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, "").unwrap();
}

#[test]
fn park_link_isolate_secure_flow() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    let sites = t.path().join("Sites");
    touch(&sites.join("shop/artisan"));
    touch(&sites.join("shop/public/index.php"));
    touch(&sites.join("blog/wp-config.php"));
    let api = t.path().join("api");
    touch(&api.join("public/index.php"));

    ok(&corral(&home, t.path(), &["park", sites.to_str().unwrap()]));
    ok(&corral(&home, &api, &["link", "my-api"]));
    ok(&corral(&home, &api, &["isolate", "8.3"]));
    ok(&corral(&home, t.path(), &["unsecure", "--site", "blog"]));

    let json: serde_json::Value =
        serde_json::from_str(&ok(&corral(&home, t.path(), &["--json", "sites"]))).unwrap();
    let by_name = |n: &str| {
        json.as_array()
            .unwrap()
            .iter()
            .find(|s| s["name"] == n)
            .unwrap()
            .clone()
    };
    assert_eq!(by_name("shop")["driver"]["kind"], "laravel");
    assert_eq!(by_name("blog")["secure"], false);
    assert_eq!(by_name("my-api")["php"], "8.3");
    assert_eq!(by_name("my-api")["origin"], "linked");

    let rendered = ok(&corral(&home, t.path(), &["render"]));
    assert!(rendered.contains("my-api wants PHP 8.3"), "{rendered}");
    assert!(home.join("generated/caddy.json").exists());
    assert!(home.join("certs/shop.test.crt").exists());
    assert!(!home.join("certs/blog.test.crt").exists());

    ok(&corral(&home, &api, &["unlink"]));
    let after = ok(&corral(&home, t.path(), &["sites"]));
    assert!(!after.contains("my-api"));
}

#[test]
fn rejects_bad_input() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    let o = corral(&home, t.path(), &["tld", "com"]);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("public suffix"));
    let o = corral(&home, t.path(), &["isolate", "8.4"]);
    assert!(String::from_utf8_lossy(&o.stderr).contains("not a Corral site"));
    let o = corral(&home, t.path(), &["use", "8.4"]);
    assert!(String::from_utf8_lossy(&o.stderr).contains("not installed"));
}

#[test]
fn tld_change_rotates_ca_and_hosts() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    let sites = t.path().join("Sites");
    touch(&sites.join("app/index.php"));
    ok(&corral(&home, t.path(), &["park", sites.to_str().unwrap()]));
    ok(&corral(&home, t.path(), &["render"]));
    let ca1 = std::fs::read(home.join("certs/ca.crt")).unwrap();
    ok(&corral(&home, t.path(), &["tld", "localhost"]));
    assert!(ok(&corral(&home, t.path(), &["sites"])).contains("app.localhost"));
    assert_ne!(std::fs::read(home.join("certs/ca.crt")).unwrap(), ca1);
    assert!(home.join("certs/app.localhost.crt").exists());
    assert!(!home.join("certs/app.test.crt").exists());
}
