<script lang="ts">
  import { onMount } from "svelte";
  import { fade, fly, scale } from "svelte/transition";
  import { quintOut } from "svelte/easing";
  import { lightbox, closeLightbox, step } from "./lightbox";
  import { saveFromMessage } from "./saved";
  import { addPackBySlug } from "./packs";
  import { t } from "./i18n";

  // Save the shown item into your collection ("сохранёнки"). For a sticker that
  // belongs to a pack, this adds the WHOLE pack instead of the single sticker.
  let saving = false;
  let savedOk = false;
  async function save(): Promise<void> {
    if (!current || saving) return;
    saving = true;
    savedOk = current.packSlug
      ? !!(await addPackBySlug(current.packSlug))
      : await saveFromMessage(current.id);
    saving = false;
    if (savedOk) setTimeout(() => (savedOk = false), 1500);
  }

  // Zoom & pan for the shown image. Fit-to-screen at scale 1; pinch (two fingers)
  // or the wheel zooms in, a drag pans when zoomed, double-tap toggles. Reset when
  // the image changes. `tx`/`ty` are pixel offsets, applied via a CSS transform.
  let zoom = 1;
  let tx = 0;
  let ty = 0;
  let gesturing = false; // true mid-drag/pinch → drop the transform transition
  let imgEl: HTMLImageElement | undefined;
  const MAX_ZOOM = 5;
  const MIN_ZOOM = 0.4; // allow zooming OUT below fit so a full-screen photo can be pulled back

  const pointers = new Map<number, { x: number; y: number }>();
  let startDist = 0;
  let startZoom = 1;
  let panStart: { x: number; y: number; tx: number; ty: number } | null = null;

  function resetZoom(): void {
    zoom = 1;
    tx = 0;
    ty = 0;
  }

  // Keep the image roughly in view: at `zoom` it grows by (dim*(zoom-1)), so half
  // that brings each edge to the frame. Add a 20% overscroll margin so content
  // right at the edge — e.g. the top, tucked under the toolbar — can be pulled
  // clear instead of jamming against the boundary.
  function clampPan(): void {
    if (!imgEl) return;
    // Half the extra size when zoomed in, plus a constant margin so you can always
    // drag the image around a bit (even at/below fit — that's the "move" gesture).
    const margin = 0.35;
    const maxX = Math.max(0, (imgEl.clientWidth * (zoom - 1)) / 2) + imgEl.clientWidth * margin;
    const maxY = Math.max(0, (imgEl.clientHeight * (zoom - 1)) / 2) + imgEl.clientHeight * margin;
    tx = Math.min(maxX, Math.max(-maxX, tx));
    ty = Math.min(maxY, Math.max(-maxY, ty));
  }

  function onImgPointerDown(e: PointerEvent): void {
    (e.currentTarget as Element).setPointerCapture?.(e.pointerId);
    pointers.set(e.pointerId, { x: e.clientX, y: e.clientY });
    if (pointers.size === 2) {
      const [a, b] = [...pointers.values()];
      startDist = Math.hypot(a.x - b.x, a.y - b.y);
      startZoom = zoom;
      panStart = null;
      gesturing = true;
    } else if (pointers.size === 1 && (zoom > 1 || e.pointerType === "mouse")) {
      // Drag to move: always with the mouse (touch keeps single-finger swipe for
      // paging when not zoomed in).
      panStart = { x: e.clientX, y: e.clientY, tx, ty };
      gesturing = true;
    }
  }

  function onImgPointerMove(e: PointerEvent): void {
    if (!pointers.has(e.pointerId)) return;
    pointers.set(e.pointerId, { x: e.clientX, y: e.clientY });
    if (pointers.size === 2 && startDist > 0) {
      const [a, b] = [...pointers.values()];
      const dist = Math.hypot(a.x - b.x, a.y - b.y);
      zoom = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, startZoom * (dist / startDist)));
      clampPan();
    } else if (panStart) {
      tx = panStart.tx + (e.clientX - panStart.x);
      ty = panStart.ty + (e.clientY - panStart.y);
      clampPan();
    }
  }

  function onImgPointerUp(e: PointerEvent): void {
    pointers.delete(e.pointerId);
    if (pointers.size < 2) startDist = 0;
    if (pointers.size === 0) {
      panStart = null;
      gesturing = false;
    }
    // Snap the SCALE back to a clean 1× when we're right at it, but keep the
    // panned position — don't undo a deliberate zoom-out or a drag.
    if (Math.abs(zoom - 1) < 0.04) {
      zoom = 1;
      clampPan();
    }
  }

  // Desktop wheel zoom, centered.
  function onWheel(e: WheelEvent): void {
    e.preventDefault();
    zoom = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, zoom * (e.deltaY < 0 ? 1.15 : 1 / 1.15)));
    if (Math.abs(zoom - 1) < 0.02) zoom = 1;
    clampPan();
  }

  // Double-tap / double-click toggles between fit and a 2.5× look.
  function onDblClick(): void {
    if (zoom !== 1) resetZoom();
    else zoom = 2.5;
  }

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
  $: state?.index, resetZoom();

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

  // Horizontal swipe on touch to page through the gallery. Ignore mostly-vertical
  // swipes and any gesture on a zoomed image (that's a pan, not a page turn).
  let touchX: number | null = null;
  let touchY = 0;
  function onTouchStart(e: TouchEvent): void {
    touchX = e.changedTouches[0]?.clientX ?? null;
    touchY = e.changedTouches[0]?.clientY ?? 0;
  }
  function onTouchEnd(e: TouchEvent): void {
    if (touchX === null || zoom > 1) return; // panning a zoomed image ≠ paging
    const dx = (e.changedTouches[0]?.clientX ?? touchX) - touchX;
    const dy = (e.changedTouches[0]?.clientY ?? touchY) - touchY;
    if (Math.abs(dx) > 45 && Math.abs(dx) > Math.abs(dy) * 1.5) go(dx < 0 ? 1 : -1);
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
          aria-label={current.packSlug ? $t("addPack") : $t("saveImage")}
          title={current.packSlug ? $t("addPack") : $t("saveImage")}
          class="grid size-9 shrink-0 cursor-pointer place-items-center rounded-md border text-lg disabled:opacity-50 {savedOk
            ? 'border-emerald-400/60 text-emerald-400'
            : 'border-white/20 text-white/80 hover:border-white/60 hover:text-white'}"
        >
          {savedOk ? "✓" : current.packSlug ? "＋" : "🔖"}
        </button>
      {/if}
      <a
        href={current.download ?? current.src}
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
      class="flex min-h-0 flex-1 items-center justify-center overflow-hidden p-4 sm:p-8"
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
          <!-- Pinch / wheel to zoom, drag to pan, double-tap to toggle. touch-none
               hands raw gestures to our pointer handlers instead of the browser. -->
          <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
          <div
            role="img"
            aria-label={current.alt}
            in:fly={{ x: ms(dir * 40), duration: ms(220), easing: quintOut }}
            onpointerdown={onImgPointerDown}
            onpointermove={onImgPointerMove}
            onpointerup={onImgPointerUp}
            onpointercancel={onImgPointerUp}
            onwheel={onWheel}
            ondblclick={onDblClick}
            class="pointer-events-auto flex size-full touch-none items-center justify-center {gesturing
              ? 'cursor-grabbing'
              : 'cursor-grab'}"
          >
            <img
              bind:this={imgEl}
              src={current.src}
              alt={current.alt}
              draggable="false"
              style="transform: translate3d({tx}px, {ty}px, 0) scale({zoom}); transition: {gesturing
                ? 'none'
                : 'transform 0.15s ease-out'};"
              class="max-h-full max-w-full select-none rounded-md object-contain shadow-2xl"
              class:checkerboard={current.transparent}
            />
          </div>
        {/if}
      {/key}
    </div>

    {#if many}
      <!-- Prev / next: faint and unobtrusive (you can also swipe). A soft shadow
           keeps the glyph legible over light images without a solid button. -->
      <button
        type="button"
        onclick={() => go(-1)}
        aria-label={$t("prevImage")}
        title={$t("prevImage")}
        class="pointer-events-auto absolute left-1 top-1/2 grid size-9 -translate-y-1/2 place-items-center rounded-full text-3xl leading-none text-white/30 drop-shadow-[0_1px_3px_rgba(0,0,0,0.7)] transition hover:text-white/90 sm:left-3"
      >
        ‹
      </button>
      <button
        type="button"
        onclick={() => go(1)}
        aria-label={$t("nextImage")}
        title={$t("nextImage")}
        class="pointer-events-auto absolute right-1 top-1/2 grid size-9 -translate-y-1/2 place-items-center rounded-full text-3xl leading-none text-white/30 drop-shadow-[0_1px_3px_rgba(0,0,0,0.7)] transition hover:text-white/90 sm:right-3"
      >
        ›
      </button>
    {/if}
  </div>
{/if}
