<script lang="ts">
  import { t } from "./i18n";
  import Sticker from "./Sticker.svelte";
  import Thumb from "./Thumb.svelte";
  import PickerTile from "./PickerTile.svelte";
  import { type SavedItem } from "./saved";
  import {
    type Pack,
    type PackKind,
    isLottie,
    isVideoSticker,
    packItemUrl,
    packShareUrl,
    importPack,
    deletePack,
    deletePackItem,
    setPackKind,
    setPackPublic,
    convertPack,
    packHasWebm,
  } from "./packs";

  export let packs: Pack[];
  // Tapping an item sends it (picker); omit in a management-only view (profile).
  export let onSend: ((itemId: string) => void) | undefined = undefined;
  export let onChanged: (pack?: Pack) => void;

  const KINDS: { k: PackKind; label: () => string }[] = [
    { k: "sticker", label: () => $t("kindSticker") },
    { k: "gif", label: () => $t("kindGif") },
    { k: "saved", label: () => $t("kindSaved") },
  ];

  async function changeKind(p: Pack, k: PackKind): Promise<void> {
    if (p.kind === k) return;
    if (await setPackKind(p.id, k)) onChanged();
  }

  async function togglePublic(p: Pack): Promise<void> {
    if (await setPackPublic(p.id, !p.public)) onChanged();
  }

  let importing = false;
  let fileInput: HTMLInputElement;
  let sharedSlug: string | null = null; // which pack just got its link copied

  async function onImport(e: Event): Promise<void> {
    const input = e.target as HTMLInputElement;
    const file = input.files?.[0];
    input.value = "";
    if (!file) return;
    importing = true;
    // No kind → the server infers it from the archive contents. The pack may land
    // under a different sub-list; onChanged switches to it so it's not "lost".
    const pack = await importPack(file);
    importing = false;
    if (pack) onChanged(pack);
  }

  async function share(p: Pack): Promise<void> {
    const url = packShareUrl(p.share_slug);
    try {
      if (navigator.share) {
        await navigator.share({ title: p.name, url });
        return;
      }
    } catch {
      /* user dismissed the share sheet — fall through to copy */
    }
    try {
      await navigator.clipboard.writeText(url);
      sharedSlug = p.share_slug;
      setTimeout(() => (sharedSlug = sharedSlug === p.share_slug ? null : sharedSlug), 1600);
    } catch {
      /* clipboard blocked — nothing else we can do silently */
    }
  }

  async function removePack(p: Pack): Promise<void> {
    if (!confirm($t("deletePackConfirm"))) return;
    if (await deletePack(p.id)) onChanged();
  }

  async function removeItem(p: Pack, it: SavedItem): Promise<void> {
    if (await deletePackItem(p.id, it.id)) onChanged();
  }

  // Convert an old WebM pack to the lighter animated-WebP format (server-side,
  // via the transcoder sidecar). Disabled while a pack is mid-conversion.
  let converting = new Set<string>();
  async function convert(p: Pack): Promise<void> {
    converting = new Set(converting).add(p.id);
    const updated = await convertPack(p.id);
    const next = new Set(converting);
    next.delete(p.id);
    converting = next;
    if (updated) onChanged(updated);
  }

  // Toggle a per-pack "manage" mode that reveals delete affordances on items.
  let managing = new Set<string>();
  function toggleManage(id: string): void {
    managing = new Set(managing);
    if (managing.has(id)) managing.delete(id);
    else managing.add(id);
  }
</script>

