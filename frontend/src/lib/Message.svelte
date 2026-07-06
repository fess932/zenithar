<script lang="ts">
  import { type ChatMessage, highlightId, flashMessage, toggleReaction, readAt } from "./chat";
  import { me } from "./session";
  import { t } from "./i18n";
  import VoicePlayer from "./VoicePlayer.svelte";
  import Avatar from "./Avatar.svelte";
  import Sticker from "./Sticker.svelte";
  import { sticker } from "./stickers";
  import { openLightbox } from "./lightbox";
  import { openMessageMenu } from "./messageMenu";
  import { openProfile } from "./profile";
  import { linkify } from "./util/linkify";
  import { openExternal } from "./platform";
  import { fmtSize } from "./util/format";
  import { fmtTime, fullTime } from "./util/time";

  export let m: ChatMessage;

  // A sticker message renders the bundled animation instead of a text bubble.
  $: stickerDef = m.sticker ? sticker(m.sticker) : undefined;
  // First message of an author's run: only then do we print the name + add the
  // group gap. Continuation lines tuck under it. Computed by the parent loop.
  export let firstInGroup = true;

  $: mine = $me?.id === m.author_id;
  // Read receipt for your own messages: ✓✓ once someone else has read past this,
  // else ✓ (delivered — a rendered message is already stored server-side).
  $: readByOthers = mine && ($readAt[m.room_id] ?? 0) >= m.created_at;

  // Quoted-reply snippet: the parent's text, or a marker for attachment-only.
  $: replyText = m.reply_to
    ? m.reply_to.body.trim() || (m.reply_to.has_attachment ? $t("attachment") : "")
    : "";

  // Open the context menu (reactions + actions): a plain TAP on touch, a
  // RIGHT-click on desktop. Taps/clicks on interactive children — images, the
  // quote, links — are left to act on their own.
  let lastType = "mouse";

  function onPointerDown(e: PointerEvent): void {
    lastType = e.pointerType; // remember touch vs mouse for the handlers below
  }

  function isInteractive(target: EventTarget | null): boolean {
    return target instanceof Element && target.closest("button, a") !== null;
  }

  // Touch: a normal tap opens the menu. Desktop ignores left-click (uses
  // right-click below).
  function onTap(e: MouseEvent): void {
    if (lastType === "mouse") return;
    if (isInteractive(e.target)) return;
    openMessageMenu(m, e.clientX, e.clientY);
  }

  // Desktop right-click opens the menu; suppress the native menu on the bubble.
  // (On touch a long-press also fires this — just swallow it; the tap handles it.)
  function onContextMenu(e: MouseEvent): void {
    if (isInteractive(e.target)) return; // leave the native menu on media/links
    e.preventDefault();
    if (lastType === "mouse") openMessageMenu(m, e.clientX, e.clientY);
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


  const orig = (id: string) => `/api/attachments/${id}`;
  const thumb = (id: string) => `/api/attachments/${id}/thumb`;
</script>

<!-- The message bubble itself; shared by the "mine" (right, no avatar) and the
     "others" (left, avatar gutter) layouts below. -->
{#snippet bubbleBlock()}
  <div
    class="bubble arrived"
    class:mine
    class:cont={!firstInGroup}
    class:flash={$highlightId === m.id}
  >
    {#if m.reply_to}
      <button
        type="button"
        onclick={jumpToReply}
        class="mb-1 flex w-full flex-col items-start overflow-hidden rounded-md border-l-2 border-beacon bg-black/15 px-2 py-1 text-left transition-colors hover:bg-black/25"
      >
        <span class="max-w-full truncate font-mono text-[0.72rem] text-beacon"
          >{m.reply_to.author_name}</span
        >
        <span class="line-clamp-1 max-w-full text-[0.8rem] text-muted">
          {#if m.reply_to.has_attachment && !m.reply_to.body.trim()}📎 {/if}{replyText}
        </span>
      </button>
    {/if}

    {#if m.body}<span class="whitespace-pre-wrap break-words"
        >{#each linkify(m.body) as part}{#if part.href}<a
              href={part.href}
              target="_blank"
              rel="noopener noreferrer"
              class="text-beacon underline underline-offset-2 hover:brightness-110"
              onclick={(e) => {
                // The app webview swallows target=_blank; hand the URL to the
                // system browser. stopPropagation so tapping a link doesn't also
                // trigger the row's context-menu handler.
                e.preventDefault();
                e.stopPropagation();
                openExternal(part.href!);
              }}
              >{part.text}</a
            >{:else}{part.text}{/if}{/each}</span
      >{/if}
    {#if m.edited_at}<span
        class="ml-1 align-baseline text-[0.7rem] text-muted"
        title={$t("edited")}>({$t("edited")})</span
      >{/if}

    {#if m.attachments.length > 0}
      <div class="mt-1 flex flex-wrap items-start gap-2">
        {#each m.attachments as a (a.id)}
          {#if a.content_type.startsWith("image/")}
            <!-- Transparent images (stickers) render frameless — no border or
                 surface behind them — and uncropped, so their shape reads true. -->
            <button
              type="button"
              onclick={() => openLightbox(a.id)}
              class="block cursor-zoom-in transition hover:brightness-110 active:scale-95 motion-reduce:transition-none motion-reduce:active:scale-100 {a.has_alpha
                ? ''
                : 'overflow-hidden rounded border border-line hover:border-beacon'}"
            >
              <img
                src={a.has_thumb ? thumb(a.id) : orig(a.id)}
                alt={a.filename}
                loading="lazy"
                class="max-h-44 max-w-[10rem] {a.has_alpha ? 'object-contain' : 'object-cover'}"
              />
            </button>
          {:else if a.content_type.startsWith("video/")}
            <button
              type="button"
              onclick={() => openLightbox(a.id)}
              class="group relative block cursor-pointer overflow-hidden rounded border border-line transition hover:border-beacon active:scale-95 motion-reduce:transition-none motion-reduce:active:scale-100"
            >
              <!-- First frame as the poster; the lightbox handles playback. -->
              <!-- svelte-ignore a11y_media_has_caption -->
              <video
                src={`${orig(a.id)}#t=0.1`}
                preload="metadata"
                muted
                playsinline
                tabindex="-1"
                class="max-h-44 max-w-[10rem] bg-black object-cover"
              ></video>
              <span
                class="pointer-events-none absolute inset-0 grid place-items-center transition group-hover:brightness-110"
              >
                <span
                  class="grid size-11 place-items-center rounded-full bg-black/55 text-lg text-white backdrop-blur-sm"
                  >▶</span
                >
              </span>
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

    <!-- Trailing time: the ghost reserves inline space on the last line so the
         absolutely-pinned, visible time never overlaps the text. -->
    <span class="timeghost" aria-hidden="true">{fmtTime(m.created_at)}{#if mine} ✓✓{/if}</span>
    <span class="time" title={fullTime(m.created_at)}
      >{fmtTime(m.created_at)}{#if mine}<span
          class="ml-0.5 {readByOthers ? 'text-sky-400' : ''}">{readByOthers ? "✓✓" : "✓"}</span
        >{/if}</span
    >
  </div>
{/snippet}

<!-- Sticker message: the bundled animation, no bubble chrome, with a small time. -->
{#snippet stickerBlock()}
  {#if stickerDef}
    <Sticker def={stickerDef} size={128} />
  {:else}
    <span class="text-5xl leading-none" title={m.sticker}>🖼️</span>
  {/if}
  <span class="mt-0.5 font-mono text-[0.6rem] text-muted" title={fullTime(m.created_at)}>
    {fmtTime(m.created_at)}{#if mine}<span class="ml-0.5 {readByOthers ? 'text-sky-400' : ''}"
        >{readByOthers ? "✓✓" : "✓"}</span
      >{/if}
  </span>
{/snippet}

<!-- Reaction chips below the bubble; tap to toggle your own. -->
{#snippet reactionChips()}
  {#if m.reactions.length > 0}
    <div class="mt-1 flex flex-wrap gap-1" class:justify-end={mine}>
      {#each m.reactions as r (r.emoji)}
        {@const mineR = r.by.some((x) => x.id === $me?.id)}
        <button
          type="button"
          onclick={() => toggleReaction(m.id, r.emoji)}
          aria-pressed={mineR}
          class="flex items-center gap-1 rounded-full border py-0.5 pl-1.5 pr-1.5 leading-none transition-colors {mineR
            ? 'border-beacon bg-beacon/15'
            : 'border-line bg-surface-2 hover:border-beacon/60'}"
        >
          <span class="text-[0.9rem] leading-none">{r.emoji}</span>
          <span class="flex -space-x-1.5">
            {#each r.by.slice(0, 3) as reactor (reactor.id)}
              <span class="rounded-full ring-2 ring-surface-2">
                <Avatar id={reactor.id} name="" avatar={reactor.avatar} size={16} />
              </span>
            {/each}
          </span>
          {#if r.by.length > 3}<span class="font-mono text-[0.68rem] text-muted">+{r.by.length - 3}</span>{/if}
        </button>
      {/each}
    </div>
  {/if}
{/snippet}

{#if mine}
  <!-- Your own line: right-aligned, no avatar (it's you). The whole ROW opens the
       menu on tap/right-click (so you can react to a photo by tapping beside it);
       taps on the photo/quote/links still act on their own via isInteractive. -->
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions a11y_click_events_have_key_events -->
  <div
    class="flex flex-col items-end px-2 sm:px-3"
    class:mt-3={firstInGroup}
    class:mt-[0.15rem]={!firstInGroup}
    data-mid={m.id}
    role="listitem"
    onpointerdown={onPointerDown}
    onclick={onTap}
    oncontextmenu={onContextMenu}
  >
    {#if m.sticker}{@render stickerBlock()}{:else}{@render bubbleBlock()}{/if}
    {@render reactionChips()}
  </div>
{:else}
  <!-- Someone else: avatar gutter on the left, name once atop the group. Whole row
       opens the menu (tap beside the bubble); the avatar/quote/media still act. -->
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions a11y_click_events_have_key_events -->
  <div
    class="flex items-start gap-2 px-2 sm:px-3"
    class:mt-3={firstInGroup}
    class:mt-[0.15rem]={!firstInGroup}
    data-mid={m.id}
    role="listitem"
    onpointerdown={onPointerDown}
    onclick={onTap}
    oncontextmenu={onContextMenu}
  >
    <div class="w-9 shrink-0">
      {#if firstInGroup}
        <button
          type="button"
          onclick={() =>
            openProfile({ id: m.author_id, name: m.author_name, avatar: m.author_avatar ?? null })}
          aria-label={m.author_name}
          class="cursor-pointer rounded-full hover:opacity-80"
        >
          <Avatar id={m.author_id} name={m.author_name} avatar={m.author_avatar} size={36} />
        </button>
      {/if}
    </div>
    <div class="flex min-w-0 flex-col items-start">
      {#if firstInGroup}
        <span class="mb-0.5 ml-1 font-mono text-[0.72rem] font-medium text-beacon">
          {m.author_name}
        </span>
      {/if}
      {#if m.sticker}{@render stickerBlock()}{:else}{@render bubbleBlock()}{/if}
      {@render reactionChips()}
    </div>
  </div>
{/if}
