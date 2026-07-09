<script lang="ts">
  // A picker cell with Telegram-style interaction: a quick tap SENDS the item; a
  // press-and-hold opens a big preview (release closes it, no send). Keyboard /
  // mouse click still sends (accessible), and a hold suppresses the trailing
  // click so holding never accidentally sends.
  import {
    openStickerPreview,
    closeStickerPreview,
    type PreviewKind,
  } from "./stickerPreview";

  export let onSend: (() => void) | null = null;
  export let previewSrc: string;
  export let previewKind: PreviewKind;
  export let alt = "";
  let klass = "";
  export { klass as class };

  const HOLD_MS = 260;
  let timer: ReturnType<typeof setTimeout> | null = null;
  let held = false;

  function clearTimer(): void {
    if (timer) {
      clearTimeout(timer);
      timer = null;
    }
  }

  function down(): void {
    held = false;
    clearTimer();
    timer = setTimeout(() => {
      held = true;
      openStickerPreview({ src: previewSrc, kind: previewKind, alt, send: onSend });
    }, HOLD_MS);
  }

  function up(): void {
    // Release after a hold leaves the preview OPEN (it's a sticky modal now) so
    // you can look, then tap Send — closing on release would give no time on
    // touch. A pending (not-yet-fired) hold is just cancelled.
    clearTimer();
  }

  function cancel(): void {
    // Sliding off before the hold fires cancels the pending preview; an
    // already-open preview stays up (dismissed via the overlay itself).
    clearTimer();
  }

  function click(e: MouseEvent): void {
    // Swallow the click that follows a hold so it doesn't also send.
    if (held) {
      e.preventDefault();
      e.stopPropagation();
      held = false;
      return;
    }
    closeStickerPreview(); // dismiss any lingering preview when sending
    onSend?.();
  }
</script>

<button
  type="button"
  aria-label={alt}
  onpointerdown={down}
  onpointerup={up}
  onpointerleave={cancel}
  onpointercancel={cancel}
  onclick={click}
  oncontextmenu={(e) => e.preventDefault()}
  class={klass}
>
  <slot />
</button>
