<script lang="ts">
  import { scale } from "svelte/transition";
  import { messageMenu, closeMessageMenu } from "./messageMenu";
  import { replyingTo, editing, deleteMessage } from "./chat";
  import { me } from "./session";
  import { t } from "./i18n";

  const MENU_W = 176; // w-44
  const ITEM_H = 44;

  $: m = $messageMenu;
  $: canCopy = !!m?.message.body.trim();
  $: mine = !!m && $me?.id === m.message.author_id;
  $: canEdit = mine && !!m?.message.body.trim(); // only text messages
  $: canDelete = mine || ($me?.is_admin ?? false);
  // Visible item count drives the height (keeps the menu fully on-screen).
  $: items = 1 + (canCopy ? 1 : 0) + (canEdit ? 1 : 0) + (canDelete ? 1 : 0);
  $: menuH = items * ITEM_H + 8;
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
