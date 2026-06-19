<script lang="ts">
  import { onMount, afterUpdate } from "svelte";
  import Header from "./Header.svelte";
  import Composer from "./Composer.svelte";
  import Message from "./Message.svelte";
  import Principals from "./Principals.svelte";
  import { messages, connect } from "./chat";
  import { t } from "./i18n";

  let logEl: HTMLElement;
  let pinned = true;
  let view: "chat" | "admin" = "chat";

  onMount(connect);

  function onScroll(): void {
    pinned = logEl.scrollHeight - logEl.scrollTop - logEl.clientHeight < 80;
  }

  // Keep pinned to the newest line after each DOM update (not a reactive
  // dependency, so updating scroll can't re-trigger itself).
  afterUpdate(() => {
    if (pinned && logEl) logEl.scrollTop = logEl.scrollHeight;
  });
</script>

{#if view === "admin"}
  <Principals onBack={() => (view = "chat")} />
{:else}
  <div class="grid h-dvh grid-rows-[auto_1fr_auto] bg-ink font-sans text-[15px] text-text">
    <Header onOpenAdmin={() => (view = "admin")} />

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
{/if}
