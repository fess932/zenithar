<script lang="ts">
  import { onMount } from "svelte";
  import { me, loaded, loadMe } from "./lib/session";
  import Chat from "./lib/Chat.svelte";
  import Landing from "./lib/Landing.svelte";

  onMount(() => {
    // The cookie is already set by GET /i/:token; drop the token from the URL.
    if (location.pathname.startsWith("/i/")) {
      history.replaceState(null, "", "/");
    }
    loadMe();
  });
</script>

{#if !$loaded}
  <div class="h-dvh bg-ink"></div>
{:else if $me}
  <Chat />
{:else}
  <Landing />
{/if}
