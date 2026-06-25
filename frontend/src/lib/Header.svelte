<script lang="ts">
  import { lang, t } from "./i18n";
  import { fontScale, FONT_SCALES } from "./uiscale";
  import { status } from "./chat";
  import { me, renameMe, logout, openInApp } from "./session";
  import Connections from "./Connections.svelte";

  export let onOpenAdmin: () => void;
  export let isEmployee = false;
  export let roomTitle = "";
  export let unreadTotal = 0;
  export let onOpenRooms: () => void = () => {};
  // null = no dot (common/own room); true/false = client online/offline.
  export let roomOnline: boolean | null = null;

  const statusKey = {
    connecting: "connecting",
    live: "live",
    down: "reconnecting",
  } as const;

  let editing = false;
  let draft = "";
  let menuOpen = false;
  let showConnections = false;

  function startEdit(): void {
    if ($me?.kind !== "user") return; // clients are anonymous
    draft = $me.display_name;
    editing = true;
  }
  async function saveEdit(): Promise<void> {
    const name = draft.trim();
    if (name) await renameMe(name);
    editing = false;
  }
</script>

<header
  class="relative flex items-center gap-2 border-b border-line bg-surface px-3 pt-[calc(0.7rem+env(safe-area-inset-top))] pb-[0.7rem] sm:gap-3.5 sm:px-5"
