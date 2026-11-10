//! Tauri shell. Kept deliberately thin: every command delegates to
//! `corral-core` / `corral-engine` / `corral-services`, the same code the CLI uses.

use corral_core::php::{self, PhpVersion};
use corral_core::sites::{Registry, Site};
use corral_core::{Config, Paths};
use corral_engine::{install, orphans};
use corral_services::store::ServicesFile;
use serde::Serialize;
use std::sync::Mutex;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tokio::sync::oneshot;

/// Errors cross the IPC boundary as plain strings.
type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

struct AppState {
    paths: Paths,
    /// Present while the app's own engine is running; sending stops it.
    engine: Mutex<Option<oneshot::Sender<()>>>,
}

#[derive(Serialize)]
struct SiteView {
    #[serde(flatten)]
    site: Site,
    url: String,
    driver_label: &'static str,
}

#[derive(Serialize)]
struct PhpView {
    version: String,
    is_default: bool,
}

#[derive(Serialize)]
struct ServiceView {
    name: String,
    kind: String,
    label: &'static str,
    version: String,
    port: u16,
    enabled: bool,
    listening: bool,
}

#[derive(Serialize)]
struct NodeView {
    major: u32,
    version: String,
    is_default: bool,
}

#[derive(Serialize)]
struct Overview {
    node: Vec<NodeView>,
    sites: Vec<SiteView>,
    php: Vec<PhpView>,
    services: Vec<ServiceView>,
    tld: String,
    parked: Vec<String>,
    default_php: Option<String>,
    /// The app's own engine is running.
    running: bool,
    /// An engine started elsewhere (`corral start`) is running.
    running_elsewhere: bool,
    caddy_installed: bool,
    composer_installed: bool,
    path_configured: bool,
    autostart: bool,
    http_port: u16,
    https_port: u16,
}

fn home() -> std::path::PathBuf {
    std::env::var_os("HOME").map(Into::into).unwrap_or_default()
}

fn overview_inner(app: &AppHandle, state: &AppState) -> anyhow::Result<Overview> {
    let paths = &state.paths;
    let cfg = Config::load(paths)?;
    let sites = Registry::load(paths)?
        .resolve(&cfg)?
        .into_iter()
        .map(|s| SiteView {
            url: s.url(&cfg),
            driver_label: s.driver.kind.label(),
            site: s,
        })
        .collect();
    let php = php::installed(paths)?
        .into_iter()
        .map(|i| PhpView {
            is_default: cfg.default_php.as_deref() == Some(&i.version.to_string()),
            version: i.version.to_string(),
        })
        .collect();
    let services = ServicesFile::load(paths)?
        .instances
        .into_iter()
        .map(|(name, i)| ServiceView {
            name,
            kind: i.kind.as_str().into(),
            label: i.kind.label(),
            version: i.version,
            port: i.port,
            enabled: i.enabled,
            listening: corral_services::port_in_use(i.port),
        })
        .collect();
    let running = state.engine.lock().unwrap().is_some();
    let node = corral_engine::node::installed(paths)
        .into_iter()
        .map(|(major, version)| NodeView {
            is_default: cfg.default_node == Some(major),
            major,
            version,
        })
        .collect();
    Ok(Overview {
        node,
        sites,
        php,
        services,
        tld: cfg.tld.clone(),
        parked: cfg.parked.iter().map(|p| p.display().to_string()).collect(),
        default_php: cfg.default_php.clone(),
        running,
        running_elsewhere: !running && orphans::engine_running(paths),
        caddy_installed: install::caddy_bin(paths).exists(),
        composer_installed: install::composer_phar(paths).exists(),
        path_configured: corral_core::shell::rc_files(&home())
            .iter()
            .any(|f| corral_core::shell::has_block(f)),
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
        http_port: cfg.http_port,
        https_port: cfg.https_port,
    })
}

macro_rules! ov {
    ($app:expr, $state:expr) => {
        overview_inner(&$app, &$state).map_err(err)
    };
}

#[tauri::command]
fn overview(app: AppHandle, state: State<'_, AppState>) -> CmdResult<Overview> {
    ov!(app, state)
}

