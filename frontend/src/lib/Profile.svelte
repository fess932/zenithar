<script lang="ts">
  import { onMount } from "svelte";
  import { fly, fade } from "svelte/transition";
  import { me } from "./session";
  import { t } from "./i18n";
  import Avatar from "./Avatar.svelte";
  import {
    listSaved,
    listSavedOf,
    setSavedPublic,
    deleteSaved,
    uploadSaved,
    savedThumb,
    savedUrl,
    type SavedItem,
  } from "./saved";
  import type { ProfileTarget } from "./profile";

  export let target: ProfileTarget;
  export let onClose: () => void;

  $: mine = $me?.id === target.id;
  $: isPhoto = !!target.avatar && target.avatar.startsWith("photo");

  let items: SavedItem[] = [];
  let loading = true;
  let full: { url: string; video: boolean } | null = null; // full-screen view target
  let fileInput: HTMLInputElement;

  async function onPicked(e: Event): Promise<void> {
    const input = e.target as HTMLInputElement;
    const files = Array.from(input.files ?? []);
    input.value = "";
    for (const file of files) {
      const it = await uploadSaved(file);
      if (it) items = [it, ...items];
    }
  }

  onMount(load);
  async function load(): Promise<void> {
    loading = true;
    items = mine ? await listSaved() : await listSavedOf(target.id);
    loading = false;
  }

  async function togglePublic(it: SavedItem): Promise<void> {
    const next = !it.public;
    if (await setSavedPublic(it.id, next)) {
      items = items.map((x) => (x.id === it.id ? { ...x, public: next } : x));
    }
  }
  async function remove(it: SavedItem): Promise<void> {
    if (!window.confirm($t("deleteConfirm"))) return;
    if (await deleteSaved(it.id)) items = items.filter((x) => x.id !== it.id);
  }

  function onKey(e: KeyboardEvent): void {
    if (e.key !== "Escape") return;
    if (full) full = null;
    else onClose();
  }
</script>

<svelte:window onkeydown={onKey} />

<div
  transition:fly={{ x: 24, duration: 160 }}
  class="fixed inset-0 z-50 flex flex-col overflow-y-auto bg-ink text-text"
>
  <!-- top bar -->
  <header
    class="sticky top-0 z-10 flex items-center gap-2 border-b border-line bg-surface px-3 pt-[calc(0.7rem+env(safe-area-inset-top))] pb-[0.7rem] sm:px-5"
  >
    <button
      type="button"
      onclick={onClose}
      aria-label={$t("back")}
      class="-ml-1.5 grid size-9 shrink-0 cursor-pointer place-items-center rounded-md text-muted hover:text-text"
    >
      <span class="text-2xl leading-none">‹</span>
    </button>
    <span class="truncate font-mono text-[0.9rem] text-text">{$t("profile")}</span>
  </header>

  <!-- identity -->
  <div class="flex flex-col items-center gap-2 px-4 py-6">
    <button
      type="button"
      onclick={() => isPhoto && (full = { url: `/api/avatars/${target.id}`, video: false })}
      class:cursor-zoom-in={isPhoto}
      class:cursor-default={!isPhoto}
      aria-label={target.name}
    >
      <Avatar id={target.id} name={target.name} avatar={target.avatar} size={144} />
    </button>
    <div class="font-mono text-[1.05rem] text-you">{target.name}</div>
  </div>

  <!-- saved grid -->
  <div class="px-3 pb-[calc(1rem+env(safe-area-inset-bottom))] sm:px-5">
    <div class="mb-2 px-1 font-mono text-[0.72rem] uppercase tracking-[0.1em] text-muted">
      {$t("saved")}
    </div>
    {#if loading}
      <p class="px-1 py-6 font-mono text-[0.78rem] text-muted">…</p>
    {:else if !mine && items.length === 0}
      <p class="px-1 py-6 font-mono text-[0.78rem] text-muted">{$t("noPublicSaved")}</p>
    {:else}
      <div class="grid grid-cols-[repeat(auto-fill,minmax(6rem,1fr))] gap-2">
        {#if mine}
          <!-- Upload straight into your saved collection. -->
          <button
            type="button"
            onclick={() => fileInput.click()}
            aria-label={$t("uploadSaved")}
            title={$t("uploadSaved")}
            class="grid aspect-square cursor-pointer place-items-center rounded-md border border-dashed border-line text-3xl text-muted hover:border-beacon hover:text-beacon"
          >
            +
          </button>
        {/if}
        {#each items as it (it.id)}
          <div class="relative aspect-square overflow-hidden rounded-md border border-line">
            <button
              type="button"
              onclick={() =>
                (full = { url: savedUrl(it.id), video: it.content_type.startsWith("video/") })}
              class="block size-full cursor-zoom-in"
            >
              {#if it.content_type.startsWith("video/")}
                <!-- svelte-ignore a11y_media_has_caption -->
                <video
                  src={`${savedUrl(it.id)}#t=0.1`}
                  muted
                  playsinline
                  preload="metadata"
                  class="size-full object-cover"
                ></video>
                <span class="pointer-events-none absolute inset-0 grid place-items-center text-white/90">▶</span>
              {:else}
                <img
                  src={it.has_thumb ? savedThumb(it.id) : savedUrl(it.id)}
                  alt={it.filename}
                  loading="lazy"
                  class="size-full object-cover"
                />
              {/if}
            </button>
            {#if mine}
              <button
                type="button"
                onclick={() => togglePublic(it)}
                title={it.public ? $t("makePrivate") : $t("makePublic")}
                aria-pressed={it.public}
                class="absolute left-1 top-1 grid size-7 cursor-pointer place-items-center rounded-full bg-black/55 text-sm backdrop-blur-sm hover:bg-black/75"
              >
                {it.public ? "🌐" : "🔒"}
              </button>
              <button
                type="button"
                onclick={() => remove(it)}
                aria-label={$t("delete")}
                class="absolute right-1 top-1 grid size-7 cursor-pointer place-items-center rounded-full bg-black/55 text-sm backdrop-blur-sm hover:bg-black/75 hover:text-bad"
              >
                🗑
              </button>
            {/if}
          </div>
        {/each}
      </div>
    {/if}
    {#if mine}
      <input
        bind:this={fileInput}
        type="file"
        accept="image/*"
        multiple
        onchange={onPicked}
        class="hidden"
        aria-hidden="true"
      />
    {/if}
  </div>
</div>

<!-- full-screen view (image or video) -->
{#if full}
  <div transition:fade={{ duration: 120 }} class="fixed inset-0 z-[60] grid place-items-center bg-black/85 p-4">
    <button
      type="button"
      onclick={() => (full = null)}
      aria-label={$t("close")}
      class="absolute inset-0 cursor-zoom-out"
    ></button>
    {#if full.video}
      <!-- svelte-ignore a11y_media_has_caption -->
      <video
        src={full.url}
        controls
        autoplay
        playsinline
        class="relative max-h-full max-w-full rounded-md object-contain"
      ></video>
    {:else}
      <img src={full.url} alt="" class="relative max-h-full max-w-full rounded-md object-contain" />
    {/if}
  </div>
{/if}
