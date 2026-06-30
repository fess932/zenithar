<script lang="ts">
  import { t } from "./i18n";
  import { EMOJI } from "./emoji";
  import { me, setAvatarEmoji, setAvatarPhoto, resetAvatar } from "./session";
  import { notify } from "./chat";
  import Avatar from "./Avatar.svelte";

  export let onClose: () => void;

  const MAX_PHOTO = 6 * 1024 * 1024;
  let busy = false;
  let fileInput: HTMLInputElement;

  async function pickEmoji(em: string): Promise<void> {
    if (busy) return;
    busy = true;
    await setAvatarEmoji(em);
    busy = false;
    onClose();
  }

  async function onPhoto(e: Event): Promise<void> {
    const input = e.target as HTMLInputElement;
    const file = input.files?.[0];
    input.value = "";
    if (!file) return;
    if (file.size > MAX_PHOTO) {
      notify($t("avatarTooBig"));
      return;
    }
    busy = true;
    const ok = await setAvatarPhoto(file);
    busy = false;
    if (ok) onClose();
  }

  async function reset(): Promise<void> {
    if (busy) return;
    busy = true;
    await resetAvatar();
    busy = false;
    onClose();
  }
</script>

<!-- Modal: dim backdrop closes it. -->
<button
  type="button"
  aria-label={$t("close")}
  onclick={onClose}
  class="fixed inset-0 z-50 cursor-default bg-black/50"
></button>

<div
  role="dialog"
  aria-label={$t("changeAvatar")}
  class="fixed left-1/2 top-1/2 z-50 flex w-[min(24rem,92vw)] -translate-x-1/2 -translate-y-1/2 flex-col gap-4 rounded-xl border border-line bg-surface p-5 shadow-2xl"
>
  <div class="flex items-center gap-3">
    {#if $me}
      <Avatar id={$me.id} name={$me.display_name} avatar={$me.avatar} size={56} />
    {/if}
    <div class="min-w-0">
      <div class="font-mono text-[0.72rem] uppercase tracking-[0.1em] text-muted">
        {$t("changeAvatar")}
      </div>
      <div class="truncate text-[0.95rem] text-text">{$me?.display_name ?? ""}</div>
    </div>
    <button
      type="button"
      onclick={onClose}
      aria-label={$t("close")}
      class="ml-auto grid size-8 shrink-0 cursor-pointer place-items-center rounded text-muted hover:text-text"
    >
      ✕
    </button>
  </div>

  <!-- Photo upload + reset to default -->
  <div class="flex gap-2">
    <button
      type="button"
      onclick={() => fileInput.click()}
      disabled={busy}
      class="flex flex-1 cursor-pointer items-center justify-center gap-2 rounded-md border border-line bg-surface-2 px-3 py-2 text-[0.85rem] text-text hover:border-beacon disabled:opacity-50"
    >
      📷 {$t("uploadPhoto")}
    </button>
    <button
      type="button"
      onclick={reset}
      disabled={busy}
      class="cursor-pointer rounded-md border border-line px-3 py-2 text-[0.85rem] text-muted hover:text-text disabled:opacity-50"
    >
      {$t("resetAvatar")}
    </button>
  </div>

  <!-- Emoji grid -->
  <div class="max-h-56 overflow-y-auto rounded-md border border-line bg-surface-2 p-2">
    <div class="grid grid-cols-8 gap-1 sm:grid-cols-10">
      {#each EMOJI as em}
        <button
          type="button"
          onclick={() => pickEmoji(em)}
          disabled={busy}
          class="grid aspect-square cursor-pointer place-items-center rounded text-xl hover:bg-surface disabled:opacity-50"
        >
          {em}
        </button>
      {/each}
    </div>
  </div>

  <input
    bind:this={fileInput}
    type="file"
    accept="image/*"
    onchange={onPhoto}
    class="hidden"
    aria-hidden="true"
  />
</div>