#[tauri::command]
fn park(app: AppHandle, state: State<'_, AppState>, dir: String) -> CmdResult<Overview> {
    let mut cfg = Config::load(&state.paths).map_err(err)?;
    let dir = std::path::PathBuf::from(dir).canonicalize().map_err(err)?;
    if !cfg.parked.contains(&dir) {
        cfg.parked.push(dir);
        cfg.save(&state.paths).map_err(err)?;
    }
    ov!(app, state)
}

#[tauri::command]
fn unpark(app: AppHandle, state: State<'_, AppState>, dir: String) -> CmdResult<Overview> {
    let mut cfg = Config::load(&state.paths).map_err(err)?;
    cfg.parked.retain(|p| p.display().to_string() != dir);
    cfg.save(&state.paths).map_err(err)?;
    ov!(app, state)
}

#[tauri::command]
fn link(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    name: Option<String>,
) -> CmdResult<Overview> {
    let mut reg = Registry::load(&state.paths).map_err(err)?;
    reg.link(
        std::path::Path::new(&path),
        name.as_deref().filter(|n| !n.is_empty()),
    )
    .map_err(err)?;
    reg.save().map_err(err)?;
    ov!(app, state)
}

#[tauri::command]
fn unlink(app: AppHandle, state: State<'_, AppState>, name: String) -> CmdResult<Overview> {
    let mut reg = Registry::load(&state.paths).map_err(err)?;
    reg.unlink(&name).map_err(err)?;
    reg.save().map_err(err)?;
    ov!(app, state)
}

#[tauri::command]
fn set_site_php(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
    version: Option<String>,
) -> CmdResult<Overview> {
    let cfg = Config::load(&state.paths).map_err(err)?;
    let version = match version.filter(|v| !v.is_empty()) {
        Some(v) => Some(
            PhpVersion::parse(&v)
                .ok_or("invalid PHP version")?
                .to_string(),
        ),
        None => None,
    };
    let mut reg = Registry::load(&state.paths).map_err(err)?;
    reg.isolate(&cfg, &name, version).map_err(err)?;
    reg.save().map_err(err)?;
    ov!(app, state)
}

#[tauri::command]
fn set_site_secure(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
    secure: bool,
) -> CmdResult<Overview> {
    let cfg = Config::load(&state.paths).map_err(err)?;
    let mut reg = Registry::load(&state.paths).map_err(err)?;
    let value = (secure != cfg.secure_by_default).then_some(secure);
    reg.set_secure(&cfg, &name, value).map_err(err)?;
    reg.save().map_err(err)?;
    ov!(app, state)
}

#[tauri::command]
fn set_default_php(
    app: AppHandle,
    state: State<'_, AppState>,
    version: String,
) -> CmdResult<Overview> {
    let v = PhpVersion::parse(&version).ok_or("invalid PHP version")?;
    let mut cfg = Config::load(&state.paths).map_err(err)?;
    cfg.default_php = Some(v.to_string());
    cfg.save(&state.paths).map_err(err)?;
    php::write_shims(&state.paths, Some(v)).map_err(err)?;
    ov!(app, state)
}

fn start_engine_inner(app: &AppHandle, state: &AppState) -> CmdResult<()> {
    let mut guard = state.engine.lock().unwrap();
    if guard.is_some() {
        return Ok(());
    }
    if orphans::engine_running(&state.paths) {
        return Err(
            "Corral is already running from the terminal (`corral start`). Stop it there first."
                .into(),
        );
    }
    if !install::caddy_bin(&state.paths).exists() {
        return Err("Install the web server first (Setup).".into());
    }
    let (tx, rx) = oneshot::channel::<()>();
    let paths = state.paths.clone();
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = corral_engine::run(paths, async {
            let _ = rx.await;
        })
        .await;
        if let Err(e) = result {
            tracing::error!(error = %e, "engine stopped with an error");
        }
        // Engine ended (stopped or failed): clear state so the UI reflects it.
        if let Some(st) = handle.try_state::<AppState>() {
            st.engine.lock().unwrap().take();
        }
        refresh_tray(&handle);
    });
    *guard = Some(tx);
    drop(guard);
    refresh_tray(app);
    Ok(())
}

fn stop_engine_inner(app: &AppHandle, state: &AppState) {
    if let Some(tx) = state.engine.lock().unwrap().take() {
        let _ = tx.send(());
    }
    refresh_tray(app);
}

