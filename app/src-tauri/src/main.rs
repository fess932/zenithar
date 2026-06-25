// Zenithar desktop shell.
//
// This is a thin native window onto the self-hosted Zenithar server: the web app
// runs INSIDE it, unchanged. Because the window loads the server's own URL (see
// `app.windows[].url` in tauri.conf.json), the web app stays same-origin and its
// relative `/api` + `/ws` calls reach the backend exactly as in a browser — so
// the web frontend never has to know it's running under Tauri.
//
// (Bundling the assets + a token-provisioning seam for "download → identified"
// is a later iteration; this first cut is the wrapper + the build pipeline.)
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running the Zenithar desktop app");
}
