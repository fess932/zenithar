// Zenithar shell — shared entry for desktop (main.rs) and mobile (the
// `mobile_entry_point` below, which Android/iOS call into).
//
// A thin native window onto a self-hosted Zenithar server: the web app runs
// INSIDE it, unchanged, on the server's own origin — so it stays same-origin and
// never depends on Tauri. NO host is hard-coded: the window opens a bundled
// landing page (`../landing`, pick/enter a server), and we navigate it to the
// real host from a deep link or the remembered last host.
//
// Deep links: `zenithar://login?u=<full https URL to a host's /i/<token>>` opens
// /focuses the app and logs it in. The web page (already signed in, on whatever
// host) mints a one-time link via `/api/me/app-link` and builds the deep link from
// its OWN origin — so one app serves several hosts. We just navigate the webview to
// that `/i/<token>`, which sets the session cookie. No backend-auth changes, no
// token kept by us.

use tauri::{Manager, Url};
use tauri_plugin_deep_link::DeepLinkExt;

/// Where we persist the last host the app logged into (one line, the origin).
fn host_file(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|d| d.join("last_host.txt"))
}

/// Remember the host (origin, e.g. `https://host2.example.com`) so the next plain
/// launch reopens it — multi-host: each user sticks to their own server.
fn remember_host(app: &tauri::AppHandle, origin: &str) {
    if let Some(path) = host_file(app) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, origin);
    }
}

fn last_host(app: &tauri::AppHandle) -> Option<String> {
    let s = std::fs::read_to_string(host_file(app)?).ok()?;
    let s = s.trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// Navigate the webview to the login URL carried by an incoming deep link.
fn open_token_urls(app: &tauri::AppHandle, urls: Vec<Url>) {
    // Debug: visible in `adb logcat` (Android pipes Rust stderr under tag
    // RustStdoutStderr). Shows the deep link arrived and where we navigate.
    eprintln!("[zenithar] deep-link received: {urls:?}");
    let Some(win) = app.get_webview_window("main") else {
        eprintln!("[zenithar] no main window to navigate");
        return;
    };
    let _ = win.set_focus();
    for u in urls {
        // `zenithar://login?u=<full https URL to a host's /i/token>`. The web page
        // builds it from its OWN origin, so one app serves several hosts.
        let Some(target) = u
            .query_pairs()
            .find(|(k, _)| k == "u")
            .map(|(_, v)| v.into_owned())
        else {
            eprintln!("[zenithar] no ?u= login url in {u}");
            continue;
        };
        match Url::parse(&target) {
            // Constrain to a login link (`/i/…`) over http(s) — not an arbitrary page.
            Ok(url) if matches!(url.scheme(), "https" | "http") && url.path().starts_with("/i/") => {
                let origin = url.origin().ascii_serialization();
                eprintln!("[zenithar] navigating to {url}; remembering host {origin}");
                remember_host(app, &origin);
                let _ = win.navigate(url);
            }
            _ => eprintln!("[zenithar] ignoring unsafe deep-link target: {target}"),
        }
    }
}

/// Called by the bundled landing page when the user types a server address:
/// validate, remember it, and point the webview there. (The page lacks a token,
/// so this only logs in if a session cookie for that host already exists.)
#[tauri::command]
fn open_host(app: tauri::AppHandle, url: String) -> Result<(), String> {
    let u = Url::parse(&url).map_err(|_| "bad url".to_string())?;
    if !matches!(u.scheme(), "https" | "http") {
        return Err("scheme must be http(s)".into());
    }
    remember_host(&app, &u.origin().ascii_serialization());
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.navigate(u);
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();

    // single-instance is desktop-only (Win/Linux): a 2nd launch carrying the deep
    // link focuses the running app (the `deep-link` feature forwards the URL).
    // cfg'd shadowing (not `mut`) so mobile, where this is removed, doesn't warn.
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
        if let Some(win) = app.get_webview_window("main") {
            let _ = win.set_focus();
        }
    }));

    builder
        .plugin(tauri_plugin_deep_link::init())
        .invoke_handler(tauri::generate_handler![open_host])
        .setup(|app| {
            // Register the scheme at runtime for dev on Win/Linux (bundled desktop
            // apps get it from the installer / Info.plist; Android from its manifest).
            #[cfg(any(windows, target_os = "linux"))]
            let _ = app.deep_link().register_all();

            // Links received while the app is already running (warm).
            let handle = app.handle().clone();
            app.deep_link()
                .on_open_url(move |event| open_token_urls(&handle, event.urls()));

            // Cold start: a launch deep link wins (on_open_url won't fire for it).
            // Otherwise reopen the last host we logged into — its session cookie
            // persists in the webview, so the app comes back signed in. With
            // neither, the window stays on the bundled landing (pick a server).
            match app.deep_link().get_current() {
                Ok(Some(urls)) => open_token_urls(app.handle(), urls),
                _ => {
                    if let (Some(host), Some(win)) =
                        (last_host(app.handle()), app.get_webview_window("main"))
                    {
                        if let Ok(url) = Url::parse(&host) {
                            eprintln!("[zenithar] reopening last host {host}");
                            let _ = win.navigate(url);
                        }
                    }
                }
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running the Zenithar app");
}
