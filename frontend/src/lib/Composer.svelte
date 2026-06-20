<script lang="ts">
  import { t } from "./i18n";
  import { send, uploadFile, notify, replyingTo, type Attachment } from "./chat";
  import { EMOJI } from "./emoji";

  const MAX_ATTACH = 5;

  let body = "";
  let pending: Attachment[] = [];
  let uploading = false;
  let showEmoji = false;

  let fileInput: HTMLInputElement;

  // voice recording
  let recording = false;
  let recorder: MediaRecorder | null = null;
  let stream: MediaStream | null = null;
  let chunks: Blob[] = [];
  let recSeconds = 0;
  let recTimer: ReturnType<typeof setInterval> | null = null;

  function fmtDur(s: number): string {
    const m = Math.floor(s / 60);
    const sec = Math.floor(s % 60);
    return `${m}:${String(sec).padStart(2, "0")}`;
  }

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

  function submit(e: SubmitEvent): void {
    e.preventDefault();
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
    }
  }

  function addEmoji(em: string): void {
    body += em;
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
    uploading = true;
    const a = await uploadFile(file);
    uploading = false;
    if (a) pending = [...pending, a];
  }

  function removePending(id: string): void {
    pending = pending.filter((a) => a.id !== id);
  }

  // Paste an image straight from the clipboard (e.g. a screenshot) — uploads it
  // as an attachment instead of pasting garbage text. Mostly a desktop flow;
  // mobile keyboards rarely expose images here, so phones use the 📎 button.
  async function onPaste(e: ClipboardEvent): Promise<void> {
    const items = Array.from(e.clipboardData?.items ?? []);
    const images = items.filter((it) => it.kind === "file" && it.type.startsWith("image/"));
    if (images.length === 0) return; // ordinary text paste — leave it alone
    e.preventDefault();
    for (const it of images) {
      const blob = it.getAsFile();
      if (!blob) continue;
      const ext = (blob.type.split("/")[1] || "png").replace("jpeg", "jpg");
      const file = new File([blob], `pasted-${Date.now()}.${ext}`, { type: blob.type });
      await doUpload(file);
    }
  }

  // ---- voice ----------------------------------------------------------------
  async function startRecording(): Promise<void> {
    try {
      stream = await navigator.mediaDevices.getUserMedia({
        audio: { echoCancellation: true, noiseSuppression: true, autoGainControl: true },
      });
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

<div class="min-w-0 w-full border-t border-line bg-surface px-3 pt-[0.6rem] pb-[calc(0.6rem+env(safe-area-inset-bottom))] sm:px-5">
  <!-- reply target (Telegram-style): quoted line above the input -->
  {#if $replyingTo}
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

  <!-- pending attachments (max 5) -->
  {#if pending.length > 0}
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
              {a.content_type.startsWith("audio/") ? "🎤" : "📎"}
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
    </div>
  {/if}

  <!-- emoji panel -->
  {#if showEmoji}
    <div class="mb-2 max-h-44 overflow-y-auto rounded-md border border-line bg-surface-2 p-2">
      <div class="grid grid-cols-8 gap-1 sm:grid-cols-10">
        {#each EMOJI as em}
          <button
            type="button"
            onclick={() => addEmoji(em)}
            class="grid aspect-square cursor-pointer place-items-center rounded text-xl hover:bg-surface"
          >
            {em}
          </button>
        {/each}
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
        bind:value={body}
        onpaste={onPaste}
        placeholder={uploading ? $t("uploading") : $t("messagePlaceholder")}
        aria-label={$t("messageAria")}
        maxlength="4000"
        disabled={uploading}
        class="min-h-11 min-w-0 flex-1 rounded-md border border-line bg-surface-2 px-3 text-base text-text placeholder:text-muted focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-beacon disabled:opacity-60 sm:text-[0.9375rem]"
      />

      {#if canSend}
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
  </form>
</div>
