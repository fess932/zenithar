//! Embed the build's git commit (short hash + subject) so the startup log can
//! report exactly which build is running. In CI the `.git` dir isn't in the
//! Docker build context, so the values are passed as `GIT_SHA` / `GIT_MSG` build
//! args (exposed to this script as env vars). Locally we fall back to `git`.

use std::process::Command;

fn main() {
    let sha = resolve("GIT_SHA", &["rev-parse", "--short", "HEAD"]);
    let msg = resolve("GIT_MSG", &["log", "-1", "--pretty=%s"]);
    println!("cargo:rustc-env=ZENITHAR_GIT_SHA={sha}");
    println!("cargo:rustc-env=ZENITHAR_GIT_MSG={msg}");
    // Re-run when the injected values or the local commit change.
    println!("cargo:rerun-if-env-changed=GIT_SHA");
    println!("cargo:rerun-if-env-changed=GIT_MSG");
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-changed=../.git/logs/HEAD");
}

/// Prefer the CI-injected env var; else ask local git; else "unknown".
fn resolve(var: &str, git_args: &[&str]) -> String {
    if let Ok(v) = std::env::var(var) {
        let v = v.trim();
        if !v.is_empty() {
            return clean(v);
        }
    }
    match Command::new("git").args(git_args).output() {
        Ok(o) if o.status.success() => clean(&String::from_utf8_lossy(&o.stdout)),
        _ => "unknown".to_string(),
    }
}

/// Keep it to a single, env-safe line.
fn clean(s: &str) -> String {
    s.trim().replace(['\n', '\r'], " ")
}