>
  {#if isEmployee}
    <!-- Doubles as the room switcher: tap to open the drawer; label = current room. -->
    <button
      type="button"
      onclick={onOpenRooms}
      aria-label={$t("rooms")}
      class="relative -ml-1 flex min-w-0 cursor-pointer items-center gap-2 rounded-md px-1 py-1 text-muted hover:text-text"
    >
      <span class="text-lg leading-none">☰</span>
      {#if unreadTotal > 0}
        <span
          class="absolute -left-0.5 -top-0.5 grid min-w-[1.05rem] place-items-center rounded-full bg-beacon px-1 text-[0.66rem] font-medium leading-tight text-[#1a1206]"
        >
          {unreadTotal}
        </span>
      {/if}
      <span class="truncate font-mono text-[0.84rem] text-text">{roomTitle}</span>
      {#if roomOnline !== null}
        <span
          class="size-1.5 shrink-0 rounded-full {roomOnline ? 'bg-emerald-400' : 'bg-muted/50'}"
          title={roomOnline ? "online" : "offline"}
        ></span>
      {/if}
    </button>
  {:else}
    <span
      class="text-[0.78rem] font-bold uppercase tracking-[0.2em] text-text sm:text-[0.82rem] sm:tracking-[0.22em]"
      >Zenithar</span
    >
  {/if}

  <div class="ml-auto flex items-center gap-2 sm:gap-[1.1rem]">
    {#if $me}
      {#if editing}
        <input
          bind:value={draft}
          onblur={saveEdit}
          onkeydown={(e) => e.key === "Enter" && saveEdit()}
          maxlength="40"
          aria-label={$t("editNameAria")}
          class="w-32 rounded-md border border-line bg-surface-2 px-2 py-[0.3rem] font-mono text-[0.8rem] text-text focus-visible:outline-2 focus-visible:outline-beacon sm:w-36"
        />
      {:else}
        <button
          type="button"
          onclick={startEdit}
          title={$me.kind === "user" ? $t("editNameAria") : ""}
          class="max-w-[34vw] truncate font-mono text-[0.78rem] text-you sm:max-w-none"
          class:cursor-pointer={$me.kind === "user"}
          class:cursor-default={$me.kind !== "user"}
        >
          {$me.display_name}
        </button>
      {/if}

      {#if $me.is_admin}
        <button
          type="button"
          onclick={onOpenAdmin}
          class="cursor-pointer font-mono text-[0.72rem] uppercase tracking-[0.08em] text-muted hover:text-text"
        >
          {$t("adminLinks")}
        </button>
      {/if}
    {/if}

    {#if isEmployee}
      <!-- Connection status doubles as the "who's online" (connections) button. -->
      <button
        type="button"
        onclick={() => (showConnections = true)}
        title={$t("connections")}
        aria-label={$t("connections")}
        class="beacon flex cursor-pointer items-center gap-1.5 rounded-md border border-line px-2 py-1 font-mono text-[0.72rem] uppercase tracking-[0.08em] text-muted hover:text-text sm:border-0 sm:px-0 sm:py-0"
        data-state={$status}
      >
        <span class="text-base leading-none">👥</span>
        <span class="beacon-dot"></span>
        <span class="hidden sm:inline">{$t(statusKey[$status])}</span>
      </button>
    {:else}
      <span
        class="beacon flex items-center gap-2 font-mono text-[0.72rem] uppercase tracking-[0.08em] text-muted"
        data-state={$status}
      >
        <span class="beacon-dot"></span><span class="hidden sm:inline"
          >{$t(statusKey[$status])}</span
        >
      </span>
    {/if}

    <!-- Desktop: language + logout inline -->
    <div class="hidden items-center gap-[1.1rem] min-[900px]:flex">
      <div class="flex overflow-hidden rounded-md border border-line" role="group" aria-label="Language">
        <button
          type="button"
          aria-pressed={$lang === "ru"}
          onclick={() => lang.set("ru")}
          class="cursor-pointer bg-transparent px-2 py-[0.22rem] font-mono text-[0.68rem] tracking-[0.06em] text-muted hover:text-text aria-[pressed=true]:bg-surface-2 aria-[pressed=true]:text-beacon"
        >
          RU
        </button>
        <button
          type="button"
          aria-pressed={$lang === "en"}
          onclick={() => lang.set("en")}
          class="cursor-pointer border-l border-line bg-transparent px-2 py-[0.22rem] font-mono text-[0.68rem] tracking-[0.06em] text-muted hover:text-text aria-[pressed=true]:bg-surface-2 aria-[pressed=true]:text-beacon"
        >
          EN
        </button>
      </div>

      <div class="flex overflow-hidden rounded-md border border-line" role="group" aria-label={$t("textSize")}>
        {#each FONT_SCALES as s, i}
          <button
            type="button"
            aria-pressed={$fontScale === s}
            onclick={() => fontScale.set(s)}
            title="{$t('textSize')} · {Math.round(s * 100)}%"
            class="cursor-pointer bg-transparent px-2 py-[0.22rem] font-mono text-[0.68rem] tracking-[0.06em] text-muted hover:text-text aria-[pressed=true]:bg-surface-2 aria-[pressed=true]:text-beacon {i >
            0
              ? 'border-l border-line'
              : ''}"
          >
            {Math.round(s * 100)}%
          </button>
        {/each}
      </div>

      {#if $me}
        <button
          type="button"
          onclick={openInApp}
          class="cursor-pointer font-mono text-[0.72rem] uppercase tracking-[0.08em] text-muted hover:text-text"
        >
          {$t("openInApp")}
        </button>
        <button
          type="button"
          onclick={logout}
          class="cursor-pointer font-mono text-[0.72rem] uppercase tracking-[0.08em] text-muted hover:text-bad"
        >
          {$t("logout")}
        </button>
      {/if}
    </div>

    <!-- Mobile: overflow menu holding language + open-in-app + logout -->
    <button
      type="button"
      aria-label={$t("menu")}
      aria-expanded={menuOpen}
      onclick={() => (menuOpen = !menuOpen)}
      class="grid size-9 cursor-pointer place-items-center rounded-md border border-line text-muted hover:text-text min-[900px]:hidden"
    >
      <span class="text-lg leading-none">⋯</span>
    </button>
  </div>

  {#if menuOpen}
    <!-- backdrop closes the menu -->
    <button
      type="button"
      aria-label={$t("menu")}
      onclick={() => (menuOpen = false)}
      class="fixed inset-0 z-10 cursor-default min-[900px]:hidden"
    ></button>
    <div
      class="absolute right-2 top-full z-20 mt-1 flex w-44 flex-col gap-2 rounded-lg border border-line bg-surface-2 p-3 shadow-lg min-[900px]:hidden"
    >
      <div class="flex overflow-hidden rounded-md border border-line" role="group" aria-label="Language">
        <button
          type="button"
          aria-pressed={$lang === "ru"}
          onclick={() => lang.set("ru")}
          class="flex-1 cursor-pointer bg-transparent py-2 font-mono text-[0.72rem] tracking-[0.06em] text-muted hover:text-text aria-[pressed=true]:bg-surface aria-[pressed=true]:text-beacon"
        >
          RU
        </button>
        <button
          type="button"
          aria-pressed={$lang === "en"}
          onclick={() => lang.set("en")}
          class="flex-1 cursor-pointer border-l border-line bg-transparent py-2 font-mono text-[0.72rem] tracking-[0.06em] text-muted hover:text-text aria-[pressed=true]:bg-surface aria-[pressed=true]:text-beacon"
        >
          EN
        </button>
      </div>

      <div class="flex overflow-hidden rounded-md border border-line" role="group" aria-label={$t("textSize")}>
        {#each FONT_SCALES as s, i}
          <button
            type="button"
            aria-pressed={$fontScale === s}
            onclick={() => fontScale.set(s)}
            class="flex-1 cursor-pointer bg-transparent py-2 font-mono text-[0.72rem] tracking-[0.06em] text-muted hover:text-text aria-[pressed=true]:bg-surface aria-[pressed=true]:text-beacon {i >
            0
              ? 'border-l border-line'
              : ''}"
          >
            {Math.round(s * 100)}%
          </button>
        {/each}
      </div>

      {#if $me}
        <button
          type="button"
          onclick={() => {
            menuOpen = false;
            openInApp();
          }}
          class="cursor-pointer rounded-md py-2 font-mono text-[0.74rem] uppercase tracking-[0.08em] text-muted hover:text-text"
        >
          {$t("openInApp")}
        </button>
        <button
          type="button"
          onclick={() => {
            menuOpen = false;
            logout();
          }}
          class="cursor-pointer rounded-md py-2 font-mono text-[0.74rem] uppercase tracking-[0.08em] text-muted hover:text-bad"
        >
          {$t("logout")}
        </button>
      {/if}
    </div>
  {/if}

  {#if showConnections}
    <Connections onClose={() => (showConnections = false)} />
  {/if}
</header>
