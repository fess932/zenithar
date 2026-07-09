<script lang="ts">
  // A thumbnail <img> with a built-in shimmer placeholder: shows a `.skeleton`
  // until the image decodes, then fades it in. Must be placed inside a
  // position:relative box (the skeleton fills it via inset-0) — every picker /
  // grid tile already is one.
  export let src: string;
  export let alt = "";
  let klass = "size-full object-cover";
  export { klass as class };

  let loaded = false;
</script>

{#if !loaded}
  <span class="skeleton pointer-events-none absolute inset-0"></span>
{/if}
<img
  {src}
  {alt}
  loading="lazy"
  onload={() => (loaded = true)}
  class="{klass} relative transition-opacity duration-200 {loaded ? '' : 'opacity-0'}"
/>
