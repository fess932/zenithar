<script lang="ts">
  // Styled audio player (voice messages). Wraps a hidden <audio> so the UI matches
  // the Zenithar look instead of the browser default. Handles the MediaRecorder
  // webm quirk where duration reads Infinity until a seek forces it to resolve.
  import { t } from "./i18n";

  export let src: string;

  let audio: HTMLAudioElement;
  let playing = false;
  let duration = 0; // seconds
  let current = 0;
  let fixing = false;

  function fmt(s: number): string {
    if (!isFinite(s) || s < 0) s = 0;
    const m = Math.floor(s / 60);
    const sec = Math.floor(s % 60);
    return `${m}:${String(sec).padStart(2, "0")}`;
  }

  function toggle(): void {
    if (!audio) return;
    if (playing) audio.pause();
    else void audio.play();
  }

  function onLoaded(): void {
    if (!isFinite(audio.duration)) {
      // Force the browser to compute a real duration for webm/opus blobs.
      fixing = true;
      audio.currentTime = 1e101;
    } else {
      duration = audio.duration;
    }
  }

  function onDurationChange(): void {
    if (fixing && isFinite(audio.duration)) {
      duration = audio.duration;
      fixing = false;
      audio.currentTime = 0;
    }
  }

  function onTimeUpdate(): void {
    if (!fixing) current = audio.currentTime;
  }

  function seek(e: MouseEvent): void {
    if (!audio || !duration) return;
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const ratio = Math.min(1, Math.max(0, (e.clientX - rect.left) / rect.width));
    audio.currentTime = ratio * duration;
  }

  $: pct = duration > 0 ? Math.min(100, (current / duration) * 100) : 0;
</script>

<div
  class="inline-flex max-w-full items-center gap-2 rounded-md border border-line bg-surface-2 px-2 py-1.5"
>
  <button
    type="button"
    onclick={toggle}
    aria-label={$t("voiceMessage")}
    class="grid size-9 shrink-0 cursor-pointer place-items-center rounded-full border border-beacon text-[0.85rem] text-beacon hover:bg-beacon hover:text-[#1a1206]"
  >
    {playing ? "⏸" : "▶"}
  </button>

  <button
    type="button"
    onclick={seek}
    aria-label="seek"
    class="relative h-1.5 w-28 cursor-pointer rounded-full bg-line sm:w-40"
  >
    <span class="absolute inset-y-0 left-0 rounded-full bg-beacon" style="width:{pct}%"></span>
  </button>

  <span class="shrink-0 font-mono text-[0.7rem] tabular-nums text-muted">
    {fmt(current)} / {fmt(duration)}
  </span>

  <audio
    bind:this={audio}
    {src}
    preload="metadata"
    onloadedmetadata={onLoaded}
    ondurationchange={onDurationChange}
    ontimeupdate={onTimeUpdate}
    onplay={() => (playing = true)}
    onpause={() => (playing = false)}
    onended={() => ((playing = false), (current = 0))}
></audio>
</div>
