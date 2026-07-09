<script lang="ts">
  import { tick } from "svelte";
  import { t } from "./i18n";
  import { send, sendSticker, uploadFile, notify, replyingTo, editing, editMessage, uploadLimit, type Attachment } from "./chat";
  import { EMOJI } from "./emoji";
  import { getMicStream } from "./mic";
  import { fmtDur } from "./util/format";
  import Sticker from "./Sticker.svelte";
  import Thumb from "./Thumb.svelte";
  import { STICKERS, stickerUrl, formatOf, sticker } from "./stickers";
  import PickerTile from "./PickerTile.svelte";
  import { listSaved, sendSaved, uploadSaved, savedThumb, savedUrl, type SavedItem } from "./saved";
  import PacksPanel from "./PacksPanel.svelte";
  import { listPacks, isLottie, isVideoSticker, type Pack } from "./packs";
  import {
    pushRecentEmoji,
    pushRecentBundled,
    pushRecentItem,
    recentsFor,
    type RecentCat,
    type RecentEntry,
  } from "./recents";
  import type { PreviewKind } from "./stickerPreview";

  const recentEmoji = recentsFor("emoji");
  const recentStickers = recentsFor("stickers");
  const recentGifs = recentsFor("gifs");
  const recentSaved = recentsFor("saved");

  // Renderer for a recent item, inferred from its stored content-type.
  function itemKind(ct: string): PreviewKind {
    return isLottie(ct)
      ? "lottie"
      : isVideoSticker(ct)
        ? "webm"
        : ct.startsWith("video/")
          ? "video"
          : "img";
  }
  // Look up a just-sent item's content-type from the loaded lists so the recent
  // entry knows how to render it later.
  function findCt(id: string): string {
    const s = savedItems?.find((it) => it.id === id);
    if (s) return s.content_type;
    for (const p of packs ?? []) {
      const it = p.items.find((x) => x.id === id);
      if (it) return it.content_type;
    }
    return "";
  }

  const MAX_ATTACH = 5;

  let body = "";
  let pending: Attachment[] = [];
  let uploading = false;
  let showEmoji = false;
  // Picker tabs. Stickers/GIFs/Saved each host the matching pack sub-lists.
  let pickerTab: "emoji" | "stickers" | "gifs" | "saved" = "emoji";

  // Saved items ("сохранёнки"), loaded lazily when the tab is first opened.
  let savedItems: SavedItem[] | null = null;
  let loadingSaved = false;
  let savedFileInput: HTMLInputElement;

  async function loadSaved(): Promise<void> {
    loadingSaved = true;
    savedItems = await listSaved();
    loadingSaved = false;
  }
  $: if (showEmoji && pickerTab === "saved" && savedItems === null && !loadingSaved) void loadSaved();

  // Packs (sticker/gif/saved sub-lists), loaded once when the picker first opens.
  let packs: Pack[] | null = null;
  async function loadPacks(): Promise<void> {
    packs = await listPacks();
  }
  // After an import the server may file the pack under a different kind (detected
  // from its contents) — reload and jump to that tab so it's visible.
  async function onPacksChanged(pack?: Pack): Promise<void> {
    await loadPacks();
    if (pack)
      pickerTab = pack.kind === "sticker" ? "stickers" : pack.kind === "gif" ? "gifs" : "saved";
  }
  $: if (showEmoji && packs === null) void loadPacks();
  $: stickerPacks = (packs ?? []).filter((p) => p.kind === "sticker");
  $: gifPacks = (packs ?? []).filter((p) => p.kind === "gif");
  $: savedPacks = (packs ?? []).filter((p) => p.kind === "saved");

  function pickSaved(id: string): void {
    const cat: RecentCat = pickerTab === "gifs" ? "gifs" : pickerTab === "saved" ? "saved" : "stickers";
    pushRecentItem(cat, id, findCt(id));
    void sendSaved(id);
    showEmoji = false;
  }
  async function onSavedPicked(e: Event): Promise<void> {
    const input = e.target as HTMLInputElement;
    const files = Array.from(input.files ?? []);
    input.value = "";
    for (const file of files) {
      const it = await uploadSaved(file);
      if (it) savedItems = [it, ...(savedItems ?? [])];
    }
  }

  let fileInput: HTMLInputElement;
  let inputEl: HTMLInputElement;

  // Keep the soft keyboard up after sending: tapping the send button (or its
  // swap to the mic button when the field empties) blurs the input, which closes
  // the keyboard on Android. Re-focusing inside the submit gesture holds it open.
  function keepFocus(): void {
    inputEl?.focus();
  }

  // voice recording
  let recording = false;
  let recorder: MediaRecorder | null = null;
  let stream: MediaStream | null = null;
  let chunks: Blob[] = [];
  let recSeconds = 0;
  let recTimer: ReturnType<typeof setInterval> | null = null;


  function stopTimer(): void {
    if (recTimer) clearInterval(recTimer);
    recTimer = null;
  }

  $: canSend = (body.trim().length > 0 || pending.length > 0) && !uploading;

  // Preview text for the reply banner (parent body, or an attachment marker).
  $: replyText = $replyingTo
    ? $replyingTo.body.trim() ||
      ($replyingTo.attachments.length > 0 ? $t("attachment") : "")
    : "";

  // Entering edit mode: prefill the input with the message body once (when the
  // target changes), without clobbering what the user types afterwards.
  let editId: string | null = null;
  $: {
    const cur = $editing?.id ?? null;
    if (cur && cur !== editId) body = $editing?.body ?? "";
    editId = cur;
  }

  function cancelEdit(): void {
    editing.set(null);
    body = "";
  }

  function submit(e: SubmitEvent): void {
    e.preventDefault();
    if ($editing) {
      const text = body.trim();
      if (text.length === 0) return; // empty edit is a no-op (delete instead)
      editMessage($editing.id, text);
      editing.set(null);
      body = "";
      showEmoji = false;
      keepFocus();
      return;
    }
    if (!canSend) return;
    if (
      send(
        body.trim(),
        pending.map((a) => a.id),
        $replyingTo?.id ?? null,
      )
    ) {
      body = "";
      pending = [];
      showEmoji = false;
      replyingTo.set(null);
      keepFocus();
    }
  }

  function addEmoji(em: string): void {
    body += em;
    pushRecentEmoji(em);
  }

  function pickSticker(id: string): void {
    if (sendSticker(id)) {
      pushRecentBundled(id);
      showEmoji = false;
    }
  }

  async function onFilePicked(e: Event): Promise<void> {
    const input = e.target as HTMLInputElement;
    const files = Array.from(input.files ?? []);
    input.value = ""; // allow re-picking the same file
    for (const file of files) await doUpload(file);
  }

  async function doUpload(file: File): Promise<void> {
    if (pending.length >= MAX_ATTACH) {
      notify($t("errTooMany"));
      return;
    }
    if (file.size > uploadLimit(file)) {
      notify($t("errTooBig"));
      return;
    }
    uploading = true;
    const a = await uploadFile(file);
    uploading = false;
    if (a) pending = [...pending, a];
    // The input is disabled while uploading, which blurs it — so after a paste
    // (or drop/pick) focus is gone and Enter no longer submits. Return focus once
    // the field is enabled again so you can type a caption and hit Enter.
    await tick();
    inputEl?.focus();
  }

  function removePending(id: string): void {
    pending = pending.filter((a) => a.id !== id);
  }

  // Drag-and-drop: drop a file anywhere over the app to attach it (the reliable
  // desktop path when Finder-copied files don't make it onto the clipboard).
  // Listeners sit on window so the whole chat is a drop target; a counter tracks
  // enter/leave so the overlay doesn't flicker over child elements.
  let dragging = false;
  let dragDepth = 0;

  function hasFiles(e: DragEvent): boolean {
    return Array.from(e.dataTransfer?.types ?? []).includes("Files");
  }

  function onDragEnter(e: DragEvent): void {
    if (!hasFiles(e)) return;
    e.preventDefault();
    if ($editing) return; // edits don't carry attachments
    dragDepth += 1;
    dragging = true;
  }

  function onDragOver(e: DragEvent): void {
    if (!hasFiles(e)) return;
    e.preventDefault(); // required so the drop event fires instead of navigation
    if (e.dataTransfer) e.dataTransfer.dropEffect = $editing ? "none" : "copy";
  }

  function onDragLeave(): void {
    if (!dragging) return;
    dragDepth -= 1;
    if (dragDepth <= 0) {
      dragDepth = 0;
      dragging = false;
    }
  }

  async function onDrop(e: DragEvent): Promise<void> {
    if (!hasFiles(e)) return;
    e.preventDefault();
    dragging = false;
    dragDepth = 0;
    if ($editing) return;
    for (const file of Array.from(e.dataTransfer?.files ?? [])) await doUpload(file);
  }

  // Paste image/video straight from the clipboard (e.g. a screenshot, or a clip
  // copied from an app) — uploads it as an attachment instead of pasting garbage
  // text. Mostly a desktop flow; mobile keyboards rarely expose media here, so
  // phones use the 📎 button. Note: on macOS a file COPIED IN FINDER usually
  // isn't exposed to the page as clipboard bytes — that path needs the picker or
  // drag-drop; this catches media data already on the clipboard (screenshots etc).
  async function onPaste(e: ClipboardEvent): Promise<void> {
    const items = Array.from(e.clipboardData?.items ?? []);
    const media = items.filter(
      (it) =>
        it.kind === "file" &&
        (it.type.startsWith("image/") || it.type.startsWith("video/")),
    );
    if (media.length === 0) return; // ordinary text paste — leave it alone
    e.preventDefault();
    for (const it of media) {
      const blob = it.getAsFile();
      if (!blob) continue;
      const ext = (blob.type.split("/")[1] || "bin").replace("jpeg", "jpg");
      const prefix = blob.type.startsWith("video/") ? "clip" : "pasted";
      const file = new File([blob], `${prefix}-${Date.now()}.${ext}`, { type: blob.type });
      await doUpload(file);
    }
  }

  // ---- voice ----------------------------------------------------------------
  async function startRecording(): Promise<void> {
    try {
      stream = await getMicStream();
    } catch {
      return; // permission denied / no mic
    }
    chunks = [];
    recorder = new MediaRecorder(stream);
    recorder.ondataavailable = (e) => e.data.size > 0 && chunks.push(e.data);
    recorder.onstop = onRecordingStop;
    recorder.start();
    recording = true;
    recSeconds = 0;
    recTimer = setInterval(() => (recSeconds += 1), 1000);
  }

  function stopRecording(): void {
    stopTimer();
    recorder?.stop(); // fires onstop → uploads
    recording = false;
  }

  function cancelRecording(): void {
    stopTimer();
    if (recorder) recorder.onstop = null;
    recorder?.stop();
    releaseMic();
    recording = false;
    chunks = [];
  }

  async function onRecordingStop(): Promise<void> {
    stopTimer();
    const type = recorder?.mimeType || "audio/webm";
    const blob = new Blob(chunks, { type });
    releaseMic();
    const ext = type.includes("ogg") ? "ogg" : type.includes("mp4") ? "m4a" : "webm";
    const file = new File([blob], `voice-${Date.now()}.${ext}`, { type });
    await doUpload(file);
  }

  function releaseMic(): void {
    stream?.getTracks().forEach((tr) => tr.stop());
    stream = null;
  }

  function pendingLabel(a: Attachment): string {
    return a.content_type.startsWith("audio/") ? $t("voiceMessage") : a.filename;
  }
