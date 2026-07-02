<script lang="ts">
  import { onMount } from "svelte";
  import { fade, fly, scale } from "svelte/transition";
  import { quintOut } from "svelte/easing";
  import { lightbox, closeLightbox, step } from "./lightbox";
  import { saveFromMessage } from "./saved";
  import { t } from "./i18n";

  // Save the shown image/video into your private collection ("сохранёнки").
  let saving = false;
  let savedOk = false;
  async function save(): Promise<void> {
    if (!current || saving) return;
    saving = true;
    savedOk = await saveFromMessage(current.id);
    saving = false;
    if (savedOk) setTimeout(() => (savedOk = false), 1500);
  }

  // Fit-to-screen by default; tap the image to view it at full size and scroll.
  let zoomed = false;

  // Honor prefers-reduced-motion: collapse all transition durations to 0.
  let reduce = false;
  onMount(() => {
    reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  });
  $: ms = (n: number): number => (reduce ? 0 : n);

  $: state = $lightbox;
  $: current = state ? state.items[state.index] : null;
  $: many = (state?.items.length ?? 0) > 1;
  // Reset zoom whenever the shown image changes.
  $: state?.index, (zoomed = false);

  // Direction of the last step, so the incoming image slides in from the
  // correct side (0 on first open = a plain fade-in).
  let dir = 0;
  function go(delta: number): void {
    dir = delta;
    step(delta);
  }

  function onKey(e: KeyboardEvent): void {
    if (!state) return;
    if (e.key === "Escape") {
      e.stopImmediatePropagation(); // don't also close a Profile viewer underneath
      closeLightbox();
    } else if (e.key === "ArrowRight") go(1);
    else if (e.key === "ArrowLeft") go(-1);
  }

  // Horizontal swipe on touch to page through the gallery.
  let touchX: number | null = null;
  function onTouchStart(e: TouchEvent): void {
    touchX = e.changedTouches[0]?.clientX ?? null;
  }
  function onTouchEnd(e: TouchEvent): void {
    if (touchX === null || zoomed) return;
    const dx = (e.changedTouches[0]?.clientX ?? touchX) - touchX;
    if (Math.abs(dx) > 50) go(dx < 0 ? 1 : -1);
    touchX = null;
  }
</script>

<svelte:window onkeydown={onKey} />

{#if state && current}
  <!-- Backdrop is its own button: clicking the empty space closes the viewer. -->
  <button
    type="button"
    aria-label={$t("dismiss")}
    onclick={closeLightbox}
    transition:fade={{ duration: ms(160) }}
    class="fixed inset-0 z-[70] cursor-zoom-out bg-black/85 backdrop-blur-sm"
  ></button>

  <!-- Content layer sits above the backdrop. It ignores pointer events so any
       gap around the controls/image falls through to the backdrop (= close);
       the bits you can actually interact with re-enable them. -->
  <div
    transition:scale={{ start: 0.97, opacity: 0, duration: ms(180), easing: quintOut }}
    class="pointer-events-none fixed inset-0 z-[70] flex flex-col"
  >
    <div
      class="pointer-events-auto flex items-center gap-3 px-4 py-3 pt-[calc(0.75rem+env(safe-area-inset-top))]"
    >
      <span class="min-w-0 flex-1 truncate font-mono text-[0.8rem] text-white/70">
        {current.filename}
      </span>
      {#if many}
        <span class="shrink-0 font-mono text-[0.8rem] tabular-nums text-white/50">
          {state.index + 1} / {state.items.length}
        </span>
      {/if}
      {#if current.saveable !== false}
        <button
          type="button"
          onclick={save}
          disabled={saving}
          aria-label={$t("saveImage")}
          title={$t("saveImage")}
          class="grid size-9 shrink-0 cursor-pointer place-items-center rounded-md border text-lg disabled:opacity-50 {savedOk
            ? 'border-emerald-400/60 text-emerald-400'
            : 'border-white/20 text-white/80 hover:border-white/60 hover:text-white'}"
        >
          {savedOk ? "✓" : "🔖"}
        </button>
      {/if}
      <a
        href={current.src}
        download={current.filename}
        aria-label={$t("download")}
        title={$t("download")}
        class="grid size-9 shrink-0 place-items-center rounded-md border border-white/20 text-lg text-white/80 hover:border-white/60 hover:text-white"
      >
        ↓
      </a>
      <button
        type="button"
        onclick={closeLightbox}
        aria-label={$t("dismiss")}
        title={$t("dismiss")}
        class="grid size-9 shrink-0 cursor-pointer place-items-center rounded-md border border-white/20 text-white/80 hover:border-white/60 hover:text-white"
      >
        ✕
      </button>
    </div>

    <div
      role="group"
      aria-label={current.filename}
      class="flex min-h-0 flex-1 items-center justify-center overflow-auto p-4 sm:p-8"
      ontouchstart={onTouchStart}
      ontouchend={onTouchEnd}
    >
      {#key state.index}
        {#if current.kind === "video"}
          <!-- svelte-ignore a11y_media_has_caption -->
          <video
            src={current.src}
            controls
            autoplay
            playsinline
            in:fly={{ x: ms(dir * 40), duration: ms(220), easing: quintOut }}
            class="pointer-events-auto max-h-full max-w-full rounded-md object-contain shadow-2xl"
          ></video>
        {:else}
          <button
            type="button"
            onclick={() => (zoomed = !zoomed)}
            aria-label={current.alt}
            in:fly={{ x: ms(dir * 40), duration: ms(220), easing: quintOut }}
            class="pointer-events-auto flex max-h-full max-w-full items-center justify-center {zoomed
              ? 'cursor-zoom-out'
              : 'cursor-zoom-in'}"
          >
            <img
              src={current.src}
              alt={current.alt}
              class="rounded-md shadow-2xl transition-transform duration-200 motion-reduce:transition-none {zoomed
                ? 'max-w-none'
                : 'max-h-full max-w-full object-contain'}"
            />
          </button>
        {/if}
      {/key}
    </div>

    {#if many}
      <!-- Prev / next, large enough to thumb on mobile, vertically centered. -->
      <button
        type="button"
        onclick={() => go(-1)}
        aria-label={$t("prevImage")}
        title={$t("prevImage")}
        class="pointer-events-auto absolute left-2 top-1/2 grid size-11 -translate-y-1/2 place-items-center rounded-full border border-white/20 bg-black/40 text-xl text-white/80 hover:border-white/60 hover:text-white sm:left-4"
      >
        ‹
      </button>
      <button
        type="button"
        onclick={() => go(1)}
        aria-label={$t("nextImage")}
        title={$t("nextImage")}
        class="pointer-events-auto absolute right-2 top-1/2 grid size-11 -translate-y-1/2 place-items-center rounded-full border border-white/20 bg-black/40 text-xl text-white/80 hover:border-white/60 hover:text-white sm:right-4"
      >
        ›
      </button>
    {/if}
  </div>
{/if}
