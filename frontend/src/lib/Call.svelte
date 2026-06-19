<script lang="ts">
  import { t } from "./i18n";
  import { activeRoom } from "./chat";
  import {
    callState,
    callElapsed,
    callMuted,
    incoming,
    startCall,
    acceptCall,
    declineCall,
    hangup,
    toggleMute,
  } from "./call";

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
