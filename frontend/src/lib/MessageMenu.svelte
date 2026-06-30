<script lang="ts">
  import { scale } from "svelte/transition";
  import { messageMenu, closeMessageMenu } from "./messageMenu";
  import { replyingTo, editing, deleteMessage, toggleReaction } from "./chat";
  import { saveFromMessage } from "./saved";
  import { me } from "./session";
  import { t } from "./i18n";

  const MENU_W = 176; // w-44
  const ITEM_H = 44;
  const REACT_H = 44; // the quick-reaction bar on top
  // Quick reactions, Telegram-style. Tapping toggles your own.
  const QUICK = ["👍", "❤️", "😂", "🔥", "🎉", "😮"];

  function react(emoji: string): void {
    if (m) toggleReaction(m.message.id, emoji);
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
  // Visible item count drives the height (keeps the menu fully on-screen).
  $: items = 1 + (canCopy ? 1 : 0) + (canSave ? 1 : 0) + (canEdit ? 1 : 0) + (canDelete ? 1 : 0);
  $: menuH = items * ITEM_H + REACT_H + 8;
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
    <!-- Quick reactions row -->
    <div class="flex items-center justify-between border-b border-line px-1.5 py-1">
      {#each QUICK as em}
        <button
          type="button"
          onclick={() => react(em)}
          aria-label={em}
          class="grid size-7 cursor-pointer place-items-center rounded-full text-lg leading-none hover:bg-surface-2"
        >
          {em}
        </button>
      {/each}
    </div>
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
  </div>
{/if}
