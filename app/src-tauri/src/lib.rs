//! Tauri shell. Kept deliberately thin: every command delegates to
//! `corral-core` / `corral-engine`, the same code the CLI uses.

use corral_core::php::{self, PhpVersion};
use corral_core::sites::{Registry, Site};
use corral_core::{Config, Paths};
use serde::Serialize;
use std::sync::Mutex;
use tauri::State;
use tokio::sync::oneshot;

/// Errors cross the IPC boundary as plain strings.
type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

struct AppState {
    paths: Paths,
    /// Present while the engine is running; sending stops it.
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
struct Overview {
    sites: Vec<SiteView>,
    php: Vec<PhpView>,
    tld: String,
    parked: Vec<String>,
    default_php: Option<String>,
    running: bool,
    caddy_installed: bool,
}

#[derive(Serialize)]
struct PhpView {
    version: String,
    is_default: bool,
}

fn overview_inner(state: &AppState) -> anyhow::Result<Overview> {
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
    Ok(Overview {
        sites,
        php,
        tld: cfg.tld.clone(),
        parked: cfg.parked.iter().map(|p| p.display().to_string()).collect(),
        default_php: cfg.default_php.clone(),
        running: state.engine.lock().unwrap().is_some(),
        caddy_installed: corral_engine::install::caddy_bin(paths).exists(),
    })
}

#[tauri::command]
fn overview(state: State<'_, AppState>) -> CmdResult<Overview> {
    overview_inner(&state).map_err(err)
}

#[tauri::command]
fn park(state: State<'_, AppState>, dir: String) -> CmdResult<Overview> {
    let mut cfg = Config::load(&state.paths).map_err(err)?;
    let dir = std::path::PathBuf::from(dir).canonicalize().map_err(err)?;
    if !cfg.parked.contains(&dir) {
        cfg.parked.push(dir);
        cfg.save(&state.paths).map_err(err)?;
    }
    overview_inner(&state).map_err(err)
}

#[tauri::command]
fn unpark(state: State<'_, AppState>, dir: String) -> CmdResult<Overview> {
    let mut cfg = Config::load(&state.paths).map_err(err)?;
    cfg.parked.retain(|p| p.display().to_string() != dir);
    cfg.save(&state.paths).map_err(err)?;
    overview_inner(&state).map_err(err)
}

#[tauri::command]
fn link(state: State<'_, AppState>, path: String, name: Option<String>) -> CmdResult<Overview> {
    let mut reg = Registry::load(&state.paths).map_err(err)?;
    reg.link(
        std::path::Path::new(&path),
        name.as_deref().filter(|n| !n.is_empty()),
    )
    .map_err(err)?;
    reg.save().map_err(err)?;
    overview_inner(&state).map_err(err)
}

#[tauri::command]
fn unlink(state: State<'_, AppState>, name: String) -> CmdResult<Overview> {
    let mut reg = Registry::load(&state.paths).map_err(err)?;
    reg.unlink(&name).map_err(err)?;
    reg.save().map_err(err)?;
    overview_inner(&state).map_err(err)
}

#[tauri::command]
fn set_site_php(
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
    overview_inner(&state).map_err(err)
}

#[tauri::command]
fn set_site_secure(state: State<'_, AppState>, name: String, secure: bool) -> CmdResult<Overview> {
    let cfg = Config::load(&state.paths).map_err(err)?;
    let mut reg = Registry::load(&state.paths).map_err(err)?;
    let value = (secure != cfg.secure_by_default).then_some(secure);
    reg.set_secure(&cfg, &name, value).map_err(err)?;
    reg.save().map_err(err)?;
    overview_inner(&state).map_err(err)
}

#[tauri::command]
fn set_default_php(state: State<'_, AppState>, version: String) -> CmdResult<Overview> {
    let v = PhpVersion::parse(&version).ok_or("invalid PHP version")?;
    let mut cfg = Config::load(&state.paths).map_err(err)?;
    cfg.default_php = Some(v.to_string());
    cfg.save(&state.paths).map_err(err)?;
    php::write_shims(&state.paths, Some(v)).map_err(err)?;
    overview_inner(&state).map_err(err)
}

#[tauri::command]
fn start_engine(state: State<'_, AppState>) -> CmdResult<Overview> {
    {
        let mut guard = state.engine.lock().unwrap();
        if guard.is_none() {
            let (tx, rx) = oneshot::channel::<()>();
            let paths = state.paths.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = corral_engine::run(paths, async {
                    let _ = rx.await;
                })
                .await
                {
                    tracing::error!(error = %e, "engine stopped with an error");
                }
            });
            *guard = Some(tx);
        }
    }
    overview_inner(&state).map_err(err)
}

#[tauri::command]
fn stop_engine(state: State<'_, AppState>) -> CmdResult<Overview> {
    if let Some(tx) = state.engine.lock().unwrap().take() {
        let _ = tx.send(());
    }
    overview_inner(&state).map_err(err)
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
        .manage(AppState {
            paths,
            engine: Mutex::new(None),
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
            stop_engine
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                let state = window.state::<AppState>();
                if let Some(tx) = state.engine.lock().unwrap().take() {
                    let _ = tx.send(());
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

use tauri::Manager as _;