#[tauri::command]
fn start_engine(app: AppHandle, state: State<'_, AppState>) -> CmdResult<Overview> {
    start_engine_inner(&app, &state)?;
    ov!(app, state)
}

#[tauri::command]
fn stop_engine(app: AppHandle, state: State<'_, AppState>) -> CmdResult<Overview> {
    stop_engine_inner(&app, &state);
    ov!(app, state)
}

/// Long-running installs. `what`: "web" (Caddy + Composer), "php" (needs
/// `version`; unverified upstream build), or "mysql".
#[tauri::command]
async fn install_component(
    app: AppHandle,
    state: State<'_, AppState>,
    what: String,
    version: Option<String>,
) -> CmdResult<Overview> {
    let paths = state.paths.clone();
    match what.as_str() {
        "web" => {
            if !install::caddy_bin(&paths).exists() {
                install::install_caddy(&paths).await.map_err(err)?;
            }
            if !install::composer_phar(&paths).exists() {
                install::install_composer(&paths).await.map_err(err)?;
            }
            // First run on a machine where ports 80/443 need the helper: use high ports.
            let mut cfg = Config::load(&paths).map_err(err)?;
            if cfg.http_port == 80 {
                cfg.http_port = cfg.internal_http_port;
                cfg.https_port = cfg.internal_https_port;
                cfg.save(&paths).map_err(err)?;
            }
        }
        "php" => {
            let v = PhpVersion::parse(version.as_deref().unwrap_or("8.4"))
                .ok_or("invalid PHP version")?;
            let full = install::latest_static_php(v).await.map_err(err)?;
            let src = install::PhpSource::static_php(&full);
            install::install_php(&paths, v, &src, true)
                .await
                .map_err(err)?;
            let mut cfg = Config::load(&paths).map_err(err)?;
            if cfg.default_php.is_none() {
                cfg.default_php = Some(v.to_string());
                cfg.save(&paths).map_err(err)?;
            }
        }
        "mysql" | "valkey" | "postgres" => {
            let kind = corral_services::manifest::Kind::parse(&what).ok_or("unknown service")?;
            corral_services::add(&paths, kind, version.as_deref(), None, None)
                .await
                .map_err(err)?;
        }
        other => return Err(format!("unknown component {other:?}")),
    }
    corral_engine::render(&paths).map_err(err)?;
    ov!(app, state)
}

#[tauri::command]
fn set_service_enabled(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
    enabled: bool,
) -> CmdResult<Overview> {
    let mut f = ServicesFile::load(&state.paths).map_err(err)?;
    f.set_enabled(&name, enabled).map_err(err)?;
    f.save(&state.paths).map_err(err)?;
    ov!(app, state)
}

#[tauri::command]
fn set_shell_path(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> CmdResult<Overview> {
    for rc in corral_core::shell::rc_files(&home()) {
        if enabled {
            corral_core::shell::install(&rc, &state.paths.bin).map_err(err)?;
        } else {
            corral_core::shell::uninstall(&rc).map_err(err)?;
        }
    }
    ov!(app, state)
}

#[tauri::command]
fn set_autostart(app: AppHandle, state: State<'_, AppState>, enabled: bool) -> CmdResult<Overview> {
    let al = app.autolaunch();
    if enabled {
        al.enable().map_err(err)?;
    } else {
        al.disable().map_err(err)?;
    }
    ov!(app, state)
}

fn site_path(state: &AppState, name: &str) -> CmdResult<std::path::PathBuf> {
    let cfg = Config::load(&state.paths).map_err(err)?;
    Registry::load(&state.paths)
        .map_err(err)?
        .resolve(&cfg)
        .map_err(err)?
        .into_iter()
        .find(|s| s.name == name)
        .map(|s| s.path)
        .ok_or_else(|| format!("no site named {name:?}"))
}

/// Read a site's `.env` (only for sites Corral manages).
#[tauri::command]
fn read_env(state: State<'_, AppState>, site: String) -> CmdResult<Option<String>> {
    let f = site_path(&state, &site)?.join(".env");
    match std::fs::read_to_string(&f) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(err(e)),
    }
}

