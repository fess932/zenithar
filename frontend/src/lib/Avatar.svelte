<script lang="ts">
  // A round avatar. `avatar` is the stored value: an emoji, "photo:<ver>", or
  // null/empty → a default emoji + tint derived deterministically from the id, so
  // every person has a stable, distinct face even before they pick one.
  export let id: string;
  export let name = "";
  export let avatar: string | null | undefined = null;
  export let size = 36; // px

  const DEFAULTS = [
    "🦊","🐱","🐼","🦉","🐙","🐝","🦋","🐬",
    "🦜","🐢","🦔","🐧","🦦","🐳","🦚","🐲",
  ];
  // Muted tints that sit well on the dark surface (paired with the emoji set).
  const BGS = [
    "#3b4a63","#5a4636","#3f5a4a","#5a3f54",
    "#444a5e","#4a5a3f","#5e4444","#3f5560",
  ];

  function hash(s: string): number {
    let h = 0;
    for (let i = 0; i < s.length; i++) h = (h * 31 + s.charCodeAt(i)) >>> 0;
    return h;
  }

  $: h = hash(id || name || "?");
  $: isPhoto = !!avatar && avatar.startsWith("photo");
  $: ver = isPhoto ? (avatar!.split(":")[1] ?? "0") : "0";
  $: emoji = avatar && !isPhoto ? avatar : DEFAULTS[h % DEFAULTS.length];
  $: bg = BGS[h % BGS.length];
</script>

{#if isPhoto}
  <img
    src={`/api/avatars/${id}?v=${ver}`}
    alt={name}
    class="shrink-0 rounded-full border border-line object-cover"
    style="width:{size}px;height:{size}px"
  />
{:else}
  <span
    class="grid shrink-0 place-items-center rounded-full leading-none"
    style="width:{size}px;height:{size}px;background:{bg};font-size:{Math.round(size * 0.55)}px"
    title={name}
    aria-label={name}
  >
    {emoji}
  </span>
{/if}
