<script lang="ts">
  import { type ChatMessage, replyingTo, highlightId, flashMessage } from "./chat";
  import { me } from "./session";
  import { t } from "./i18n";
  import VoicePlayer from "./VoicePlayer.svelte";
  import { openLightbox } from "./lightbox";

  export let m: ChatMessage;

  $: mine = $me?.id === m.author_id;

  // Quoted-reply snippet: the parent's text, or a marker for attachment-only.
  $: replyText = m.reply_to
    ? m.reply_to.body.trim() || (m.reply_to.has_attachment ? $t("attachment") : "")
    : "";

  function startReply(): void {
    replyingTo.set(m);
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

<div class="line arrived group relative" class:mine class:flash={$highlightId === m.id} data-mid={m.id}>
  <span class="time">{fmtTime(m.created_at)}</span>
  <span class="who">{mine ? $t("you") : m.author_name}</span>
  <div class="body">
    {#if m.reply_to}
      <button
        type="button"
        onclick={jumpToReply}
        class="mb-1 flex w-full max-w-md flex-col items-start overflow-hidden rounded border-l-2 border-beacon bg-surface-2/60 px-2 py-1 text-left transition-colors hover:bg-surface-2"
      >
        <span class="font-mono text-[0.72rem] text-beacon">{m.reply_to.author_name}</span>
        <span class="line-clamp-1 text-[0.8rem] text-muted">
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

  <!-- Reply action: appears on hover (desktop) / dimmed-always (touch). -->
  <button
    type="button"
    onclick={startReply}
    aria-label={$t("reply")}
    title={$t("reply")}
    class="absolute right-2 top-1 grid size-7 place-items-center rounded border border-line bg-surface text-muted opacity-0 transition hover:text-beacon focus-visible:opacity-100 group-hover:opacity-100 [@media(hover:none)]:opacity-60"
  >
    ↩
  </button>
</div>
