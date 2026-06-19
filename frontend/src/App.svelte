<script lang="ts">
  import { onMount, tick } from "svelte";
  import Header from "./lib/Header.svelte";
  import Composer from "./lib/Composer.svelte";
  import Message from "./lib/Message.svelte";
  import { messages, connect } from "./lib/chat";
  import { t } from "./lib/i18n";

  let logEl: HTMLElement;
  let pinned = true;

  onMount(connect);

  function onScroll(): void {
    pinned = logEl.scrollHeight - logEl.scrollTop - logEl.clientHeight < 80;
  }

  // Keep the transcript pinned to the newest line when the user is at the bottom.
  $: if (logEl && $messages.length) {
    tick().then(() => {
      if (pinned) logEl.scrollTop = logEl.scrollHeight;
    });
  }
</script>

<div class="grid h-dvh grid-rows-[auto_1fr_auto] bg-ink font-sans text-[15px] text-text">
  <Header />

  <main
    bind:this={logEl}
    on:scroll={onScroll}
    class="overflow-y-auto py-3"
    aria-live="polite"
  >
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
