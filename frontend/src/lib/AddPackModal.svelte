<script lang="ts">
  import { onMount } from "svelte";
  import { t } from "./i18n";
  import Sticker from "./Sticker.svelte";
  import {
    previewPack,
    addPackBySlug,
    isLottie,
    isVideoSticker,
    sharedItemUrl,
    addPackSlug,
    type Pack,
  } from "./packs";

  // Opened by a share link (?addpack=<slug>) or by tapping a sticker's pack: preview
  // the shared pack, offer to add it. Driven by the `addPackSlug` store.
  let slug: string | null = null;
  let pack: Pack | null = null;
  let state: "idle" | "loading" | "ready" | "adding" | "done" | "error" = "idle";

  function clearParam(): void {
    const u = new URL(location.href);
    if (u.searchParams.has("addpack")) {
      u.searchParams.delete("addpack");
      history.replaceState(null, "", u.pathname + u.search);
    }
  }

  // React to the store: load the pack when a slug arrives, reset when cleared.
  $: void openFor($addPackSlug);
  let loadedFor: string | null = null;
  async function openFor(s: string | null): Promise<void> {
    if (s === loadedFor) return;
    loadedFor = s;
    slug = s;
    if (!s) {
      pack = null;
      state = "idle";
      return;
    }
    state = "loading";
    pack = await previewPack(s);
    state = pack ? "ready" : "error";
  }

  onMount(() => {
    const s = new URLSearchParams(location.search).get("addpack");
    if (s) addPackSlug.set(s);
  });

  async function add(): Promise<void> {
    if (!slug) return;
    state = "adding";
    const added = await addPackBySlug(slug);
    state = added ? "done" : "error";
    if (added) setTimeout(close, 900);
  }

  function close(): void {
    clearParam();
    addPackSlug.set(null);
  }
</script>

{#if slug}
  <div
    class="fixed inset-0 z-50 grid place-items-center bg-black/60 p-4"
    role="dialog"
    aria-modal="true"
    aria-label={$t("addPackTitle")}
  >
    <div class="flex max-h-[80dvh] w-full max-w-sm flex-col gap-3 rounded-lg border border-line bg-ink p-4 shadow-xl">
      {#if state === "loading"}
        <p class="py-6 text-center font-mono text-[0.8rem] text-muted">…</p>
      {:else if state === "error"}
        <p class="py-6 text-center font-mono text-[0.8rem] text-bad">{$t("packNotFound")}</p>
        <button
          type="button"
          onclick={close}
          class="rounded border border-line px-3 py-1.5 font-mono text-[0.78rem] text-muted hover:border-beacon"
          >{$t("cancel")}</button
        >
      {:else if pack}
        <header class="flex items-baseline gap-2">
          <h2 class="font-mono text-sm text-you">{pack.name}</h2>
          <span class="font-mono text-[0.7rem] text-muted">{pack.items.length}</span>
        </header>
        <div class="grid grid-cols-4 gap-1.5 overflow-y-auto">
          {#each pack.items.slice(0, 16) as it (it.id)}
            <div class="grid aspect-square place-items-center rounded p-0.5">
              {#if isLottie(it.content_type)}
                <Sticker src={sharedItemUrl(pack.share_slug, it.id)} format="lottie" size={56} />
              {:else if isVideoSticker(it.content_type)}
                <Sticker src={sharedItemUrl(pack.share_slug, it.id)} format="webm" size={56} />
              {:else}
                <img
                  src={sharedItemUrl(pack.share_slug, it.id)}
                  alt={it.filename}
                  loading="lazy"
                  class="max-h-full max-w-full object-contain"
                />
              {/if}
            </div>
          {/each}
        </div>
        <div class="flex gap-2">
          <button
            type="button"
            onclick={close}
            disabled={state === "adding"}
            class="flex-1 rounded border border-line px-3 py-1.5 font-mono text-[0.78rem] text-muted hover:border-beacon disabled:opacity-60"
            >{$t("cancel")}</button
          >
          <button
            type="button"
            onclick={add}
            disabled={state === "adding" || state === "done"}
            class="flex-1 rounded border border-beacon bg-beacon px-3 py-1.5 font-mono text-[0.78rem] text-[#1a1206] disabled:opacity-60"
          >
            {state === "done" ? $t("packAdded") : $t("addPack")}
          </button>
        </div>
      {/if}
    </div>
  </div>
{/if}