</script>

<svelte:window
  ondragenter={onDragEnter}
  ondragover={onDragOver}
  ondragleave={onDragLeave}
  ondrop={onDrop}
/>

<!-- Drag-and-drop overlay: covers the viewport while a file is dragged over. -->
{#if dragging}
  <div
    class="pointer-events-none fixed inset-0 z-40 grid place-items-center bg-ink/70 p-6 backdrop-blur-sm"
  >
    <div
      class="grid place-items-center gap-2 rounded-2xl border-2 border-dashed border-beacon bg-surface/80 px-10 py-8 text-center shadow-2xl"
    >
      <span class="text-4xl leading-none">📎</span>
      <span class="font-mono text-sm text-beacon">{$t("dropHint")}</span>
    </div>
  </div>
{/if}

<div class="min-w-0 w-full border-t border-line bg-surface px-3 pt-[0.6rem] pb-[calc(0.6rem+env(safe-area-inset-bottom))] sm:px-5">
  <!-- edit mode: banner above the input; submit sends an edit, not a new message -->
  {#if $editing}
    <div class="mb-2 flex items-center gap-2 rounded-md border-l-2 border-beacon bg-surface-2 py-1 pl-2 pr-1">
      <span class="grid shrink-0 place-items-center text-base text-beacon">✎</span>
      <div class="min-w-0 flex-1">
        <div class="truncate font-mono text-[0.72rem] text-beacon">{$t("editing")}</div>
        <div class="line-clamp-1 text-[0.8rem] text-muted">{$editing.body}</div>
      </div>
      <button
        type="button"
        onclick={cancelEdit}
        aria-label={$t("cancelEdit")}
        class="grid size-7 shrink-0 cursor-pointer place-items-center rounded text-muted hover:text-bad"
      >
        ✕
      </button>
    </div>
  {/if}

  <!-- reply target (Telegram-style): quoted line above the input -->
  {#if $replyingTo && !$editing}
    <div class="mb-2 flex items-center gap-2 rounded-md border-l-2 border-beacon bg-surface-2 py-1 pl-2 pr-1">
      <span class="grid shrink-0 place-items-center text-base text-beacon">↩</span>
      <div class="min-w-0 flex-1">
        <div class="truncate font-mono text-[0.72rem] text-beacon">{$replyingTo.author_name}</div>
        <div class="line-clamp-1 text-[0.8rem] text-muted">
          {#if $replyingTo.attachments.length > 0 && !$replyingTo.body.trim()}📎 {/if}{replyText}
        </div>
      </div>
      <button
        type="button"
        onclick={() => replyingTo.set(null)}
        aria-label={$t("cancelReply")}
        class="grid size-7 shrink-0 cursor-pointer place-items-center rounded text-muted hover:text-bad"
      >
        ✕
      </button>
    </div>
  {/if}

  <!-- pending attachments (max 5), plus an in-progress chip while a file uploads
       (the wait covers server-side image processing, so show it's not stuck) -->
  {#if pending.length > 0 || uploading}
    <div class="mb-2 flex flex-wrap gap-2">
      {#each pending as a (a.id)}
        <div class="flex items-center gap-2 rounded-md border border-line bg-surface-2 py-1 pl-1 pr-2">
          {#if a.content_type.startsWith("image/")}
            <img
              src={`/api/attachments/${a.id}${a.has_thumb ? "/thumb" : ""}`}
              alt={a.filename}
              class="size-9 shrink-0 rounded border border-line object-cover"
            />
          {:else}
            <span class="grid size-9 shrink-0 place-items-center text-base">
              {a.content_type.startsWith("audio/")
                ? "🎤"
                : a.content_type.startsWith("video/")
                  ? "🎬"
                  : "📎"}
            </span>
          {/if}
          <span class="max-w-[8rem] truncate font-mono text-[0.74rem] text-text">{pendingLabel(a)}</span>
          <button
            type="button"
            onclick={() => removePending(a.id)}
            aria-label={$t("removeAttachment")}
            class="grid size-6 shrink-0 cursor-pointer place-items-center rounded text-muted hover:text-bad"
          >
            ✕
          </button>
        </div>
      {/each}
      {#if uploading}
        <div
          class="flex items-center gap-2 rounded-md border border-line bg-surface-2 py-1 pl-2 pr-2.5"
        >
          <span
            class="size-4 shrink-0 animate-spin rounded-full border-2 border-line border-t-beacon motion-reduce:animate-none"
          ></span>
          <span class="font-mono text-[0.74rem] text-muted">{$t("uploading")}</span>
        </div>
      {/if}
    </div>
  {/if}

  <!-- One recent picker item (bundled sticker or a saved/pack item). -->
  {#snippet recentTile(e: RecentEntry)}
    {#if e.kind === "bundled"}
      {@const def = sticker(e.id)}
      {#if def}
        <PickerTile
          onSend={() => pickSticker(e.id)}
          previewSrc={stickerUrl(def)}
          previewKind={formatOf(def.file) === "lottie"
            ? "lottie"
            : formatOf(def.file) === "webm"
              ? "webm"
              : "img"}
          alt={def.emoji}
          class="grid aspect-square cursor-pointer place-items-center rounded p-1 hover:bg-surface"
        >
          <Sticker {def} size={52} />
        </PickerTile>
      {/if}
    {:else if e.kind === "item"}
      {@const k = itemKind(e.ct)}
      <div class="relative aspect-square">
        <PickerTile
          onSend={() => pickSaved(e.id)}
          previewSrc={savedUrl(e.id)}
          previewKind={k}
          class="grid size-full cursor-pointer place-items-center rounded p-0.5 hover:bg-surface"
        >
          {#if k === "lottie"}
            <Sticker src={savedUrl(e.id)} format="lottie" size={52} />
          {:else if k === "webm"}
            <Sticker src={savedUrl(e.id)} format="webm" size={52} />
          {:else}
            <Thumb src={savedUrl(e.id)} class="max-h-full max-w-full object-contain" />
          {/if}
        </PickerTile>
      </div>
    {/if}
  {/snippet}

  <!-- "Recent" strip at the top of a sticker/gif/saved tab. -->
  {#snippet recentStrip(entries: RecentEntry[])}
    {#if entries.length}
      <div class="mb-2 border-b border-line pb-2">
        <p class="mb-1 px-0.5 font-mono text-[0.6rem] uppercase tracking-[0.08em] text-muted">
          {$t("recent")}
        </p>
        <div class="grid grid-cols-[repeat(auto-fill,minmax(3.5rem,1fr))] gap-1">
          {#each entries.slice(0, 16) as e (e.kind + ":" + (e.kind === "emoji" ? e.v : e.id))}
            {@render recentTile(e)}
          {/each}
        </div>
      </div>
    {/if}
  {/snippet}

  <!-- emoji / stickers / gifs panel -->
  {#if showEmoji}
    <div class="mb-2 overflow-hidden rounded-md border border-line bg-surface-2">
      <!-- tabs -->
      <div class="flex border-b border-line">
        {#each [{ id: "emoji", label: $t("emoji") }, { id: "stickers", label: $t("stickers") }, { id: "gifs", label: $t("gifs") }, { id: "saved", label: $t("saved") }] as tab}
          <button
            type="button"
            onclick={() => (pickerTab = tab.id as typeof pickerTab)}
            aria-pressed={pickerTab === tab.id}
            class="flex-1 border-b-2 border-transparent py-1.5 font-mono text-[0.7rem] uppercase tracking-[0.06em] text-muted hover:text-text aria-[pressed=true]:border-beacon aria-[pressed=true]:text-beacon"
          >
            {tab.label}
          </button>
        {/each}
      </div>
      <!-- content -->
      <div class="max-h-44 overflow-y-auto p-2">
        {#if pickerTab === "emoji"}
          {#if $recentEmoji.length}
            <div class="mb-2 border-b border-line pb-2">
              <p
                class="mb-1 px-0.5 font-mono text-[0.6rem] uppercase tracking-[0.08em] text-muted"
              >
                {$t("recent")}
              </p>
              <div class="grid grid-cols-[repeat(auto-fill,minmax(2.25rem,1fr))] gap-1">
                {#each $recentEmoji.slice(0, 24) as e (e.kind === "emoji" ? e.v : "")}
                  {#if e.kind === "emoji"}
                    <button
                      type="button"
                      onclick={() => addEmoji(e.v)}
                      class="grid aspect-square cursor-pointer place-items-center rounded text-lg hover:bg-surface"
                    >
                      {e.v}
                    </button>
                  {/if}
                {/each}
              </div>
            </div>
          {/if}
          <div class="grid grid-cols-[repeat(auto-fill,minmax(2.25rem,1fr))] gap-1">
            {#each EMOJI as em}
              <button
                type="button"
                onclick={() => addEmoji(em)}
                class="grid aspect-square cursor-pointer place-items-center rounded text-lg hover:bg-surface"
              >
                {em}
              </button>
            {/each}
          </div>
        {:else if pickerTab === "stickers"}
          {@render recentStrip($recentStickers)}
          <div class="grid grid-cols-[repeat(auto-fill,minmax(4.5rem,1fr))] gap-1">
            {#each STICKERS as s (s.id)}
              <PickerTile
                onSend={() => pickSticker(s.id)}
                previewSrc={stickerUrl(s)}
                previewKind={formatOf(s.file) === "lottie"
                  ? "lottie"
                  : formatOf(s.file) === "webm"
                    ? "webm"
                    : "img"}
                alt={s.emoji}
                class="grid aspect-square cursor-pointer place-items-center rounded p-1 hover:bg-surface"
              >
                <Sticker def={s} size={64} />
              </PickerTile>
            {/each}
          </div>
          <div class="mt-2 border-t border-line pt-2">
            <PacksPanel packs={stickerPacks} onSend={pickSaved} onChanged={onPacksChanged} />
          </div>
        {:else if pickerTab === "gifs"}
          {@render recentStrip($recentGifs)}
          <PacksPanel packs={gifPacks} onSend={pickSaved} onChanged={onPacksChanged} />
        {:else if pickerTab === "saved"}
          {@render recentStrip($recentSaved)}
          <div class="grid grid-cols-[repeat(auto-fill,minmax(4.5rem,1fr))] gap-1">
            <!-- Upload tile -->
            <button
              type="button"
              onclick={() => savedFileInput.click()}
              aria-label={$t("uploadSaved")}
              title={$t("uploadSaved")}
              class="grid aspect-square cursor-pointer place-items-center rounded border border-dashed border-line text-2xl text-muted hover:border-beacon hover:text-beacon"
            >
              +
            </button>
            {#each savedItems ?? [] as it (it.id)}
              <PickerTile
                onSend={() => pickSaved(it.id)}
                previewSrc={savedUrl(it.id)}
                previewKind={it.content_type.startsWith("video/") ? "video" : "img"}
                alt={it.filename}
                class="relative aspect-square cursor-pointer overflow-hidden rounded border border-line hover:border-beacon"
              >
                {#if it.content_type.startsWith("video/")}
                  <!-- svelte-ignore a11y_media_has_caption -->
                  <video
                    src={`${savedUrl(it.id)}#t=0.1`}
                    muted
                    playsinline
                    preload="metadata"
                    class="size-full object-cover"
                  ></video>
                  <span class="pointer-events-none absolute inset-0 grid place-items-center text-white/90">▶</span>
                {:else}
                  <Thumb
                    src={it.has_thumb && !it.has_alpha ? savedThumb(it.id) : savedUrl(it.id)}
                    alt={it.filename}
                  />
                {/if}
              </PickerTile>
            {/each}
          </div>
          {#if savedItems !== null && savedItems.length === 0}
            <p class="px-1 py-3 font-mono text-[0.74rem] leading-snug text-muted">{$t("noSaved")}</p>
          {/if}
          <div class="mt-2 border-t border-line pt-2">
            <PacksPanel packs={savedPacks} onSend={pickSaved} onChanged={onPacksChanged} />
          </div>
        {/if}
      </div>
    </div>
  {/if}

  <form onsubmit={submit} class="flex items-center gap-1.5" autocomplete="off">
    {#if recording}
      <!-- recording bar -->
      <button
        type="button"
        onclick={cancelRecording}
        aria-label={$t("cancelRecording")}
        class="grid size-11 shrink-0 cursor-pointer place-items-center rounded-md border border-line text-muted hover:text-bad"
      >
        ✕
      </button>
      <div class="beacon flex flex-1 items-center gap-2 px-2 font-mono text-[0.85rem] text-bad" data-state="down">
        <span class="beacon-dot"></span>
        <span>{$t("recording")}</span>
        <span class="ml-auto tabular-nums">{fmtDur(recSeconds)}</span>
      </div>
      <button
        type="button"
        onclick={stopRecording}
        aria-label={$t("stopRecording")}
        class="grid size-11 shrink-0 cursor-pointer place-items-center rounded-md border border-beacon bg-beacon text-[#1a1206]"
      >
        ⏹
      </button>
    {:else}
      <button
        type="button"
        onclick={() => (showEmoji = !showEmoji)}
        aria-label={$t("emoji")}
        aria-pressed={showEmoji}
        class="grid size-11 shrink-0 cursor-pointer place-items-center rounded-md border border-line text-lg text-muted hover:text-text aria-[pressed=true]:text-beacon"
      >
        😊
      </button>
      <button
        type="button"
        onclick={() => fileInput.click()}
        aria-label={$t("attach")}
        disabled={pending.length >= MAX_ATTACH}
        class="grid size-11 shrink-0 cursor-pointer place-items-center rounded-md border border-line text-lg text-muted hover:text-text disabled:opacity-40"
      >
        📎
      </button>

      <input
        bind:this={inputEl}
        bind:value={body}
        onpaste={onPaste}
        placeholder={uploading ? $t("uploading") : $t("messagePlaceholder")}
        aria-label={$t("messageAria")}
        maxlength="4000"
        disabled={uploading}
        class="min-h-11 min-w-0 flex-1 rounded-md border border-line bg-surface-2 px-3 text-base text-text placeholder:text-muted focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-beacon disabled:opacity-60 sm:text-[0.9375rem]"
      />

      {#if canSend || $editing}
        <button
          type="submit"
          aria-label={$t("send")}
          class="grid size-11 shrink-0 cursor-pointer place-items-center rounded-md border border-beacon bg-beacon text-lg text-[#1a1206] hover:brightness-110"
        >
          ➤
        </button>
      {:else}
        <button
          type="button"
          onclick={startRecording}
          aria-label={$t("recordVoice")}
          class="grid size-11 shrink-0 cursor-pointer place-items-center rounded-md border border-line text-lg text-muted hover:text-text"
        >
          🎤
        </button>
      {/if}
    {/if}

    <input
      bind:this={fileInput}
      type="file"
      multiple
      onchange={onFilePicked}
      class="hidden"
      aria-hidden="true"
    />
    <input
      bind:this={savedFileInput}
      type="file"
      accept="image/*"
      multiple
      onchange={onSavedPicked}
      class="hidden"
      aria-hidden="true"
    />
  </form>
</div>
