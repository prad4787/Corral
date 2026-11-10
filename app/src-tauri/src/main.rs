// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Run by launchd (see corral-system): forward ports 80/443 to Caddy.
    // Must be handled before Tauri starts; no window, no engine.
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--port-forward") {
        let port = |i: usize, d: u16| args.get(i).and_then(|a| a.parse().ok()).unwrap_or(d);
        if let Err(e) = corral_system::forward::main_from_launchd(port(2, 18080), port(3, 18443)) {
            eprintln!("corral port forwarder: {e:#}");
            std::process::exit(1);
        }
        return;
    }
    corral_app_lib::run()
}
