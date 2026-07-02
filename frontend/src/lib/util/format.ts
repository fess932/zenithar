// Small pure formatters shared across components. Framework-free → unit-tested
// in format.test.ts (runs under `bun test`, no browser).

/// Human-readable file size: "512 B", "3 KB", "1.4 MB".
export function fmtSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/// Byte size across the full range (B → TB), for admin totals that can reach
/// gigabytes. One decimal from MB up; whole numbers for B/KB. `fmtSize` caps at
/// MB and stays for per-file labels; this is the dashboard variant.
export function fmtBytes(bytes: number): string {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let n = Math.max(0, bytes);
  let u = 0;
  while (n >= 1024 && u < units.length - 1) {
    n /= 1024;
    u++;
  }
  const s = u <= 1 ? String(Math.round(n)) : n.toFixed(1);
  return `${s} ${units[u]}`;
}

/// Duration as "m:ss" from whole seconds (voice-note / call timer).
export function fmtDur(s: number): string {
  const m = Math.floor(s / 60);
  const sec = Math.floor(s % 60);
  return `${m}:${String(sec).padStart(2, "0")}`;
}
