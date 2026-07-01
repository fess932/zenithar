// Local date/time helpers for the chat UI. Pure given a timestamp → unit-tested
// in time.test.ts. All operate in the LOCAL timezone (that's what the UI shows).

const pad = (n: number): string => String(n).padStart(2, "0");

/// "HH:MM" for a unix-millis timestamp (local time).
export function fmtTime(ms: number): string {
  const d = new Date(ms);
  return `${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/// "HH:MM:SS" — full precision, used for the hover tooltip.
export function fullTime(ms: number): string {
  const d = new Date(ms);
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

/// Whether two timestamps fall on the same local calendar day.
export function sameDay(a: number, b: number): boolean {
  const x = new Date(a);
  const y = new Date(b);
  return (
    x.getFullYear() === y.getFullYear() &&
    x.getMonth() === y.getMonth() &&
    x.getDate() === y.getDate()
  );
}