/// Save a site's `.env`. The first save keeps `.env.corral-backup`.
#[tauri::command]
fn write_env(state: State<'_, AppState>, site: String, contents: String) -> CmdResult<()> {
    let f = site_path(&state, &site)?.join(".env");
    let backup = f.with_file_name(".env.corral-backup");
    if f.exists() && !backup.exists() {
        std::fs::copy(&f, &backup).map_err(err)?;
    }
    corral_core::fsutil::write_atomic(&f, contents.as_bytes()).map_err(err)
}

#[tauri::command]
fn read_php_ini(state: State<'_, AppState>, version: String) -> CmdResult<String> {
    let v = PhpVersion::parse(&version).ok_or("invalid PHP version")?;
    let inst = php::find(&state.paths, v)
        .map_err(err)?
        .ok_or("PHP version not installed")?;
    Ok(
        std::fs::read_to_string(php::user_ini(&inst)).unwrap_or_else(|_| {
            "; Your PHP overrides, e.g.\n; memory_limit = 1G\n; upload_max_filesize = 1G\n".into()
        }),
    )
}

/// Save the version's 90-user.ini; a running engine restarts that pool.
#[tauri::command]
fn write_php_ini(state: State<'_, AppState>, version: String, contents: String) -> CmdResult<()> {
    let v = PhpVersion::parse(&version).ok_or("invalid PHP version")?;
    let inst = php::find(&state.paths, v)
        .map_err(err)?
        .ok_or("PHP version not installed")?;
    std::fs::create_dir_all(inst.conf_d()).map_err(err)?;
    corral_core::fsutil::write_atomic(&php::user_ini(&inst), contents.as_bytes()).map_err(err)
}

#[tauri::command]
async fn node_install(
    app: AppHandle,
    state: State<'_, AppState>,
    major: Option<u32>,
) -> CmdResult<Overview> {
    let paths = state.paths.clone();
    let (m, _) = corral_engine::node::install(&paths, major)
        .await
        .map_err(err)?;
    let mut cfg = Config::load(&paths).map_err(err)?;
    if cfg.default_node.is_none() {
        cfg.default_node = Some(m);
        cfg.save(&paths).map_err(err)?;
    }
    corral_engine::node::write_shims(&paths, cfg.default_node).map_err(err)?;
    ov!(app, state)
}

#[tauri::command]
fn node_use(app: AppHandle, state: State<'_, AppState>, major: u32) -> CmdResult<Overview> {
    corral_engine::node::write_shims(&state.paths, Some(major)).map_err(err)?;
    let mut cfg = Config::load(&state.paths).map_err(err)?;
    cfg.default_node = Some(major);
    cfg.save(&state.paths).map_err(err)?;
    ov!(app, state)
}

