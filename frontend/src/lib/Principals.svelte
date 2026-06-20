<script lang="ts">
  import { onMount } from "svelte";
  import { t } from "./i18n";
  import VoicePlayer from "./VoicePlayer.svelte";
  import {
    listPrincipals,
    createPrincipal,
    rotateLink,
    revokeLink,
    listIntegrations,
    createIntegration,
    rotateIntegration,
    revokeIntegration,
    listRecordings,
    type PrincipalSummary,
    type IntegrationSummary,
    type Recording,
    type Link,
  } from "./session";

  export let onBack: () => void;

  let rows: PrincipalSummary[] = [];
  let kind: "user" | "client" = "client";
  let name = "";
  let fresh: { url: string; label: string } | null = null;
  let copied = false;

  // integrations
  let integrations: IntegrationSummary[] = [];
  let intName = "";
  let freshToken: string | null = null;
  let tokenCopied = false;

  // call recordings
  let recordings: Recording[] = [];

  onMount(refresh);

  async function refresh(): Promise<void> {
    [rows, integrations, recordings] = await Promise.all([
      listPrincipals(),
      listIntegrations(),
      listRecordings(),
    ]);
  }

  function fmtWhen(ms: number): string {
    return new Date(ms).toLocaleString();
  }
  function fmtLen(started: number, ended: number | null): string {
    if (ended === null) return "";
    const s = Math.max(0, Math.round((ended - started) / 1000));
    return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
  }

  async function addIntegration(): Promise<void> {
    const created = await createIntegration(intName.trim() || "API");
    if (created) {
      freshToken = created.token;
      tokenCopied = false;
    }
    intName = "";
    await refresh();
  }
  async function rotateInt(id: string): Promise<void> {
    const t = await rotateIntegration(id);
    if (t) {
      freshToken = t.token;
      tokenCopied = false;
    }
    await refresh();
  }
  async function revokeInt(id: string): Promise<void> {
    await revokeIntegration(id);
    freshToken = null;
    await refresh();
  }
  async function copyToken(): Promise<void> {
    if (!freshToken) return;
    await navigator.clipboard.writeText(freshToken);
    tokenCopied = true;
  }
  function fmtUsed(t: number | null, never: string): string {
    return t === null ? never : new Date(t).toLocaleString();
  }

  function fullUrl(path: string): string {
    return location.origin + path;
  }

  function showLink(link: Link | null, label: string): void {
    if (link) fresh = { url: fullUrl(link.url), label };
    copied = false;
  }

  async function create(): Promise<void> {
    showLink(await createPrincipal(kind, name), name || kind);
    name = "";
    await refresh();
  }
  async function rotate(id: string): Promise<void> {
    showLink(await rotateLink(id), id);
    await refresh();
  }
  async function revoke(id: string): Promise<void> {
    await revokeLink(id);
    if (fresh) fresh = null;
    await refresh();
  }
  async function copy(): Promise<void> {
    if (!fresh) return;
    await navigator.clipboard.writeText(fresh.url);
    copied = true;
  }
</script>

