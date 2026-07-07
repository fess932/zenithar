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
  export let size = 128;
  export let autoplay = true;
  export let loop = true;

  $: fmt = format ?? (def ? formatOf(def.file) : "webp");
  $: url = src ?? (def ? stickerUrl(def) : "");
  $: label = alt || def?.emoji || "";

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
    }
  });
  onDestroy(() => player?.destroy());
</script>

{#if fmt === "lottie"}
  <canvas bind:this={canvas} style="width:{size}px;height:{size}px"></canvas>
{:else if fmt === "webm"}
  <!-- svelte-ignore a11y_media_has_caption -->
  <video
    src={url}
    {autoplay}
    {loop}
    muted
    playsinline
    style="width:{size}px;height:{size}px;object-fit:contain"
  ></video>
{:else}
  <img src={url} alt={label} style="width:{size}px;height:{size}px;object-fit:contain" />
{/if}
