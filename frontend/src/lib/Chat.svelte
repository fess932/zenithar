<script lang="ts">
  import { onMount, afterUpdate } from "svelte";
  import Header from "./Header.svelte";
  import Composer from "./Composer.svelte";
  import Message from "./Message.svelte";
  import Principals from "./Principals.svelte";
  import { messages, connect, rooms, activeRoom, joinRoom, loadRooms, type RoomSummary } from "./chat";
  import { me } from "./session";
  import { t } from "./i18n";

  let logEl: HTMLElement;
  let pinned = true;
  let view: "chat" | "admin" = "chat";
  let drawerOpen = false;

  $: isEmployee = $me?.kind === "user";

  onMount(() => {
    connect();
    if (isEmployee) loadRooms();
  });

  function onScroll(): void {
    pinned = logEl.scrollHeight - logEl.scrollTop - logEl.clientHeight < 80;
  }

  // Keep pinned to the newest line after each DOM update (not a reactive
  // dependency, so updating scroll can't re-trigger itself).
  afterUpdate(() => {
    if (pinned && logEl) logEl.scrollTop = logEl.scrollHeight;
  });

  function roomLabel(r: RoomSummary): string {
    return r.kind === "common" ? $t("room") : (r.title ?? "—");
  }

  $: current = $rooms.find((r) => r.id === $activeRoom) ?? null;
  $: currentTitle = current ? roomLabel(current) : $t("room");

  function openDrawer(): void {
    drawerOpen = true;
    loadRooms(); // refresh in case new client rooms appeared
  }
  function pick(id: string): void {
    joinRoom(id);
    drawerOpen = false;
  }
</script>

{#if view === "admin"}
  <Principals onBack={() => (view = "chat")} />
{:else}
  <div
    class="grid h-dvh bg-ink font-sans text-[15px] text-text {isEmployee
      ? 'grid-rows-[auto_auto_1fr_auto]'
      : 'grid-rows-[auto_1fr_auto]'}"
  >
    <Header onOpenAdmin={() => (view = "admin")} />

    {#if isEmployee}
      <div class="flex items-center gap-2 border-b border-line bg-surface px-3 py-2 sm:px-5">
        <button
          type="button"
          onclick={openDrawer}
          class="inline-flex min-h-9 cursor-pointer items-center gap-2 rounded-md border border-line px-2 text-muted hover:text-text"
          aria-label={$t("rooms")}
        >
          <span class="text-base leading-none">☰</span>
          <span class="max-w-[60vw] truncate font-mono text-[0.8rem] text-text">{currentTitle}</span>
        </button>
      </div>
    {/if}

    <main bind:this={logEl} onscroll={onScroll} class="overflow-y-auto py-3" aria-live="polite">
      {#if $messages.length === 0}
        <p class="px-6 py-10 font-mono text-[0.82rem] text-muted">{$t("empty")}</p>
      {:else}
        {#each $messages as m (m.id)}
          <Message {m} />
        {/each}
      {/if}
    </main>

    <Composer />
  </div>

  <!-- Rooms drawer (employees) -->
  {#if drawerOpen}
    <button
      type="button"
      aria-label={$t("rooms")}
      onclick={() => (drawerOpen = false)}
      class="fixed inset-0 z-30 cursor-default bg-black/40"
    ></button>
    <aside
      class="fixed left-0 top-0 z-40 flex h-dvh w-72 max-w-[85vw] flex-col gap-1 overflow-y-auto border-r border-line bg-surface p-3 pt-[calc(0.75rem+env(safe-area-inset-top))]"
    >
      <div class="mb-2 px-1 font-mono text-[0.72rem] uppercase tracking-[0.1em] text-muted">
        {$t("rooms")}
      </div>
      {#if $rooms.length === 0}
        <p class="px-1 font-mono text-[0.8rem] text-muted">{$t("noRooms")}</p>
      {:else}
        {#each $rooms as r (r.id)}
          <button
            type="button"
            onclick={() => pick(r.id)}
            aria-current={r.id === $activeRoom}
            class="flex items-center gap-2 rounded-md px-3 py-2 text-left text-[0.9rem] hover:bg-surface-2 aria-[current=true]:bg-surface-2 aria-[current=true]:text-beacon"
          >
            <span
              class="size-1.5 shrink-0 rounded-full"
              class:bg-beacon={r.kind === "common"}
              class:bg-you={r.kind === "client"}
            ></span>
            <span class="truncate">{roomLabel(r)}</span>
          </button>
        {/each}
      {/if}
    </aside>
  {/if}
{/if}
