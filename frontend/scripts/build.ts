// Frontend build (Svelte + Tailwind) for bun. `--watch` rebuilds on change.
//   bun run scripts/build.ts          one-shot, minified
//   bun run scripts/build.ts --watch  rebuild src/ on change
import { $ } from "bun";
import { SveltePlugin } from "bun-plugin-svelte";

const watch = process.argv.includes("--watch");

async function buildJs(): Promise<void> {
  const r = await Bun.build({
    entrypoints: ["src/main.ts"],
    outdir: "dist",
    target: "browser",
    minify: !watch,
    plugins: [SveltePlugin({ development: watch })],
  });
  if (!r.success) {
    for (const log of r.logs) console.error(log);
    if (!watch) process.exit(1);
  }
}

// Tailwind (separate CSS pipeline) + static HTML.
const buildCss = () => $`bun run css`.quiet();
const copyHtml = () => Bun.write("dist/index.html", Bun.file("src/index.html"));

// Static assets (icons, manifest) copied verbatim into dist/assets/ — served by
// the backend's rust-embed at /assets/*.
const ASSETS = [
  "favicon.svg",
  "icon.svg",
  "manifest.webmanifest",
  "apple-touch-icon.png",
  "icon-192.png",
  "icon-512.png",
];
const copyStatic = () =>
  Promise.all(
    ASSETS.map((f) => Bun.write(`dist/assets/${f}`, Bun.file(`src/assets/${f}`))),
  );

async function buildAll(): Promise<void> {
  await Promise.all([buildJs(), buildCss(), copyHtml(), copyStatic()]);
  console.log(`built ${new Date().toLocaleTimeString()}`);
}

await buildAll();

if (watch) {
  const { watch: fsWatch } = await import("node:fs");
  let timer: ReturnType<typeof setTimeout> | undefined;
  fsWatch("src", { recursive: true }, () => {
    clearTimeout(timer);
    timer = setTimeout(buildAll, 80);
  });
  console.log("watching src/ — Ctrl-C to stop");
}
