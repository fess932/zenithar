// Zenithar shell — shared entry for desktop (main.rs) and mobile (the
// `mobile_entry_point` below, which Android/iOS call into).
//
// A thin native window onto a self-hosted Zenithar server: the web app runs
// INSIDE it, unchanged, on the server's own origin — so it stays same-origin and
// never depends on Tauri. NO host is hard-coded: the window opens a bundled
// landing page (`../landing`). The landing owns where to go (it remembers the last
// host in its own localStorage); Rust just navigates on command and hands it any
// pending deep-link login. So Rust persists nothing to disk.
//
// Deep links: `zenithar://login?u=<full https URL to a host's /i/<token>>`. The web
// page (already signed in, on whatever host) mints a one-time link via
// `/api/me/app-link` and builds the deep link from its OWN origin — so one app
// serves several hosts. For the plugin to deliver `zenithar://` on mobile, the
// scheme must be under `plugins.deep-link.mobile` in tauri.conf.json.
//
// Login persistence: MainActivity (patch-android.sh) saves the host's cookie to
// SharedPreferences when leaving the foreground and restores it on the next launch.

use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{Manager, Url};
use tauri_plugin_deep_link::DeepLinkExt;

/// Logs to logcat on Android (tag `zenithar`), stderr on desktop. On Honor/MagicOS
/// enable app logs via `*#*#2846579#*#*`.
macro_rules! zlog {
    ($($arg:tt)*) => {{
        #[cfg(target_os = "android")]
        log::info!($($arg)*);
        #[cfg(not(target_os = "android"))]
        eprintln!($($arg)*);
    }};
}

/// True once the landing has loaded and asked for a pending login — i.e. the app is
/// up. A deep link before this is stashed in PENDING (the landing picks it up);
/// after, it navigates directly (warm).
static READY: AtomicBool = AtomicBool::new(false);
/// A cold-start deep-link login URL, waiting for the landing to consume it.
static PENDING: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
/// Last URL we (or the landing) opened, so a duplicate delivery doesn't re-open a
/// one-time token.
static LAST_NAV: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Navigate the main window to `url`, on the main thread.
fn navigate(app: &tauri::AppHandle, url: Url) {
    let app = app.clone();
    std::thread::spawn(move || {
        let app2 = app.clone();
        let _ = app.run_on_main_thread(move || {
            if let Some(win) = app2.get_webview_window("main") {
                zlog!("[zenithar] navigating to {url}");
                let _ = win.navigate(url);
            }
        });
    });
}

/// Handle the login URL(s) carried by an incoming deep link.
fn open_token_urls(app: &tauri::AppHandle, urls: Vec<Url>) {
    zlog!("[zenithar] deep-link received: {urls:?}");
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.set_focus();
    }
    for u in urls {
        // `zenithar://login?u=<full https URL to a host's /i/token>`. The web page
        // builds it from its OWN origin, so one app serves several hosts.
        let Some(target) = u
            .query_pairs()
            .find(|(k, _)| k == "u")
            .map(|(_, v)| v.into_owned())
        else {
            continue;
        };
        match Url::parse(&target) {
            // Constrain to a login link (`/i/…`) over http(s) — not an arbitrary page.
            Ok(url) if matches!(url.scheme(), "https" | "http") && url.path().starts_with("/i/") => {
                if READY.load(Ordering::SeqCst) {
                    // Warm: app is up — dedup the one-time token and navigate.
                    {
                        let mut last = LAST_NAV.lock().unwrap();
                        if last.as_deref() == Some(url.as_str()) {
                            continue;
                        }
                        *last = Some(url.as_str().to_owned());
                    }
                    navigate(app, url);
                } else {
                    // Cold: stash for the landing to consume via `pending_login`.
                    *PENDING.lock().unwrap() = Some(url.to_string());
                }
            }
            _ => {}
        }
    }
}

/// The landing calls this on load: returns a cold-start deep-link login URL to open
/// (and remember), or null. Marks the app ready so later deep links navigate warm.
#[tauri::command]
fn pending_login() -> Option<String> {
    READY.store(true, Ordering::SeqCst);
    let pending = PENDING.lock().unwrap().take();
    if let Some(ref url) = pending {
        // So a duplicate warm delivery of the same token is skipped.
        *LAST_NAV.lock().unwrap() = Some(url.clone());
    }
    pending
}

/// Navigate the webview to `url` (the landing's reopen / manual-entry / deep-link).
#[tauri::command]
fn go(app: tauri::AppHandle, url: String) -> Result<(), String> {
    let u = Url::parse(&url).map_err(|_| "bad url".to_string())?;
    if !matches!(u.scheme(), "https" | "http") {
        return Err("scheme must be http(s)".into());
    }
    navigate(&app, u);
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(target_os = "android")]
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("zenithar"),
    );

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
        .invoke_handler(tauri::generate_handler![pending_login, go])
        .setup(|app| {
            // Register the scheme at runtime for dev on Win/Linux (bundled desktop
            // apps get it from the installer / Info.plist; Android from its manifest).
            #[cfg(any(windows, target_os = "linux"))]
            let _ = app.deep_link().register_all();

            // Warm deep links (app already running) arrive via on_open_url.
            let handle = app.handle().clone();
            app.deep_link()
                .on_open_url(move |event| open_token_urls(&handle, event.urls()));

            // Cold start: stash the launch deep link (if any) for the landing to pick
            // up via `pending_login`. The landing drives all navigation, so there's
            // no flash/race from us navigating during setup.
            if let Ok(Some(urls)) = app.deep_link().get_current() {
                open_token_urls(app.handle(), urls);
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running the Zenithar app");
}
