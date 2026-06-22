<script lang="ts">
  import { t } from "./i18n";
  import { activeRoom } from "./chat";
  import {
    callState,
    callElapsed,
    callMuted,
    callLevels,
    callSpeaker,
    callVolume,
    setVolume,
    canRouteAudio,
    incoming,
    startCall,
    acceptCall,
    declineCall,
    hangup,
    toggleMute,
    toggleSpeaker,
  } from "./call";

  const pct = (v: number): number => Math.round(Math.min(1, Math.max(0, v)) * 100);

  // Volume slider lives in a small popover so the control bar stays compact on phones.
  let showVolume = $state(false);
  const volIcon = (v: number): string => (v === 0 ? "🔇" : v < 0.5 ? "🔉" : "🔊");

  // Only phones get a loudspeaker toggle: a coarse pointer + a browser that can
  // actually route output. Desktops use the OS; iOS Safari can't route at all.
  const showSpeaker =
    canRouteAudio &&
    typeof window !== "undefined" &&
    !!window.matchMedia?.("(pointer: coarse)").matches;

  function fmtDur(s: number): string {
    const m = Math.floor(s / 60);
    const sec = Math.floor(s % 60);
    return `${m}:${String(sec).padStart(2, "0")}`;
  }

  function start(): void {
    if ($activeRoom) startCall($activeRoom);
  }
</script>

<!-- Idle: a floating call button (both roles), only with a room open. -->
{#if $callState === "idle" && $activeRoom}
  <button
    type="button"
    onclick={start}
    aria-label={$t("call")}
    title={$t("call")}
    class="fixed bottom-24 right-4 z-30 grid size-12 cursor-pointer place-items-center rounded-full border border-line bg-surface-2 text-xl text-muted shadow-lg hover:border-beacon hover:text-beacon"
  >
    📞
  </button>
{/if}

<!-- Active call bar (connecting / live). -->
{#if $callState === "connecting" || $callState === "live"}
  <div
    role="status"
    class="fixed left-1/2 top-[calc(0.6rem+env(safe-area-inset-top))] z-40 flex w-[min(26rem,92vw)] -translate-x-1/2 items-center gap-3 rounded-md border border-beacon bg-surface px-3 py-2 shadow-lg"
  >
    <span class="beacon-dot" class:animate-pulse={$callState === "connecting"}></span>
    <span class="min-w-0 flex-1 truncate font-mono text-[0.85rem] text-text">
      {#if $callState === "connecting"}
        {$t("callConnecting")}
      {:else}
        {$t("inCall")} · <span class="tabular-nums">{fmtDur($callElapsed)}</span>
      {/if}
    </span>

    <!-- Live audio levels: 🎙 your mic, 🔊 incoming. If the mic bar moves but the
         speaker bar stays flat, audio isn't coming back from the other side. -->
    <div class="flex shrink-0 items-center gap-2" title={$t("audioLevels")}>
      <span class="flex items-center gap-1" aria-hidden="true">
        <span class="text-[0.7rem] leading-none">🎙</span>
        <span class="h-2 w-7 overflow-hidden rounded-full bg-surface-2">
          <span
            class="block h-full rounded-full bg-you transition-[width] duration-75"
            style="width: {pct($callLevels.local)}%"
          ></span>
        </span>
      </span>
      <span class="flex items-center gap-1" aria-hidden="true">
        <span class="text-[0.7rem] leading-none">🔊</span>
        <span class="h-2 w-7 overflow-hidden rounded-full bg-surface-2">
          <span
            class="block h-full rounded-full bg-beacon transition-[width] duration-75"
            style="width: {pct($callLevels.remote)}%"
          ></span>
        </span>
      </span>
    </div>

    {#if showSpeaker}
      <button
        type="button"
        onclick={toggleSpeaker}
        aria-label={$t("speaker")}
        aria-pressed={$callSpeaker}
        title={$t("speaker")}
        class="grid size-9 shrink-0 cursor-pointer place-items-center rounded-md border border-line text-base text-muted hover:text-text aria-[pressed=true]:border-beacon aria-[pressed=true]:text-beacon"
      >
        {$callSpeaker ? "📢" : "🔈"}
      </button>
    {/if}
    <div class="relative shrink-0">
      <button
        type="button"
        onclick={() => (showVolume = !showVolume)}
        aria-label={$t("volume")}
        aria-pressed={showVolume}
        title={$t("volume")}
        class="grid size-9 cursor-pointer place-items-center rounded-md border border-line text-base text-muted hover:text-text aria-[pressed=true]:border-beacon aria-[pressed=true]:text-beacon"
      >
        {volIcon($callVolume)}
      </button>
      {#if showVolume}
        <div
          class="absolute bottom-full right-0 mb-2 flex items-center gap-2 rounded-md border border-line bg-surface p-2 shadow-lg"
        >
          <input
            type="range"
            min="0"
            max="1"
            step="0.05"
            value={$callVolume}
            oninput={(e) => setVolume(Number(e.currentTarget.value))}
            aria-label={$t("volume")}
            class="w-28 accent-beacon"
          />
          <span class="w-9 text-right text-[0.7rem] tabular-nums text-muted">{pct($callVolume)}%</span>
        </div>
      {/if}
    </div>
    <button
      type="button"
      onclick={toggleMute}
      aria-label={$callMuted ? $t("unmute") : $t("mute")}
      aria-pressed={$callMuted}
      class="grid size-9 shrink-0 cursor-pointer place-items-center rounded-md border border-line text-base text-muted hover:text-text aria-[pressed=true]:border-bad aria-[pressed=true]:text-bad"
    >
      {$callMuted ? "🔇" : "🎙"}
    </button>
    <button
      type="button"
      onclick={hangup}
      aria-label={$t("hangup")}
      class="grid size-9 shrink-0 cursor-pointer place-items-center rounded-md border border-bad bg-bad text-base text-[#1a1206]"
    >
      ☎
    </button>
  </div>
{/if}

<!-- Incoming call prompt. -->
{#if $callState === "ringing" && $incoming}
  <div class="fixed inset-0 z-50 grid place-items-center bg-black/50 px-6">
    <div
      role="alertdialog"
      aria-label={$t("callRingingTitle")}
      class="flex w-[min(22rem,90vw)] flex-col items-center gap-4 rounded-xl border border-beacon bg-surface p-6 text-center shadow-2xl"
    >
      <span class="grid size-14 animate-pulse place-items-center rounded-full bg-surface-2 text-3xl">
        📞
      </span>
      <div>
        <div class="font-mono text-[0.72rem] uppercase tracking-[0.1em] text-muted">
          {$t("callRingingTitle")}
        </div>
        <div class="mt-1 text-base text-text">
          {$incoming.fromName}
          <span class="text-muted">{$t("callFrom")}</span>
        </div>
      </div>
      <div class="flex w-full gap-3">
        <button
          type="button"
          onclick={declineCall}
          class="min-h-11 flex-1 cursor-pointer rounded-md border border-line text-muted hover:text-bad"
        >
          {$t("decline")}
        </button>
        <button
          type="button"
          onclick={acceptCall}
          class="min-h-11 flex-1 cursor-pointer rounded-md border border-beacon bg-beacon font-medium text-[#1a1206] hover:brightness-110"
        >
          {$t("accept")}
        </button>
      </div>
    </div>
  </div>
{/if}