/// Create a Laravel project (optionally from a starter kit) in the first
/// parked folder via `composer create-project`.
#[tauri::command]
async fn new_project(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
    kit: String,
) -> CmdResult<Overview> {
    let paths = state.paths.clone();
    let package = match kit.as_str() {
        "none" | "" => "laravel/laravel",
        "react" => "laravel/react-starter-kit",
        "vue" => "laravel/vue-starter-kit",
        "livewire" => "laravel/livewire-starter-kit",
        _ => return Err(format!("unknown kit {kit:?}")),
    };
    if corral_core::sites::slugify(&name).as_deref() != Some(name.as_str()) {
        return Err("Use lowercase letters, digits and '-' for the project name.".into());
    }
    let cfg = Config::load(&paths).map_err(err)?;
    let parent = cfg
        .parked
        .first()
        .cloned()
        .ok_or("Park a folder first; new projects are created inside it.")?;
    let dir = parent.join(&name);
    if dir.exists() {
        return Err(format!("{} already exists", dir.display()));
    }
    if !install::composer_phar(&paths).exists() || !paths.bin.join("php").exists() {
        return Err("Install the web server (with Composer) and PHP first.".into());
    }
    let out = tokio::process::Command::new(paths.bin.join("composer"))
        .args(["create-project", package])
        .arg(&dir)
        .args(["--no-interaction", "--prefer-dist", "--no-ansi"])
        .env(
            "PATH",
            format!(
                "{}:{}",
                paths.bin.display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .output()
        .await
        .map_err(err)?;
    if !out.status.success() {
        let tail: Vec<String> = String::from_utf8_lossy(&out.stderr)
            .lines()
            .rev()
            .take(12)
            .map(str::to_owned)
            .collect();
        return Err(format!(
            "composer create-project failed:\n{}",
            tail.into_iter().rev().collect::<Vec<_>>().join("\n")
        ));
    }
    ov!(app, state)
}

fn mail_store(state: &AppState) -> corral_mail::Store {
    corral_mail::Store::new(corral_mail::store_dir(&state.paths.root))
}

#[tauri::command]
fn mail_list(state: State<'_, AppState>, limit: Option<usize>) -> Vec<corral_mail::Summary> {
    mail_store(&state).list(limit.unwrap_or(200))
}

#[tauri::command]
fn mail_detail(state: State<'_, AppState>, id: String) -> CmdResult<corral_mail::Detail> {
    mail_store(&state).detail(&id).map_err(err)
}

#[tauri::command]
fn mail_delete(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    mail_store(&state).delete(&id).map_err(err)
}

#[tauri::command]
fn mail_clear(state: State<'_, AppState>) -> CmdResult<usize> {
    mail_store(&state).clear().map_err(err)
}

/// Save an attachment to ~/Downloads (never overwriting) and return its path.
#[tauri::command]
fn mail_save_attachment(state: State<'_, AppState>, id: String, index: usize) -> CmdResult<String> {
    let (name, bytes) = mail_store(&state).attachment(&id, index).map_err(err)?;
    let dir = home().join("Downloads");
    std::fs::create_dir_all(&dir).map_err(err)?;
    let base = std::path::Path::new(&name);
    let (stem, ext) = (
        base.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("attachment"),
        base.extension().and_then(|s| s.to_str()),
    );
    let mut path = dir.join(&name);
    let mut n = 1;
    while path.exists() {
        path = dir.join(match ext {
            Some(e) => format!("{stem} ({n}).{e}"),
            None => format!("{stem} ({n})"),
        });
        n += 1;
    }
    std::fs::write(&path, bytes).map_err(err)?;
    Ok(path.display().to_string())
}

#[tauri::command]
fn dumps_list(state: State<'_, AppState>, limit: Option<usize>) -> Vec<corral_dumps::Dump> {
    corral_dumps::Store::new(corral_dumps::store_file(&state.paths.root)).list(limit.unwrap_or(100))
}

#[tauri::command]
fn events_list(
    state: State<'_, AppState>,
    limit: Option<usize>,
) -> Vec<corral_dumps::events::Event> {
    corral_dumps::events::store(&state.paths.root).list(limit.unwrap_or(300))
}

#[tauri::command]
fn events_clear(state: State<'_, AppState>) -> CmdResult<()> {
    corral_dumps::events::store(&state.paths.root)
        .clear()
        .map_err(err)
}

#[tauri::command]
fn dumps_clear(state: State<'_, AppState>) -> CmdResult<()> {
    corral_dumps::Store::new(corral_dumps::store_file(&state.paths.root))
        .clear()
        .map_err(err)
}

#[tauri::command]
fn site_logs(
    state: State<'_, AppState>,
    site: String,
    level: Option<String>,
    limit: Option<usize>,
) -> CmdResult<Vec<corral_core::logs::Entry>> {
    use corral_core::logs;
    let path = site_path(&state, &site)?;
    let Some(file) = logs::log_files(&path).into_iter().next() else {
        return Ok(Vec::new());
    };
    let min = level.as_deref().and_then(logs::Level::parse);
    logs::tail(&file, 4 << 20, limit.unwrap_or(100), min).map_err(err)
}

/// Open a project folder in an editor or terminal.
#[tauri::command]
fn open_in(path: String, target: String) -> CmdResult<()> {
    let dir = std::path::PathBuf::from(&path);
    if !dir.is_dir() {
        return Err(format!("{path} is not a directory"));
    }
    let try_bundle = |id: &str| {
        std::process::Command::new("/usr/bin/open")
            .args(["-b", id])
            .arg(&dir)
            .status()
            .is_ok_and(|s| s.success())
    };
    let ok = match target.as_str() {
        "editor" => [
            "com.microsoft.VSCode",
            "com.todesktop.230313mzl4w4u92", // Cursor
            "dev.zed.Zed",
            "com.jetbrains.PhpStorm",
            "com.sublimetext.4",
        ]
        .iter()
        .any(|id| try_bundle(id)),
        "terminal" => [
            "com.mitchellh.ghostty",
            "dev.warp.Warp-Stable",
            "com.googlecode.iterm2",
            "com.apple.Terminal",
        ]
        .iter()
        .any(|id| try_bundle(id)),
        "tinker" => {
            // A .command file opens in Terminal and runs in the project.
            let script =
                std::env::temp_dir().join(format!("corral-tinker-{}.command", std::process::id()));
            let bin = Paths::discover().map_err(err)?.bin;
            let body = format!(
                "#!/bin/sh\ncd \"{}\" || exit 1\nexport PATH=\"{}:$PATH\"\nrm -f \"$0\"\nexec php artisan tinker\n",
                dir.display(),
                bin.display()
            );
            std::fs::write(&script, body).map_err(err)?;
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700))
                    .map_err(err)?;
            }
            std::process::Command::new("/usr/bin/open")
                .args(["-b", "com.apple.Terminal"])
                .arg(&script)
                .status()
                .is_ok_and(|s| s.success())
        }
        other => return Err(format!("unknown target {other:?}")),
    };
    if ok {
        Ok(())
    } else {
        Err(format!("no supported {target} app found"))
    }
}

