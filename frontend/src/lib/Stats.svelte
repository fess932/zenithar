<script lang="ts">
  import { onMount } from "svelte";
  import { t } from "./i18n";
  import { getStats, type UsageStats } from "./session";
  import { fmtBytes } from "./util/format";

  let stats: UsageStats | null = null;
  let loading = true;

  onMount(load);

  async function load(): Promise<void> {
    loading = true;
    stats = await getStats();
    loading = false;
  }

  // Combined media byte breakdown (chat attachments + saved copies) so the bar
  // reflects everything the blob store holds by kind. Values in bytes.
  $: media = stats
    ? {
        image: stats.attachments.image_bytes + stats.saved.image_bytes,
        video: stats.attachments.video_bytes + stats.saved.video_bytes,
        audio: stats.attachments.audio_bytes + stats.saved.audio_bytes,
        other: stats.attachments.other_bytes + stats.saved.other_bytes,
      }
    : { image: 0, video: 0, audio: 0, other: 0 };

  // One storage bar over everything this instance stores: media by kind +
  // recordings + the DB. Each segment → its share of the total + swatch color.
  $: segments = [
    { key: "statImages", bytes: media.image, color: "bg-beacon" },
    { key: "statVideos", bytes: media.video, color: "bg-sky-400" },
    { key: "statAudio", bytes: media.audio, color: "bg-emerald-400" },
    { key: "statOther", bytes: media.other, color: "bg-zinc-500" },
    { key: "statRecordings", bytes: stats?.recordings.bytes ?? 0, color: "bg-violet-400" },
    { key: "statDb", bytes: stats?.storage.db_bytes ?? 0, color: "bg-fuchsia-500" },
  ] as const;
  // Total the bar is drawn against = everything we occupy.
  $: used = segments.reduce((a, s) => a + s.bytes, 0);

  function pct(n: number): number {
    return used > 0 ? (n / used) * 100 : 0;
  }

  // Free space on the data volume (docker mount); null on non-unix servers.
  // Shown as a trailing legend chip — not a bar segment, since it's disk-scale.
  $: freeBytes = stats && stats.storage.fs_avail !== null ? stats.storage.fs_avail : null;

  function asOf(ms: number): string {
    return new Date(ms).toLocaleString();
  }
</script>

