// Runtime platform seam. The web build must NEVER import the Tauri API — it only
// *sniffs* a global the Tauri webview injects into its window. In a plain browser
// none of these exist, so `isApp` is false and the web app behaves exactly as
// before. This is the non-invasive detection point referenced in docs/ideas.md.
export const isApp: boolean =
  typeof window !== "undefined" &&
  // Tauri v2 injects __TAURI_INTERNALS__; __TAURI__ appears with globalTauri on.
  ("__TAURI_INTERNALS__" in window || "__TAURI__" in window);
