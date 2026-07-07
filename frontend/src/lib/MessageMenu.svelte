<script lang="ts">
  import { scale } from "svelte/transition";
  import { messageMenu, closeMessageMenu } from "./messageMenu";
  import { replyingTo, editing, deleteMessage, toggleReaction } from "./chat";
  import { saveFromMessage } from "./saved";
  import { openAddPack } from "./packs";
  import { me } from "./session";
  import { t } from "./i18n";
  import { EMOJI } from "./emoji";
  import { recentReactions, pushRecent, DEFAULT_REACTIONS } from "./reactions";

  const MENU_W = 176; // w-44
  const ITEM_H = 44;
  const REACT_H = 44; // the quick-reaction bar on top
  const PANEL_H = 216; // the expanded scrollable emoji grid

  // The bar shows your most-recent reactions first, padded with the defaults.
  $: quick = [...$recentReactions, ...DEFAULT_REACTIONS]
    .filter((e, i, a) => a.indexOf(e) === i)
    .slice(0, 6);

  // Tap "+" to browse the full emoji list and react with any of them.
  let expanded = false;
  let lastId: string | null = null;
  $: if (m && m.message.id !== lastId) {
    lastId = m.message.id;
    expanded = false;
  }

  function react(emoji: string): void {
    if (m) toggleReaction(m.message.id, emoji);
    pushRecent(emoji);
    closeMessageMenu();
  }

  $: m = $messageMenu;
  $: canCopy = !!m?.message.body.trim();
  $: mine = !!m && $me?.id === m.message.author_id;
  $: canEdit = mine && !!m?.message.body.trim(); // only text messages
  $: canDelete = mine || ($me?.is_admin ?? false);
  // Images or videos in the message can be saved to your collection ("сохранёнки").
  $: savable = (m?.message.attachments ?? []).filter(
    (a) => a.content_type.startsWith("image/") || a.content_type.startsWith("video/"),
  );
  $: canSave = savable.length > 0;
  // A sticker from a pack carries its share slug → offer to add the whole pack.
  $: packSlug = (m?.message.attachments ?? []).find((a) => a.pack_slug)?.pack_slug ?? null;
  $: canAddPack = !!packSlug;
  // Visible item count drives the height (keeps the menu fully on-screen).
  $: items =
    1 +
    (canCopy ? 1 : 0) +
    (canSave ? 1 : 0) +
    (canAddPack ? 1 : 0) +
    (canEdit ? 1 : 0) +
    (canDelete ? 1 : 0);
  $: menuH = (expanded ? PANEL_H : items * ITEM_H) + REACT_H + 8;
  $: left = m ? Math.max(8, Math.min(m.x, window.innerWidth - MENU_W - 8)) : 0;
  $: top = m ? Math.max(8, Math.min(m.y, window.innerHeight - menuH - 8)) : 0;

  function reply(): void {
    if (m) {
      editing.set(null);
      replyingTo.set(m.message);
    }
    closeMessageMenu();
  }

  function edit(): void {
    if (m) {
      replyingTo.set(null);
      editing.set(m.message);
    }
    closeMessageMenu();
  }

  function del(): void {
    const id = m?.message.id;
    closeMessageMenu();
    if (id && window.confirm($t("deleteConfirm"))) deleteMessage(id);
  }

  function save(): void {
    const items = savable;
    closeMessageMenu();
    for (const a of items) void saveFromMessage(a.id);
  }

  function addPack(): void {
    const slug = packSlug;
    closeMessageMenu();
    if (slug) openAddPack(slug);
  }

  async function copy(): Promise<void> {
    const text = m?.message.body ?? "";
    closeMessageMenu();
    try {
      await navigator.clipboard?.writeText(text);
    } catch {
      /* clipboard blocked (e.g. insecure context) — nothing to do */
    }
  }

  function onKey(e: KeyboardEvent): void {
    if (e.key === "Escape") closeMessageMenu();
  }

  const itemBase =
    "flex w-full cursor-pointer items-center gap-2.5 px-3 py-2.5 text-left text-[0.9rem] hover:bg-surface-2";
</script>

<svelte:window onkeydown={onKey} />

{#if m}
  <!-- Backdrop closes the menu (tap-away / right-click away). -->
  <button
    type="button"
    aria-label={$t("dismiss")}
    onclick={closeMessageMenu}
    oncontextmenu={(e) => {
      e.preventDefault();
      closeMessageMenu();
    }}
    class="fixed inset-0 z-50"
  ></button>

  <div
    role="menu"
    transition:scale={{ start: 0.92, duration: 120 }}
    style="left:{left}px; top:{top}px;"
    class="fixed z-50 w-44 overflow-hidden rounded-lg border border-line bg-surface shadow-2xl"
  >
    <!-- Quick reactions row: recents first, then a "+" to open the full list. -->
    <div class="flex items-center justify-between border-b border-line px-1.5 py-1">
      {#each quick as em}
        <button
          type="button"
          onclick={() => react(em)}
          aria-label={em}
          class="grid size-7 cursor-pointer place-items-center rounded-full text-lg leading-none hover:bg-surface-2"
        >
          {em}
        </button>
      {/each}
      <button
        type="button"
        onclick={() => (expanded = !expanded)}
        aria-label={$t("moreEmoji")}
        aria-expanded={expanded}
        class="grid size-7 cursor-pointer place-items-center rounded-full text-base leading-none text-muted hover:bg-surface-2 hover:text-text"
        class:bg-surface-2={expanded}
      >
        {expanded ? "×" : "+"}
      </button>
    </div>

    {#if expanded}
      <!-- Full emoji list, scrollable (Telegram-style). Any emoji is a reaction. -->
      <div class="overflow-y-auto p-1.5" style="height:{PANEL_H}px">
        <div class="grid grid-cols-[repeat(auto-fill,minmax(2rem,1fr))] gap-0.5">
          {#each EMOJI as em}
            <button
              type="button"
              onclick={() => react(em)}
              aria-label={em}
              class="grid aspect-square cursor-pointer place-items-center rounded text-lg leading-none hover:bg-surface-2"
            >
              {em}
            </button>
          {/each}
        </div>
      </div>
    {:else}
      <button type="button" role="menuitem" onclick={reply} class="{itemBase} text-text">
      <span class="text-base leading-none">↩</span>
      {$t("reply")}
    </button>
    {#if canCopy}
      <button
        type="button"
        role="menuitem"
        onclick={copy}
        class="{itemBase} border-t border-line text-text"
      >
        <span class="text-base leading-none">⧉</span>
        {$t("copy")}
      </button>
    {/if}
    {#if canSave}
      <button
        type="button"
        role="menuitem"
        onclick={save}
        class="{itemBase} border-t border-line text-text"
      >
        <span class="text-base leading-none">🔖</span>
        {$t("saveImage")}
      </button>
    {/if}
    {#if canAddPack}
      <button
        type="button"
        role="menuitem"
        onclick={addPack}
        class="{itemBase} border-t border-line text-text"
      >
        <span class="text-base leading-none">＋</span>
        {$t("addPack")}
      </button>
    {/if}
    {#if canEdit}
      <button
        type="button"
        role="menuitem"
        onclick={edit}
        class="{itemBase} border-t border-line text-text"
      >
        <span class="text-base leading-none">✎</span>
        {$t("edit")}
      </button>
    {/if}
    {#if canDelete}
      <button
        type="button"
        role="menuitem"
        onclick={del}
        class="{itemBase} border-t border-line text-bad"
      >
        <span class="text-base leading-none">🗑</span>
        {$t("delete")}
      </button>
    {/if}
    {/if}
  </div>
{/if}
