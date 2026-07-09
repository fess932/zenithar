<script module lang="ts">
  import { DotLottie } from "@lottiefiles/dotlottie-web";
  import { DOTLOTTIE_WASM_URL } from "./dotlottieWasm";
  // Self-host the WASM (no CDN) — the build copies it under a versioned name so it
  // can be cached immutably. Set once for all Sticker instances.
  DotLottie.setWasmUrl(DOTLOTTIE_WASM_URL);
</script>

<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { type StickerDef, type StickerFormat, stickerUrl, formatOf } from "./stickers";

  // Either a bundled sticker (`def`), or an arbitrary blob via `src` + `format`
  // (used to render pack items / saved Lottie animations).
  export let def: StickerDef | undefined = undefined;
  export let src: string | undefined = undefined;
  export let format: StickerFormat | undefined = undefined;
  export let alt = "";
  // A number is treated as px; a string is used verbatim as a CSS length, so
  // callers can pass responsive sizes like "min(62vw, 16rem)".
  export let size: number | string = 128;
  export let autoplay = true;
  export let loop = true;

  $: fmt = format ?? (def ? formatOf(def.file) : "webp");
  $: url = src ?? (def ? stickerUrl(def) : "");
  $: label = alt || def?.emoji || "";
  $: sizeCss = typeof size === "number" ? `${size}px` : size;

  // Show a shimmer placeholder until the media is actually decoded/ready, then
  // fade it in — a grid of stickers hydrates smoothly instead of popping in blank.
  let loaded = false;

  let canvas: HTMLCanvasElement | undefined;
  let player: DotLottie | undefined;

  onMount(() => {
    // Lottie → dotLottie (WASM/canvas). freezeOnOffscreen pauses scrolled-away
    // stickers so a grid of them doesn't peg the CPU. WebM/WebP use native tags.
    if (fmt === "lottie" && canvas) {
      player = new DotLottie({
        canvas,
        src: url,
        autoplay,
        loop,
        renderConfig: {
          freezeOnOffscreen: true,
          autoResize: true,
          devicePixelRatio: window.devicePixelRatio || 1,
        },
      });
      player.addEventListener("load", () => (loaded = true));
    }
  });
  onDestroy(() => player?.destroy());
</script>

<div class="relative inline-block align-middle" style="width:{sizeCss};height:{sizeCss}">
  {#if !loaded}
    <div class="skeleton absolute inset-0"></div>
  {/if}
  {#if fmt === "lottie"}
    <canvas
      bind:this={canvas}
      class="relative size-full transition-opacity duration-200"
      class:opacity-0={!loaded}
    ></canvas>
  {:else if fmt === "webm"}
    <!-- svelte-ignore a11y_media_has_caption -->
    <video
      src={url}
      {autoplay}
      {loop}
      muted
      playsinline
      onloadeddata={() => (loaded = true)}
      class="relative size-full object-contain transition-opacity duration-200"
      class:opacity-0={!loaded}
    ></video>
  {:else}
    <img
      src={url}
      alt={label}
      onload={() => (loaded = true)}
      class="relative size-full object-contain transition-opacity duration-200"
      class:opacity-0={!loaded}
    />
  {/if}
</div>
