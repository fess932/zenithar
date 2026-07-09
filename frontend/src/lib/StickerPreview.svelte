<script lang="ts">
  // The big press-and-hold preview overlay (Telegram-style). Mounted once; shows
  // whatever the store holds. Tap the backdrop / Esc to close; the Send button
  // (when the item is sendable) dispatches it and closes.
  import { scale, fade } from "svelte/transition";
  import { stickerPreview, closeStickerPreview } from "./stickerPreview";
  import Sticker from "./Sticker.svelte";
  import { t } from "./i18n";

  $: s = $stickerPreview;

  // The finger is still down when a hold opens this; its release fires a stray
  // pointerup/click over the backdrop. Stay pointer-transparent for a beat so
  // that release doesn't instantly close the preview — then arm dismissal.
  let armed = false;
  let armTimer: ReturnType<typeof setTimeout> | null = null;
  $: arm(s);
  function arm(cur: typeof s): void {
    if (armTimer) clearTimeout(armTimer);
    armed = false;
    if (cur) armTimer = setTimeout(() => (armed = true), 300);
  }

  function send(): void {
    const fn = s?.send;
    closeStickerPreview();
    fn?.();
  }

  function onKey(e: KeyboardEvent): void {
    if (e.key === "Escape") closeStickerPreview();
  }
</script>

<svelte:window onkeydown={onKey} />

{#if s}
  <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
  <div
    transition:fade={{ duration: 120 }}
    class="fixed inset-0 z-[60] grid place-items-center bg-black/70 backdrop-blur-sm {armed
      ? ''
      : 'pointer-events-none'}"
    onclick={closeStickerPreview}
  >
    <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
    <div
      transition:scale={{ start: 0.85, duration: 150 }}
      class="flex flex-col items-center gap-4"
      onclick={(e) => e.stopPropagation()}
    >
      {#if s.kind === "lottie"}
        <Sticker src={s.src} format="lottie" alt={s.alt} size="min(82vw, 30rem)" />
      {:else if s.kind === "webm"}
        <Sticker src={s.src} format="webm" alt={s.alt} size="min(82vw, 30rem)" />
      {:else if s.kind === "video"}
        <!-- svelte-ignore a11y_media_has_caption -->
        <video
          src={s.src}
          autoplay
          loop
          controls
          playsinline
          class="max-h-[78vh] max-w-[90vw] rounded-lg"
        ></video>
      {:else}
        <img
          src={s.src}
          alt={s.alt}
          class="object-contain"
          style="width:min(82vw, 30rem);height:min(82vw, 30rem)"
        />
      {/if}

      {#if s.send}
        <button
          type="button"
          onclick={send}
          class="flex items-center gap-2 rounded-full bg-beacon px-6 py-2.5 font-medium text-black shadow-lg transition hover:brightness-110 active:scale-95"
        >
          <span class="text-base leading-none">➤</span>
          {$t("send")}
        </button>
      {/if}
    </div>
  </div>
{/if}
