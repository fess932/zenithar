#!/usr/bin/env bash
#
# Re-apply our Android customizations after `tauri android init` (which regenerates
# the gen/android project on every build). Run automatically by the Makefile / CI.
#
# Everything we inject lives below as plain, readable text — edit it by hand if you
# need to. It's idempotent: each build reproduces the same result.
#
# NOTE: the `zenithar://` deep-link <intent-filter> is NOT here — Tauri generates it
# itself from `plugins.deep-link.mobile` in tauri.conf.json. Nothing to patch.
#
set -e
GEN="${1:-src-tauri/gen/android}"

# --- MainActivity ------------------------------------------------------------
# We overwrite the generated MainActivity with our own. It's tiny; ours restores
# the saved session cookie before the webview loads, so reopening the app keeps you
# logged in (some Android WebViews drop the cookie when the app is killed). The
# cookie itself is saved by the Rust side at login time (lib.rs on_page_load) to
# <data>/cookies.txt, for the host in <data>/last_host.txt.
#
# Edit the Kotlin below freely. The package line is taken from the generated file
# so it follows tauri.conf's `identifier`.

MAIN_ACTIVITY="$(find "$GEN" -name MainActivity.kt -not -path '*/generated/*' | head -1)"
[ -n "$MAIN_ACTIVITY" ] || { echo "✗ MainActivity.kt not found under $GEN"; exit 1; }

PKG="$(grep -m1 '^package ' "$MAIN_ACTIVITY" | awk '{print $2}')"

cat > "$MAIN_ACTIVITY" <<'KOTLIN'
package __PKG__

import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.webkit.CookieManager
import androidx.activity.enableEdgeToEdge

// Login persistence via SharedPreferences (Android settings — no files): we keep
// the current host and its session cookie, save it when the app leaves the
// foreground, and restore it on a plain relaunch so you stay logged in (some
// WebViews drop the cookie when the process is killed).
class MainActivity : TauriActivity() {
  private fun prefs() = getSharedPreferences("zenithar", MODE_PRIVATE)

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    // Remember the host this launch logs into (from the deep link), so we can key
    // the cookie by it later.
    hostFromDeepLink(intent)?.let { prefs().edit().putString("host", it).apply() }
    // Plain relaunch → restore the saved cookie before the host loads. A deep link
    // brings its own fresh login, so skip restore there.
    if (intent?.action != Intent.ACTION_VIEW) restoreCookie()
  }

  override fun onStop() {
    super.onStop()
    saveCookie()
  }

  // zenithar://login?u=<encoded https://host/i/token>  ->  "https://host"
  private fun hostFromDeepLink(i: Intent?): String? {
    if (i?.action != Intent.ACTION_VIEW) return null
    val u = i.data?.getQueryParameter("u") ?: return null
    return try {
      Uri.parse(u).let { if (it.scheme != null && it.host != null) "${it.scheme}://${it.host}" else null }
    } catch (e: Exception) {
      null
    }
  }

  private fun restoreCookie() {
    try {
      val host = prefs().getString("host", null) ?: return
      val cookies = prefs().getString("cookie", null) ?: return
      val cm = CookieManager.getInstance()
      cm.setAcceptCookie(true)
      for (pair in cookies.split("; ")) if (pair.isNotBlank()) cm.setCookie(host, pair)
      cm.flush()
    } catch (e: Exception) {
    }
  }

  private fun saveCookie() {
    try {
      val host = prefs().getString("host", null) ?: return
      val cookies = CookieManager.getInstance().getCookie(host) ?: return
      prefs().edit().putString("cookie", cookies).apply()
    } catch (e: Exception) {
    }
  }
}
KOTLIN
perl -pi -e "s/__PKG__/$PKG/" "$MAIN_ACTIVITY"

echo "→ android: MainActivity patched (cookie persistence via SharedPreferences)"
