<script module lang="ts">
  import { DotLottie } from "@lottiefiles/dotlottie-web";
  // Self-host the WASM (no CDN) — the build copies it into /assets. Set once.
  DotLottie.setWasmUrl("/assets/dotlottie-player.wasm");
</script>

<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { type StickerDef, stickerUrl, formatOf } from "./stickers";

  export let def: StickerDef;
  export let size = 128;
  export let autoplay = true;
  export let loop = true;

  $: fmt = formatOf(def.file);
  $: url = stickerUrl(def);

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
  <img src={url} alt={def.emoji} style="width:{size}px;height:{size}px;object-fit:contain" />
{/if}
