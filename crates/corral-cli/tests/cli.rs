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

fn corral_home(home: &Path, user_home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_corral"))
        .args(args)
        .env("CORRAL_HOME", home)
        .env("HOME", user_home)
        .env("CORRAL_LOG", "error")
        .current_dir(user_home)
        .output()
        .unwrap()
}

#[test]
fn shell_path_and_uninstall_round_trip() {
    let t = tempfile::tempdir().unwrap();
    let user = t.path().join("user");
    std::fs::create_dir_all(&user).unwrap();
    let zshrc = user.join(".zshrc");
    let original = "export EDITOR=vim\n";
    std::fs::write(&zshrc, original).unwrap();
    let home = t.path().join("corral");
    let sites = user.join("Sites");
    touch(&sites.join("app/index.php"));

    ok(&corral_home(
        &home,
        &user,
        &["park", sites.to_str().unwrap()],
    ));
    ok(&corral_home(&home, &user, &["render"]));
    ok(&corral_home(&home, &user, &["shell-path"]));
    assert!(
        std::fs::read_to_string(&zshrc).unwrap().contains(".corral")
            || std::fs::read_to_string(&zshrc)
                .unwrap()
                .contains(home.join("bin").to_str().unwrap())
    );
    let status = ok(&corral_home(&home, &user, &["status"]));
    assert!(status.contains("engine:   stopped"), "{status}");
    assert!(status.contains("PATH:     configured"), "{status}");

    // Pretend a service has data that must survive a plain uninstall.
    touch(&home.join("services/data/mysql/ibdata1"));

    let dry = ok(&corral_home(&home, &user, &["uninstall"]));
    assert!(dry.contains("Would remove"), "{dry}");
    assert!(home.join("config.toml").exists(), "dry run changes nothing");

    let out = ok(&corral_home(&home, &user, &["uninstall", "--yes"]));
    assert!(out.contains("Keeping service data"), "{out}");
    assert_eq!(
        std::fs::read_to_string(&zshrc).unwrap(),
        original,
        "rc restored exactly"
    );
    assert!(!home.join("config.toml").exists());
    assert!(!home.join("certs").exists());
    assert!(!home.join("bin").exists());
    assert!(home.join("services/data/mysql/ibdata1").exists());

    ok(&corral_home(
        &home,
        &user,
        &["uninstall", "--yes", "--delete-data"],
    ));
    assert!(!home.exists());
    assert!(
        sites.join("app/index.php").exists(),
        "user projects are never touched"
    );
}
