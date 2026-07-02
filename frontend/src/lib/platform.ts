// Runtime platform seam. The web build must NEVER import the Tauri API — it only
// *sniffs* a global the Tauri webview injects into its window. In a plain browser
// none of these exist, so `isApp` is false and the web app behaves exactly as
// before. This is the non-invasive detection point referenced in docs/ideas.md.
export const isApp: boolean =
  typeof window !== "undefined" &&
  // Tauri v2 injects __TAURI_INTERNALS__; __TAURI__ appears with globalTauri on.
  ("__TAURI_INTERNALS__" in window || "__TAURI__" in window);

// Opening links from inside the app can't go through Tauri IPC: the chat runs on
// the user's REMOTE https host, and WKWebView blocks Tauri's ipc:// custom
// protocol from a secure page as mixed content (invoke never reaches native). So
// instead the app NAVIGATES to a sentinel path; the native shell's on_navigation
// hook intercepts it, opens the URL in the system browser, and cancels the
// navigation. No IPC, no @tauri-apps import — the web build stays Tauri-free.
// In a plain browser (isApp false) we just use window.open as before.
function nativeBridge(path: string, params?: Record<string, string>): void {
  const q = params ? "?" + new URLSearchParams(params).toString() : "";
  // Cancelled natively before commit; if an old build lacks the hook, the server's
  // SPA fallback serves index.html (a reload) rather than breaking the page.
  window.location.href = path + q;
}

export function openExternal(url: string): void {
  if (isApp) {
    nativeBridge("/__zenithar_open__", { u: url });
    return;
  }
  window.open(url, "_blank", "noopener,noreferrer");
}
