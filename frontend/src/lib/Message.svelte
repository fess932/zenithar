<script lang="ts">
  import { type ChatMessage, highlightId, flashMessage } from "./chat";
  import { me } from "./session";
  import { t } from "./i18n";
  import VoicePlayer from "./VoicePlayer.svelte";
  import { openLightbox } from "./lightbox";
  import { openMessageMenu } from "./messageMenu";

  export let m: ChatMessage;

  $: mine = $me?.id === m.author_id;

  // Quoted-reply snippet: the parent's text, or a marker for attachment-only.
  $: replyText = m.reply_to
    ? m.reply_to.body.trim() || (m.reply_to.has_attachment ? $t("attachment") : "")
    : "";

  // Open the context menu: long-press on touch, plain click on desktop. (Clicks
  // on interactive children — images, attachments, the quote — are left alone.)
  let pressTimer: ReturnType<typeof setTimeout> | null = null;
  let pressX = 0;
  let pressY = 0;
  let lastType = "mouse";

  function clearPress(): void {
    if (pressTimer) {
      clearTimeout(pressTimer);
      pressTimer = null;
    }
  }

  function onPointerDown(e: PointerEvent): void {
    lastType = e.pointerType;
    if (e.pointerType === "mouse") return; // desktop opens on click instead
    pressX = e.clientX;
    pressY = e.clientY;
    clearPress();
    pressTimer = setTimeout(() => {
      navigator.vibrate?.(15);
      openMessageMenu(m, pressX, pressY);
    }, 450);
  }

  function onPointerMove(e: PointerEvent): void {
    if (pressTimer && (Math.abs(e.clientX - pressX) > 10 || Math.abs(e.clientY - pressY) > 10)) {
      clearPress();
    }
  }

  function isInteractive(target: EventTarget | null): boolean {
    return target instanceof Element && target.closest("button, a") !== null;
  }

  function onRowClick(e: MouseEvent): void {
    if (lastType !== "mouse") return; // touch is handled by long-press
    if (isInteractive(e.target)) return; // let the child (image/quote/link) act
    openMessageMenu(m, e.clientX, e.clientY);
  }

  // Jump to the quoted original (if still in the loaded window) and flash it.
  function jumpToReply(): void {
    const id = m.reply_to?.id;
    if (!id) return;
    document
      .querySelector(`[data-mid="${id}"]`)
      ?.scrollIntoView({ behavior: "smooth", block: "center" });
    flashMessage(id);
  }

  function fmtTime(ms: number): string {
    const d = new Date(ms);
    const p = (n: number) => String(n).padStart(2, "0");
    return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
  }

  function fmtSize(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }

  const orig = (id: string) => `/api/attachments/${id}`;
  const thumb = (id: string) => `/api/attachments/${id}/thumb`;
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
  class="line arrived"
  class:mine
  class:flash={$highlightId === m.id}
  data-mid={m.id}
  onpointerdown={onPointerDown}
  onpointermove={onPointerMove}
  onpointerup={clearPress}
  onpointercancel={clearPress}
  onclick={onRowClick}
  oncontextmenu={(e) => e.preventDefault()}
  role="listitem"
>
  <span class="time">{fmtTime(m.created_at)}</span>
  <span class="who">{mine ? $t("you") : m.author_name}</span>
  <div class="body">
    {#if m.reply_to}
      <button
        type="button"
        onclick={jumpToReply}
        class="mb-1 flex w-full max-w-md flex-col items-start overflow-hidden rounded border-l-2 border-beacon bg-surface-2/60 px-2 py-1 text-left transition-colors hover:bg-surface-2"
      >
        <span class="max-w-full truncate font-mono text-[0.72rem] text-beacon"
          >{m.reply_to.author_name}</span
        >
        <span class="line-clamp-1 max-w-full text-[0.8rem] text-muted">
          {#if m.reply_to.has_attachment && !m.reply_to.body.trim()}📎 {/if}{replyText}
        </span>
      </button>
    {/if}

    {#if m.body}<span class="break-words">{m.body}</span>{/if}

    {#if m.attachments.length > 0}
      <div class="mt-1 flex flex-wrap items-start gap-2">
        {#each m.attachments as a (a.id)}
          {#if a.content_type.startsWith("image/")}
            <button
              type="button"
              onclick={() => openLightbox(a.id)}
              class="block cursor-zoom-in overflow-hidden rounded-md border border-line transition hover:border-beacon hover:brightness-110 active:scale-95 motion-reduce:transition-none motion-reduce:active:scale-100"
            >
              <img
                src={a.has_thumb ? thumb(a.id) : orig(a.id)}
                alt={a.filename}
                loading="lazy"
                class="max-h-48 max-w-[12rem] object-cover"
              />
            </button>
          {:else if a.content_type.startsWith("audio/")}
            <VoicePlayer src={orig(a.id)} />
          {:else}
            <a
              href={orig(a.id)}
              target="_blank"
              rel="noreferrer"
              download={a.filename}
              class="inline-flex max-w-full items-center gap-2 rounded-md border border-line bg-surface-2 px-3 py-2 hover:border-beacon"
            >
              <span class="text-base leading-none">📎</span>
              <span class="min-w-0">
                <span class="block truncate text-[0.85rem] text-text">{a.filename}</span>
                <span class="block font-mono text-[0.7rem] text-muted">{fmtSize(a.size)}</span>
              </span>
            </a>
          {/if}
        {/each}
      </div>
    {/if}
  </div>
</div>
