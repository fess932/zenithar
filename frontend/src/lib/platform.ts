// Runtime platform seam. The web build must NEVER import the Tauri API — it only
// *sniffs* a global the Tauri webview injects into its window. In a plain browser
// none of these exist, so `isApp` is false and the web app behaves exactly as
// before. This is the non-invasive detection point referenced in docs/ideas.md.
export const isApp: boolean =
  typeof window !== "undefined" &&
  // Tauri v2 injects __TAURI_INTERNALS__; __TAURI__ appears with globalTauri on.
  ("__TAURI_INTERNALS__" in window || "__TAURI__" in window);

// The Tauri webview swallows plain <a target="_blank"> clicks (Android does
// nothing, desktop may not reach the system browser), so external links are
// routed through the opener plugin instead. We reach it via the internals-invoke
// global the webview injects — no static @tauri-apps import, so the web build
// stays Tauri-free. In a plain browser this just falls back to window.open.
type Invoke = (cmd: string, args?: unknown) => Promise<unknown>;

export function openExternal(url: string): void {
  if (isApp) {
    const invoke = (window as unknown as { __TAURI_INTERNALS__?: { invoke?: Invoke } })
      .__TAURI_INTERNALS__?.invoke;
    if (invoke) {
      // plugin:opener|open_url → system browser. Fall back if the plugin is absent.
      void invoke("plugin:opener|open_url", { url }).catch(() => {
        window.open(url, "_blank", "noopener,noreferrer");
      });
      return;
    }
  }
  window.open(url, "_blank", "noopener,noreferrer");
}
