<script lang="ts">
  import { onMount } from "svelte";
  import { t } from "./i18n";
  import {
    listPrincipals,
    createPrincipal,
    rotateLink,
    revokeLink,
    type PrincipalSummary,
    type Link,
  } from "./session";

  export let onBack: () => void;

  let rows: PrincipalSummary[] = [];
  let kind: "user" | "client" = "client";
  let name = "";
  let fresh: { url: string; label: string } | null = null;
  let copied = false;

  onMount(refresh);

  async function refresh(): Promise<void> {
    rows = await listPrincipals();
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
  <header class="flex items-center gap-3.5 border-b border-line bg-surface px-5 py-[0.9rem]">
    <button
      type="button"
      onclick={onBack}
      class="cursor-pointer font-mono text-[0.72rem] uppercase tracking-[0.08em] text-muted hover:text-text"
    >
      ← {$t("back")}
    </button>
    <span class="font-mono text-[0.78rem] text-muted">{$t("adminLinks")}</span>
  </header>

  <main class="overflow-y-auto px-5 py-4">
    <!-- create -->
    <section class="mb-6 max-w-2xl">
      <h2 class="mb-2 text-[0.8rem] font-semibold uppercase tracking-[0.08em] text-muted">
        {$t("newLink")}
      </h2>
      <div class="flex flex-wrap items-center gap-2">
        <select
          bind:value={kind}
          class="rounded-md border border-line bg-surface-2 px-2 py-[0.45rem] font-mono text-[0.8rem] text-text"
        >
          <option value="client">{$t("roleClient")}</option>
          <option value="user">{$t("roleUser")}</option>
        </select>
        <input
          bind:value={name}
          placeholder={$t("nameOptional")}
          maxlength="40"
          class="flex-1 rounded-md border border-line bg-surface-2 px-3 py-[0.45rem] text-[0.85rem] text-text placeholder:text-muted focus-visible:outline-2 focus-visible:outline-beacon"
        />
        <button
          type="button"
          onclick={create}
          class="cursor-pointer rounded-md border border-beacon bg-beacon px-3 py-[0.45rem] font-semibold text-[#1a1206] hover:brightness-110"
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
          <li class="flex items-center gap-3 py-2">
            <span
              class="font-mono text-[0.7rem] uppercase tracking-[0.06em]"
              class:text-you={p.kind === "user"}
              class:text-muted={p.kind !== "user"}
            >
              {p.kind === "user" ? $t("roleUser") : $t("roleClient")}
            </span>
            <span class="flex-1 truncate text-[0.9rem]">{p.display_name}</span>
            <span
              class="font-mono text-[0.7rem]"
              class:text-beacon={p.active}
              class:text-muted={!p.active}
            >
              {p.active ? $t("active") : $t("revoked")}
            </span>
            <button
              type="button"
              onclick={() => rotate(p.id)}
              class="cursor-pointer font-mono text-[0.72rem] text-muted hover:text-text"
            >
              {$t("rotate")}
            </button>
            <button
              type="button"
              onclick={() => revoke(p.id)}
              class="cursor-pointer font-mono text-[0.72rem] text-muted hover:text-bad"
            >
              {$t("revoke")}
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  </main>
</div>
