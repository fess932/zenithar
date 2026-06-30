<script lang="ts">
  import { rooms, activeRoom, unread, online, resync, loadRooms, type RoomSummary } from "./chat";
  import { mutedRooms, toggleMute } from "./notify";
  import { t, lang } from "./i18n";
  import Avatar from "./Avatar.svelte";

  export let onPick: (id: string) => void;

  $: onlineEmployees = Object.values($online).filter((k) => k === "user").length;
  const isClientOnline = (r: RoomSummary): boolean => !!(r.client_id && $online[r.client_id]);

  function roomLabel(r: RoomSummary): string {
    if (r.kind === "common") return $t("room");
    if (r.kind === "direct") return `@${r.title ?? "?"}`; // @handle = a person
    return r.title ?? "—";
  }

  // Last-message preview. In group-like rooms (common/client) prefix the sender;
  // a 1:1 direct room needs no name. Empty body = attachment-only → a clip marker.
  function preview(r: RoomSummary): string {
    const text = (r.last_body ?? "").trim() || "📎";
    if (r.kind === "direct") return text;
    return r.last_author ? `${r.last_author}: ${text}` : text;
  }

  // Telegram-style timestamp: today → HH:MM, yesterday → label, this week →
  // weekday, older → date.
  function shortTime(ms: number | null): string {
    if (!ms) return "";
    const d = new Date(ms);
    const now = new Date();
    const loc = $lang === "ru" ? "ru-RU" : "en-US";
    if (d.toDateString() === now.toDateString())
      return d.toLocaleTimeString(loc, { hour: "2-digit", minute: "2-digit" });
    const y = new Date(now);
    y.setDate(now.getDate() - 1);
    if (d.toDateString() === y.toDateString()) return $t("yesterday");
    if (now.getTime() - ms < 7 * 86400000) return d.toLocaleDateString(loc, { weekday: "short" });
    const opts: Intl.DateTimeFormatOptions = { day: "2-digit", month: "2-digit" };
    if (d.getFullYear() !== now.getFullYear()) opts.year = "2-digit";
    return d.toLocaleDateString(loc, opts);
  }

  // --- pull-to-refresh (drag down from the top) -------------------------------
  let scroller: HTMLElement;
  let pullStart: number | null = null;
  let pullDist = 0;
  let refreshing = false;
  const THRESHOLD = 60;

  function onTouchStart(e: TouchEvent): void {
    pullStart = scroller && scroller.scrollTop <= 0 ? (e.touches[0]?.clientY ?? null) : null;
  }
  function onTouchMove(e: TouchEvent): void {
    if (pullStart === null || refreshing) return;
    const dy = (e.touches[0]?.clientY ?? pullStart) - pullStart;
    pullDist = dy > 0 ? Math.min(dy * 0.5, 80) : 0;
  }
  async function onTouchEnd(): Promise<void> {
    if (pullStart === null) return;
    pullStart = null;
    if (pullDist >= THRESHOLD && !refreshing) {
      refreshing = true;
      pullDist = THRESHOLD; // hold while refreshing
      resync(); // bounce the socket → re-pull the open room
      await loadRooms(); // refresh the list now
      setTimeout(() => {
        refreshing = false;
        pullDist = 0;
      }, 500);
    } else {
      pullDist = 0;
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<!-- The touch handlers drive pull-to-refresh, not selection; rows below are real
     buttons. The container itself stays non-interactive. -->
<div
  bind:this={scroller}
  ontouchstart={onTouchStart}
  ontouchmove={onTouchMove}
  ontouchend={onTouchEnd}
  class="relative h-full overflow-y-auto overscroll-contain"
>
  <!-- pull-to-refresh indicator -->
  <div
    class="pointer-events-none absolute inset-x-0 top-0 flex items-center justify-center overflow-hidden"
    style:height="{pullDist}px"
    style:opacity={pullDist > 6 ? 1 : 0}
  >
    <span
      class="text-lg text-muted {refreshing ? 'animate-spin' : ''}"
      style:transform="rotate({refreshing ? 0 : Math.round(pullDist * 4)}deg)">↻</span
    >
  </div>

  <div
    style:transform="translateY({pullDist}px)"
    style:transition={pullStart !== null ? "none" : "transform 0.15s"}
  >
    {#if $rooms.length === 0}
      <p class="px-4 py-8 font-mono text-[0.82rem] text-muted">{$t("noRooms")}</p>
    {:else}
      {#each $rooms as r (r.id)}
        {@const count = $unread[r.id] ?? 0}
        {@const muted = $mutedRooms.has(r.id)}
        {@const live = r.kind === "common" ? onlineEmployees > 0 : isClientOnline(r)}
        {@const label = roomLabel(r)}
        <div class="flex items-stretch border-b border-line/40">
          <button
            type="button"
            onclick={() => onPick(r.id)}
            aria-current={r.id === $activeRoom}
            class="flex min-w-0 flex-1 items-center gap-3 px-3 py-2.5 text-left hover:bg-surface-2 aria-[current=true]:bg-surface-2"
          >
            <div class="relative shrink-0">
              {#if r.kind === "common"}
                <div
                  class="grid size-11 place-items-center rounded-full bg-surface-2 text-base text-muted"
                >
                  #
                </div>
              {:else}
                <Avatar id={r.client_id ?? r.id} name={label} avatar={null} size={44} />
              {/if}
              {#if live}
                <span
                  class="absolute bottom-0 right-0 size-3 rounded-full border-2 border-surface bg-emerald-400"
                ></span>
              {/if}
            </div>
            <div class="min-w-0 flex-1">
              <div class="flex items-baseline gap-2">
                <span class="min-w-0 flex-1 truncate font-medium text-text">{label}</span>
                {#if r.last_at}
                  <span class="shrink-0 font-mono text-[0.66rem] text-muted">{shortTime(r.last_at)}</span>
                {/if}
              </div>
              <div class="mt-0.5 flex items-center gap-2">
                <span class="min-w-0 flex-1 truncate text-[0.8rem] text-muted">{preview(r)}</span>
                {#if muted}<span class="shrink-0 text-[0.7rem] opacity-60">🔕</span>{/if}
                {#if count > 0}
                  <span
                    class="grid min-w-5 shrink-0 place-items-center rounded-full bg-beacon px-1.5 text-[0.7rem] font-medium text-[#1a1206]"
                    class:opacity-50={muted}
                  >
                    {count}
                  </span>
                {/if}
              </div>
            </div>
          </button>
          {#if r.kind === "client"}
            <button
              type="button"
              onclick={() => toggleMute(r.id)}
              aria-label={muted ? $t("unmuteRoom") : $t("muteRoom")}
              aria-pressed={muted}
              class="grid w-10 shrink-0 cursor-pointer place-items-center text-muted hover:text-text"
            >
              {muted ? "🔕" : "🔔"}
            </button>
          {/if}
        </div>
      {/each}
    {/if}
  </div>
</div>
