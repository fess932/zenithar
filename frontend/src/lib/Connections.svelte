<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { t } from "./i18n";
  import { online } from "./chat";
  import { me, listPeople, type Person } from "./session";

  export let onClose: () => void;

  let people: Person[] = [];
  let timer: ReturnType<typeof setInterval> | null = null;

  async function refresh(): Promise<void> {
    people = await listPeople();
  }

  onMount(() => {
    void refresh();
    timer = setInterval(refresh, 5000); // last-seen ticks while open
  });
  onDestroy(() => {
    if (timer) clearInterval(timer);
  });

  // Live online status from the presence store (instant), last-seen from the fetch.
  const isOnline = (p: Person): boolean => !!$online[p.id];

  // Online first, then most-recently-seen first; offline sink to the bottom.
  $: sorted = [...people].sort((a, b) => {
    const ao = isOnline(a) ? 1 : 0;
    const bo = isOnline(b) ? 1 : 0;
    if (ao !== bo) return bo - ao;
    return (b.last_seen ?? 0) - (a.last_seen ?? 0);
  });

  function ago(p: Person): string {
    if (isOnline(p)) return p.ping_ms != null ? `${p.ping_ms} ms` : $t("onlineNow");
    if (p.last_seen == null) return "—";
    const s = Math.max(0, (Date.now() - p.last_seen) / 1000);
    if (s < 60) return $t("justNow");
    const m = Math.floor(s / 60);
    if (m < 60) return `${m} ${$t("minShort")}`;
    const h = Math.floor(m / 60);
    if (h < 24) return `${h} ${$t("hrShort")}`;
    return `${Math.floor(h / 24)} ${$t("dayShort")}`;
  }
</script>

<!-- backdrop -->
<button
  type="button"
  aria-label={$t("close")}
  onclick={onClose}
  class="fixed inset-0 z-40 cursor-default bg-black/50"
></button>

<div
  role="dialog"
  aria-label={$t("connections")}
  class="fixed left-1/2 top-1/2 z-50 flex max-h-[80dvh] w-[min(24rem,92vw)] -translate-x-1/2 -translate-y-1/2 flex-col rounded-lg border border-line bg-surface shadow-xl"
>
  <header class="flex items-center justify-between border-b border-line px-4 py-3">
    <span class="font-mono text-[0.8rem] uppercase tracking-[0.08em] text-muted">
      {$t("connections")}
    </span>
    <button
      type="button"
      onclick={onClose}
      aria-label={$t("close")}
      class="cursor-pointer text-lg leading-none text-muted hover:text-text"
    >
      ✕
    </button>
  </header>

  <ul class="min-h-0 flex-1 divide-y divide-line overflow-y-auto">
    {#each sorted as p (p.id)}
      <li class="flex items-center gap-3 px-4 py-2.5" class:opacity-55={!isOnline(p)}>
        <span
          class="size-2 shrink-0 rounded-full {isOnline(p) ? 'bg-emerald-400' : 'bg-muted/40'}"
        ></span>
        <span class="min-w-0 flex-1 truncate text-[0.9rem]">
          {p.name}{#if p.id === $me?.id}<span class="text-muted"> ({$t("you")})</span>{/if}
        </span>
        <span
          class="shrink-0 font-mono text-[0.68rem] uppercase tracking-[0.06em]"
          class:text-you={p.kind === "user"}
          class:text-muted={p.kind !== "user"}
        >
          {p.kind === "user" ? $t("roleUser") : $t("roleClient")}
        </span>
        <span class="w-16 shrink-0 text-right font-mono text-[0.68rem] tabular-nums text-muted">
          {ago(p)}
        </span>
      </li>
    {:else}
      <li class="px-4 py-6 text-center font-mono text-[0.82rem] text-muted">{$t("noPeople")}</li>
    {/each}
  </ul>
</div>
