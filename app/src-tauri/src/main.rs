// Zenithar desktop shell.
//
// A thin native window onto the self-hosted Zenithar server: the web app runs
// INSIDE it, unchanged, loading the server's own URL (see `app.windows[].url` in
// tauri.conf.json) — so it stays same-origin and never depends on Tauri.
//
// Deep links: a `zenithar://…` URL opens/focuses the app and logs it in. The web
// page (already signed in) mints a one-time link-token via `/api/me/app-link` and
// hands us `zenithar://i/<token>`; we just navigate the webview to the server's
// existing `/i/<token>` login, which sets the session cookie. No backend-auth
// changes, no token kept by us.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::{Manager, Url};
use tauri_plugin_deep_link::DeepLinkExt;

/// One build = one server (kept in sync with the window url in tauri.conf.json).
const SERVER: &str = "https://chat.re-star.ru";

/// Turn incoming `zenithar://…` URLs into a server login: pull the link-token
/// (`?token=…` or the last path segment) and point the webview at `/i/<token>`.
fn open_token_urls(app: &tauri::AppHandle, urls: Vec<Url>) {
    let Some(win) = app.get_webview_window("main") else {
        return;
    };
    let _ = win.set_focus();
    for u in urls {
        let token = u
            .query_pairs()
            .find(|(k, _)| k == "token")
            .map(|(_, v)| v.into_owned())
            .or_else(|| {
                u.path_segments()
                    .and_then(|mut s| s.next_back())
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
            });
        let Some(token) = token else { continue };
        // token is server-minted (URL-safe); guard against anything weird anyway.
        if !token.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            continue;
        }
        if let Ok(target) = format!("{SERVER}/i/{token}").parse::<Url>() {
            let _ = win.navigate(target);
        }
    }
}

fn main() {
    tauri::Builder::default()
        // Win/Linux: a 2nd launch carrying the deep link focuses the running app
        // (the `deep-link` feature forwards the URL into on_open_url below).
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.set_focus();
            }
        }))
        .plugin(tauri_plugin_deep_link::init())
        .setup(|app| {
            // Register the scheme at runtime for dev on Win/Linux (bundled apps get
            // it from the installer / Info.plist via tauri.conf.json).
            #[cfg(any(windows, target_os = "linux"))]
            let _ = app.deep_link().register_all();

            let handle = app.handle().clone();
            app.deep_link()
                .on_open_url(move |event| open_token_urls(&handle, event.urls()));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running the Zenithar desktop app");
}