<section class="mb-8 max-w-2xl">
  <div class="mb-3 flex items-baseline gap-3">
    <h2 class="text-[0.8rem] font-semibold uppercase tracking-[0.08em] text-muted">
      {$t("statistics")}
    </h2>
    {#if stats}
      <span class="font-mono text-[0.66rem] text-muted"
        >{$t("statAsOf")} {asOf(stats.generated_at)}</span
      >
    {/if}
    <button
      type="button"
      onclick={load}
      class="ml-auto inline-flex min-h-8 cursor-pointer items-center rounded-md border border-line px-2 font-mono text-[0.68rem] text-muted hover:border-beacon hover:text-beacon"
    >
      ↻ {$t("refresh")}
    </button>
  </div>

  {#if loading && !stats}
    <div class="grid grid-cols-2 gap-2 sm:grid-cols-3">
      {#each Array(6) as _}
        <div class="h-[4.6rem] animate-pulse rounded-md border border-line bg-surface-2"></div>
      {/each}
    </div>
  {:else if !stats}
    <p class="font-mono text-[0.82rem] text-muted">{$t("statNoData")}</p>
  {:else}
    <!-- Headline metrics: big mono number, quiet label, one line of context. -->
    <div class="grid grid-cols-2 gap-2 sm:grid-cols-3">
      <div class="rounded-md border border-line bg-surface-2 p-3">
        <div class="font-mono text-[1.5rem] leading-none text-beacon">
          {stats.messages.total.toLocaleString()}
        </div>
        <div class="mt-1 text-[0.72rem] text-muted">{$t("statMessages")}</div>
        <div class="mt-1 font-mono text-[0.64rem] text-muted">
          +{stats.messages.last_24h} {$t("statLast24h")} · +{stats.messages.last_7d}
          {$t("statLast7d")}
        </div>
      </div>

      <div class="rounded-md border border-line bg-surface-2 p-3">
        <div class="font-mono text-[1.5rem] leading-none text-text">
          {stats.principals.total}
        </div>
        <div class="mt-1 text-[0.72rem] text-muted">{$t("statMembers")}</div>
        <div class="mt-1 font-mono text-[0.64rem] text-muted">
          {stats.principals.users}
          {$t("statEmployees")} · {stats.principals.clients} {$t("statClients")}{#if stats.principals.bots}
            · {stats.principals.bots} {$t("statBots")}{/if}
        </div>
      </div>

      <div class="rounded-md border border-line bg-surface-2 p-3">
        <div class="font-mono text-[1.5rem] leading-none text-text">{stats.rooms.total}</div>
        <div class="mt-1 text-[0.72rem] text-muted">{$t("statRooms")}</div>
        <div class="mt-1 font-mono text-[0.64rem] text-muted">
          {stats.rooms.direct}
          {$t("statDirect")} · {stats.rooms.client} {$t("statClients")}
        </div>
      </div>

      <div class="rounded-md border border-line bg-surface-2 p-3">
        <div class="font-mono text-[1.5rem] leading-none text-text">
          {stats.attachments.count.toLocaleString()}
        </div>
        <div class="mt-1 text-[0.72rem] text-muted">{$t("statFiles")}</div>
        <div class="mt-1 font-mono text-[0.64rem] text-muted">{fmtBytes(stats.attachments.bytes)}</div>
      </div>

      <div class="rounded-md border border-line bg-surface-2 p-3">
        <div class="font-mono text-[1.5rem] leading-none text-text">{stats.reactions}</div>
        <div class="mt-1 text-[0.72rem] text-muted">{$t("statReactions")}</div>
      </div>

      <div class="rounded-md border border-line bg-surface-2 p-3">
        <div class="font-mono text-[1.5rem] leading-none text-text">{stats.calls.total}</div>
        <div class="mt-1 text-[0.72rem] text-muted">{$t("statCalls")}</div>
        {#if stats.calls.recorded}
          <div class="mt-1 font-mono text-[0.64rem] text-muted">
            {stats.calls.recorded}
            {$t("statRecorded")}
          </div>
        {/if}
      </div>
    </div>

    <!-- Footprint: what this instance costs to host. -->
    <div class="mt-2 grid grid-cols-2 gap-2 sm:grid-cols-3">
      <div class="rounded-md border border-line bg-surface-2 p-3">
        <div class="font-mono text-[1.05rem] leading-none text-text">{fmtBytes(stats.storage.db_bytes)}</div>
        <div class="mt-1 text-[0.72rem] text-muted">{$t("statDbSize")}</div>
      </div>
      {#if stats.storage.blobs_bytes !== null}
        <div class="rounded-md border border-line bg-surface-2 p-3">
          <div class="font-mono text-[1.05rem] leading-none text-text">
            {fmtBytes(stats.storage.blobs_bytes)}
          </div>
          <div class="mt-1 text-[0.72rem] text-muted">{$t("statDisk")}</div>
        </div>
      {/if}
      {#if stats.recordings.count > 0}
        <div class="rounded-md border border-line bg-surface-2 p-3">
          <div class="font-mono text-[1.05rem] leading-none text-text">
            {fmtBytes(stats.recordings.bytes)}
          </div>
          <div class="mt-1 text-[0.72rem] text-muted">
            {$t("statRecordings")}
            <span class="font-mono text-[0.64rem]">· {stats.recordings.count}</span>
          </div>
        </div>
      {/if}
      {#if stats.memory}
        <div class="rounded-md border border-line bg-surface-2 p-3">
          <div class="font-mono text-[1.05rem] leading-none text-text">
            {fmtBytes(stats.memory.rss_bytes)}
          </div>
          <div class="mt-1 text-[0.72rem] text-muted">{$t("statMemory")}</div>
        </div>
      {/if}
    </div>

    <!-- Signature element: one bar of everything stored (media + recordings +
         DB), with free disk space as a trailing legend chip. -->
    {#if used > 0}
      <div class="mt-4">
        <div class="mb-1.5 flex items-baseline justify-between">
          <span class="text-[0.72rem] uppercase tracking-[0.06em] text-muted">{$t("statStorage")}</span>
          <span class="font-mono text-[0.66rem] text-muted">{fmtBytes(used)}</span>
        </div>
        <div class="flex h-2.5 overflow-hidden rounded-full bg-surface-2">
          {#each segments as seg}
            {#if seg.bytes > 0}
              <div class={seg.color} style="width: {pct(seg.bytes)}%" title={$t(seg.key)}></div>
            {/if}
          {/each}
        </div>
        <div class="mt-2 flex flex-wrap gap-x-4 gap-y-1">
          {#each segments as seg}
            {#if seg.bytes > 0}
              <span class="inline-flex items-center gap-1.5 font-mono text-[0.66rem] text-muted">
                <span class="inline-block h-2 w-2 rounded-sm {seg.color}"></span>
                {$t(seg.key)}
                <span class="text-text">{fmtBytes(seg.bytes)}</span>
              </span>
            {/if}
          {/each}
          {#if freeBytes !== null}
            <span class="inline-flex items-center gap-1.5 font-mono text-[0.66rem] text-muted">
              <span class="inline-block h-2 w-2 rounded-sm bg-line ring-1 ring-inset ring-muted/40"></span>
              {$t("statDiskFree")}
              <span class="text-text">{fmtBytes(freeBytes)}</span>
            </span>
          {/if}
        </div>
      </div>
    {/if}
  {/if}
</section>
