<script lang="ts">
  // A thumbnail <img> with a shimmer placeholder AND bounded-concurrency loading:
  // it waits for a slot from imageLoadPool (max N in flight) before fetching, so a
  // grid loads in steady batches instead of all-at-once or one-by-one. Must sit in
  // a position:relative box (the skeleton fills it via inset-0) — every tile is one.
  import { onMount, onDestroy } from "svelte";
  import { acquireLoad, releaseLoad, type LoadSlot } from "./imageLoadPool";

  export let src: string;
  export let alt = "";
  let klass = "size-full object-cover";
  export { klass as class };

  let shown = ""; // src actually handed to <img> — set only once a slot opens
  let loaded = false;
  let slot: LoadSlot | null = null;

  onMount(() => {
    slot = acquireLoad(() => (shown = src));
  });
  function done(): void {
    loaded = true;
    releaseLoad(slot);
    slot = null;
  }
  onDestroy(() => releaseLoad(slot));
</script>

{#if !loaded}
  <span class="skeleton pointer-events-none absolute inset-0"></span>
{/if}
{#if shown}
  <!-- Paints as soon as ready (instant for cached); transparent while loading, so
       the skeleton behind shows through. No per-image fade → no staggered look. -->
  <img src={shown} {alt} onload={done} onerror={done} class="{klass} relative" />
{/if}