fn running(app: &AppHandle) -> bool {
    app.try_state::<AppState>()
        .is_some_and(|s| s.engine.lock().unwrap().is_some())
}

fn refresh_tray(app: &AppHandle) {
    if let Some(item) = app.try_state::<MenuItem<tauri::Wry>>() {
        let _ = item.set_text(if running(app) {
            "Stop Corral"
        } else {
            "Start Corral"
        });
    }
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Corral", true, None::<&str>)?;
    let toggle = MenuItem::with_id(app, "toggle", "Start Corral", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Corral", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, &toggle, &sep, &quit])?;
    app.manage(toggle.clone());
    let mut builder = TrayIconBuilder::with_id("corral")
        .tooltip("Corral")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main(app),
            "toggle" => {
                let state = app.state::<AppState>();
                if running(app) {
                    stop_engine_inner(app, &state);
                } else if let Err(e) = start_engine_inner(app, &state) {
                    tracing::warn!(error = %e, "start from tray failed");
                    show_main(app);
                }
            }
            "quit" => {
                let state = app.state::<AppState>();
                stop_engine_inner(app, &state);
                // Give the engine a moment to stop children cleanly.
                let app = app.clone();
                std::thread::spawn(move || {
                    let paths = app.state::<AppState>().paths.clone();
                    for _ in 0..120 {
                        if !orphans::engine_running(&paths) {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(250));
                    }
                    app.exit(0);
                });
            }
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone()).icon_as_template(true);
    }
    builder.build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("CORRAL_LOG")
                .unwrap_or_else(|_| "info".into()),
        )
        .init();
    let paths = Paths::discover().expect("resolve data directory");
    paths.ensure().expect("create data directory");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--background"]),
        ))
        .manage(AppState {
            paths,
            engine: Mutex::new(None),
        })
        .setup(|app| {
            build_tray(app)?;
            // Launched at login: start serving and stay in the menu bar.
            if std::env::args().any(|a| a == "--background") {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.hide();
                }
                let handle = app.handle().clone();
                let state = app.state::<AppState>();
                if let Err(e) = start_engine_inner(&handle, &state) {
                    tracing::warn!(error = %e, "autostart: engine not started");
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window keeps Corral running in the menu bar.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            overview,
            park,
            unpark,
            link,
            unlink,
            set_site_php,
            set_site_secure,
            set_default_php,
            start_engine,
            stop_engine,
            install_component,
            set_service_enabled,
            set_shell_path,
            set_autostart,
            open_in,
            read_env,
            write_env,
            read_php_ini,
            write_php_ini,
            node_install,
            node_use,
            new_project,
            mail_list,
            mail_detail,
            mail_delete,
            mail_clear,
            mail_save_attachment,
            dumps_list,
            dumps_clear,
            events_list,
            events_clear,
            site_logs
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::Reopen { .. } = event {
                show_main(app);
            }
        });
}