<div class="flex flex-col gap-3">
  <!-- Import a whole pack (.wastickers / .zip / .tgs) -->
  <button
    type="button"
    onclick={() => fileInput.click()}
    disabled={importing}
    class="flex items-center justify-center gap-2 rounded border border-dashed border-line px-2 py-1.5 font-mono text-[0.74rem] text-muted hover:border-beacon hover:text-beacon disabled:opacity-60"
  >
    {#if importing}
      <span
        class="size-3.5 animate-spin rounded-full border-2 border-line border-t-beacon motion-reduce:animate-none"
      ></span>
      {$t("importing")}
    {:else}
      + {$t("importPack")}
    {/if}
  </button>
  <input
    bind:this={fileInput}
    type="file"
    accept=".wastickers,.zip,.tgs,.webm,application/zip,image/*,video/webm"
    onchange={onImport}
    class="hidden"
  />

  {#if packs.length === 0}
    <p class="px-1 py-2 font-mono text-[0.74rem] leading-snug text-muted">{$t("noPacks")}</p>
  {/if}

  {#each packs as p (p.id)}
    <section class="flex flex-col gap-1">
      <header class="flex items-center gap-1.5 px-0.5">
        <span class="truncate font-mono text-[0.72rem] text-you">{p.name}</span>
        <span class="font-mono text-[0.66rem] text-muted">{p.items.length}</span>
        <div class="ml-auto flex items-center gap-0.5">
          {#if packHasWebm(p)}
            <button
              type="button"
              onclick={() => convert(p)}
              disabled={converting.has(p.id)}
              title={$t("convertPack")}
              aria-label={$t("convertPack")}
              class="grid size-6 place-items-center rounded text-muted hover:bg-surface hover:text-beacon disabled:opacity-50"
            >
              {converting.has(p.id) ? "⏳" : "♻"}
            </button>
          {/if}
          <button
            type="button"
            onclick={() => togglePublic(p)}
            title={p.public ? $t("makePrivate") : $t("makePublic")}
            aria-pressed={p.public}
            class="grid size-6 place-items-center rounded text-muted hover:bg-surface"
          >
            {p.public ? "🌐" : "🔒"}
          </button>
          <button
            type="button"
            onclick={() => share(p)}
            title={$t("sharePack")}
            aria-label={$t("sharePack")}
            class="grid size-6 place-items-center rounded text-muted hover:bg-surface hover:text-beacon"
          >
            {sharedSlug === p.share_slug ? "✓" : "🔗"}
          </button>
          <button
            type="button"
            onclick={() => toggleManage(p.id)}
            title={$t("editPack")}
            aria-label={$t("editPack")}
            class="grid size-6 place-items-center rounded text-muted hover:bg-surface hover:text-you"
            class:text-you={managing.has(p.id)}
          >
            ✎
          </button>
          <button
            type="button"
            onclick={() => removePack(p)}
            title={$t("deletePack")}
            aria-label={$t("deletePack")}
            class="grid size-6 place-items-center rounded text-muted hover:bg-surface hover:text-bad"
          >
            🗑
          </button>
        </div>
      </header>

      {#if managing.has(p.id)}
        <!-- Change which sub-list the pack lives in. -->
        <div class="flex gap-1 px-0.5 pb-0.5">
          {#each KINDS as { k, label }}
            <button
              type="button"
              onclick={() => changeKind(p, k)}
              class="flex-1 rounded border px-1 py-0.5 font-mono text-[0.64rem] {p.kind === k
                ? 'border-beacon text-beacon'
                : 'border-line text-muted hover:border-beacon'}"
            >
              {label()}
            </button>
          {/each}
        </div>
      {/if}

      <div class="grid grid-cols-[repeat(auto-fill,minmax(3.5rem,1fr))] gap-1">
        {#each p.items as it (it.id)}
          <div class="relative aspect-square">
            <PickerTile
              onSend={onSend ? () => onSend?.(it.id) : null}
              previewSrc={packItemUrl(it.id, it.size)}
              previewKind={isLottie(it.content_type)
                ? "lottie"
                : isVideoSticker(it.content_type)
                  ? "webm"
                  : "img"}
              alt={it.filename}
              class="grid size-full place-items-center rounded p-0.5 hover:bg-surface {onSend
                ? 'cursor-pointer'
                : 'cursor-default'}"
            >
              {#if isLottie(it.content_type)}
                <Sticker src={packItemUrl(it.id, it.size)} format="lottie" alt={it.filename} size={52} />
              {:else if isVideoSticker(it.content_type)}
                <Sticker src={packItemUrl(it.id, it.size)} format="webm" alt={it.filename} size={52} />
              {:else}
                <!-- The original blob, so animated WebP/GIF actually animate. -->
                <Thumb src={packItemUrl(it.id, it.size)} alt={it.filename} class="max-h-full max-w-full object-contain" />
              {/if}
            </PickerTile>
            {#if managing.has(p.id)}
              <button
                type="button"
                onclick={() => removeItem(p, it)}
                aria-label={$t("removeItem")}
                class="absolute -right-1 -top-1 grid size-4 place-items-center rounded-full bg-bad text-[0.6rem] text-white"
              >
                ✕
              </button>
            {/if}
          </div>
        {/each}
      </div>
    </section>
  {/each}
</div>
