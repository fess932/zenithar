<script lang="ts">
  import { scale } from "svelte/transition";
  import { messageMenu, closeMessageMenu } from "./messageMenu";
  import { replyingTo } from "./chat";
  import { t } from "./i18n";

  const MENU_W = 176; // w-44
  const ITEM_H = 44;

  $: m = $messageMenu;
  $: canCopy = !!m?.message.body.trim();
  $: menuH = (1 + (canCopy ? 1 : 0)) * ITEM_H + 8;
  // Keep the menu fully on-screen near the press point.
  $: left = m ? Math.max(8, Math.min(m.x, window.innerWidth - MENU_W - 8)) : 0;
  $: top = m ? Math.max(8, Math.min(m.y, window.innerHeight - menuH - 8)) : 0;

  function reply(): void {
    if (m) replyingTo.set(m.message);
    closeMessageMenu();
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
    <button
      type="button"
      role="menuitem"
      onclick={reply}
      class="flex w-full cursor-pointer items-center gap-2.5 px-3 py-2.5 text-left text-[0.9rem] text-text hover:bg-surface-2"
    >
      <span class="text-base leading-none">↩</span>
      {$t("reply")}
    </button>
    {#if canCopy}
      <button
        type="button"
        role="menuitem"
        onclick={copy}
        class="flex w-full cursor-pointer items-center gap-2.5 border-t border-line px-3 py-2.5 text-left text-[0.9rem] text-text hover:bg-surface-2"
      >
        <span class="text-base leading-none">⧉</span>
        {$t("copy")}
      </button>
    {/if}
  </div>
{/if}