<div class="grid h-dvh grid-rows-[auto_1fr] bg-ink text-text">
  <header
    class="flex items-center gap-3.5 border-b border-line bg-surface px-3 pt-[calc(0.7rem+env(safe-area-inset-top))] pb-[0.7rem] sm:px-5"
  >
    <button
      type="button"
      onclick={onBack}
      class="-ml-1 inline-flex min-h-9 cursor-pointer items-center px-1 font-mono text-[0.72rem] uppercase tracking-[0.08em] text-muted hover:text-text"
    >
      ← {$t("back")}
    </button>
    <span class="font-mono text-[0.78rem] text-muted">{$t("adminLinks")}</span>
  </header>

  <main class="overflow-y-auto px-3 py-4 sm:px-5">
    <!-- create -->
    <section class="mb-6 max-w-2xl">
      <h2 class="mb-2 text-[0.8rem] font-semibold uppercase tracking-[0.08em] text-muted">
        {$t("newLink")}
      </h2>
      <div class="flex flex-wrap items-center gap-2">
        <select
          bind:value={kind}
          class="min-h-11 rounded-md border border-line bg-surface-2 px-2 font-mono text-[0.85rem] text-text"
        >
          <option value="client">{$t("roleClient")}</option>
          <option value="user">{$t("roleUser")}</option>
        </select>
        <input
          bind:value={name}
          placeholder={$t("nameOptional")}
          maxlength="40"
          class="min-h-11 min-w-0 flex-1 rounded-md border border-line bg-surface-2 px-3 text-base text-text placeholder:text-muted focus-visible:outline-2 focus-visible:outline-beacon sm:text-[0.9rem]"
        />
        <button
          type="button"
          onclick={create}
          class="min-h-11 w-full cursor-pointer rounded-md border border-beacon bg-beacon px-4 font-semibold text-[#1a1206] hover:brightness-110 sm:w-auto"
        >
          {$t("create")}
        </button>
      </div>

      {#if fresh}
        <div class="mt-3 rounded-md border border-line bg-surface-2 p-3">
          <div class="mb-1 font-mono text-[0.7rem] uppercase tracking-[0.06em] text-muted">
            {$t("freshLink")}
          </div>
          <div class="flex items-center gap-2">
            <code class="flex-1 truncate text-[0.8rem] text-beacon">{fresh.url}</code>
            <button
              type="button"
              onclick={copy}
              class="cursor-pointer rounded-md border border-line px-2 py-1 font-mono text-[0.72rem] text-muted hover:text-text"
            >
              {copied ? $t("copied") : $t("copy")}
            </button>
          </div>
        </div>
      {/if}
    </section>

    <!-- list -->
    {#if rows.length === 0}
      <p class="font-mono text-[0.82rem] text-muted">{$t("noPrincipals")}</p>
    {:else}
      <ul class="max-w-2xl divide-y divide-line">
        {#each rows as p (p.id)}
          <li class="flex flex-col gap-2 py-3 sm:flex-row sm:items-center sm:gap-3 sm:py-2">
            <div class="flex min-w-0 items-center gap-3 sm:flex-1">
              <span
                class="font-mono text-[0.7rem] uppercase tracking-[0.06em]"
                class:text-you={p.kind === "user"}
                class:text-muted={p.kind !== "user"}
              >
                {p.kind === "user" ? $t("roleUser") : $t("roleClient")}
              </span>
              <span class="min-w-0 flex-1 truncate text-[0.9rem]">{p.display_name}</span>
              <span
                class="font-mono text-[0.7rem]"
                class:text-beacon={p.active}
                class:text-muted={!p.active}
              >
                {p.active ? $t("active") : $t("revoked")}
              </span>
            </div>
            <div class="flex gap-1 self-end sm:self-auto">
              <button
                type="button"
                onclick={() => rotate(p.id)}
                class="inline-flex min-h-9 cursor-pointer items-center rounded-md px-2 font-mono text-[0.72rem] text-muted hover:bg-surface-2 hover:text-text"
              >
                {$t("rotate")}
              </button>
              <button
                type="button"
                onclick={() => revoke(p.id)}
                class="inline-flex min-h-9 cursor-pointer items-center rounded-md px-2 font-mono text-[0.72rem] text-muted hover:bg-surface-2 hover:text-bad"
              >
                {$t("revoke")}
              </button>
            </div>
          </li>
        {/each}
      </ul>
    {/if}

    <!-- integrations (REST API tokens) -->
    <section class="mt-10 max-w-2xl">
      <h2 class="mb-2 text-[0.8rem] font-semibold uppercase tracking-[0.08em] text-muted">
        {$t("integrations")}
      </h2>
      <div class="flex flex-wrap items-center gap-2">
        <input
          bind:value={intName}
          placeholder={$t("integrationName")}
          maxlength="40"
          class="min-h-11 min-w-0 flex-1 rounded-md border border-line bg-surface-2 px-3 text-base text-text placeholder:text-muted focus-visible:outline-2 focus-visible:outline-beacon sm:text-[0.9rem]"
        />
        <button
          type="button"
          onclick={addIntegration}
          class="min-h-11 w-full cursor-pointer rounded-md border border-beacon bg-beacon px-4 font-semibold text-[#1a1206] hover:brightness-110 sm:w-auto"
        >
          {$t("create")}
        </button>
      </div>

      {#if freshToken}
        <div class="mt-3 rounded-md border border-line bg-surface-2 p-3">
          <div class="mb-1 font-mono text-[0.7rem] uppercase tracking-[0.06em] text-muted">
            {$t("freshToken")}
          </div>
          <div class="flex items-center gap-2">
            <code class="flex-1 truncate text-[0.8rem] text-beacon">{freshToken}</code>
            <button
              type="button"
              onclick={copyToken}
              class="cursor-pointer rounded-md border border-line px-2 py-1 font-mono text-[0.72rem] text-muted hover:text-text"
            >
              {tokenCopied ? $t("copied") : $t("copy")}
            </button>
          </div>
        </div>
      {/if}

      {#if integrations.length === 0}
        <p class="mt-3 font-mono text-[0.82rem] text-muted">{$t("noIntegrations")}</p>
      {:else}
        <ul class="mt-3 divide-y divide-line">
          {#each integrations as it (it.id)}
            <li class="flex flex-col gap-2 py-3 sm:flex-row sm:items-center sm:gap-3 sm:py-2">
              <div class="flex min-w-0 items-center gap-3 sm:flex-1">
                <span class="min-w-0 flex-1 truncate text-[0.9rem]">{it.name}</span>
                <span class="shrink-0 font-mono text-[0.68rem] text-muted">
                  {$t("lastUsed")}: {fmtUsed(it.last_used_at, $t("neverUsed"))}
                </span>
                <span
                  class="shrink-0 font-mono text-[0.7rem]"
                  class:text-beacon={it.active}
                  class:text-muted={!it.active}
                >
                  {it.active ? $t("active") : $t("revoked")}
                </span>
              </div>
              <div class="flex gap-1 self-end sm:self-auto">
                <button
                  type="button"
                  onclick={() => rotateInt(it.id)}
                  class="inline-flex min-h-9 cursor-pointer items-center rounded-md px-2 font-mono text-[0.72rem] text-muted hover:bg-surface-2 hover:text-text"
                >
                  {$t("rotate")}
                </button>
                <button
                  type="button"
                  onclick={() => revokeInt(it.id)}
                  class="inline-flex min-h-9 cursor-pointer items-center rounded-md px-2 font-mono text-[0.72rem] text-muted hover:bg-surface-2 hover:text-bad"
                >
                  {$t("revoke")}
                </button>
              </div>
            </li>
          {/each}
        </ul>
      {/if}
    </section>

    <!-- call recordings -->
    <section class="mt-10 max-w-2xl">
      <h2 class="mb-2 text-[0.8rem] font-semibold uppercase tracking-[0.08em] text-muted">
        {$t("recordings")}
      </h2>

      {#if recordings.length === 0}
        <p class="font-mono text-[0.82rem] text-muted">{$t("noRecordings")}</p>
      {:else}
        <ul class="flex flex-col gap-3">
          {#each recordings as rec (rec.call_id)}
            <li class="rounded-md border border-line bg-surface-2 p-3">
              <div class="mb-2 flex flex-wrap items-baseline gap-x-3 gap-y-1">
                <span class="text-[0.9rem] font-semibold text-text">
                  {rec.room_title ?? $t("commonRoom")}
                </span>
                <span class="font-mono text-[0.72rem] text-muted">{fmtWhen(rec.started_at)}</span>
                {#if fmtLen(rec.started_at, rec.ended_at)}
                  <span class="font-mono text-[0.72rem] text-muted">
                    · {fmtLen(rec.started_at, rec.ended_at)}
                  </span>
                {/if}
                {#if rec.started_by_name}
                  <span class="font-mono text-[0.72rem] text-muted">
                    · {$t("startedBy")}: {rec.started_by_name}
                  </span>
                {/if}
              </div>
              <div class="flex flex-col gap-2">
                {#each rec.tracks as tr (tr.participant_id)}
                  <div class="flex flex-col gap-1">
                    <span class="font-mono text-[0.72rem] text-muted">🎙 {tr.participant_name}</span>
                    <VoicePlayer src={tr.url} />
                  </div>
                {/each}
              </div>
            </li>
          {/each}
        </ul>
      {/if}
    </section>
  </main>
</div>
