// Split a message body into plain-text and link segments so URLs can render as
// clickable links WITHOUT {@html} (each segment is escaped by the template).
// Pure + framework-free → unit-tested in linkify.test.ts (runs under `bun test`,
// no browser). Trailing punctuation is kept out of the link ("…x.com." → the
// period is its own text part). Bare `www.` links get an https href.
export interface LinkPart {
  text: string;
  href?: string;
}

const URL_RE = /(https?:\/\/[^\s<]+|www\.[^\s<]+)/gi;

export function linkify(text: string): LinkPart[] {
  const parts: LinkPart[] = [];
  let last = 0;
  for (const match of text.matchAll(URL_RE)) {
    const i = match.index ?? 0;
    if (i > last) parts.push({ text: text.slice(last, i) });
    let url = match[0];
    const trail = url.match(/[.,!?;:)\]]+$/);
    const trailing = trail ? trail[0] : "";
    if (trailing) url = url.slice(0, -trailing.length);
    parts.push({ text: url, href: url.startsWith("www.") ? `https://${url}` : url });
    if (trailing) parts.push({ text: trailing });
    last = i + match[0].length;
  }
  if (last < text.length) parts.push({ text: text.slice(last) });
  return parts;
}
