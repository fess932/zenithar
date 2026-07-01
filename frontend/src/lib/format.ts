// Small pure formatters shared across components. Framework-free → unit-tested
// in format.test.ts (runs under `bun test`, no browser).

/// Human-readable file size: "512 B", "3 KB", "1.4 MB".
export function fmtSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/// Duration as "m:ss" from whole seconds (voice-note / call timer).
export function fmtDur(s: number): string {
  const m = Math.floor(s / 60);
  const sec = Math.floor(s % 60);
  return `${m}:${String(sec).padStart(2, "0")}`;
}
