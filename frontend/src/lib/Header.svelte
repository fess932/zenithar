<script lang="ts">
  import { lang, t } from "./i18n";
  import { status } from "./chat";
  import { me, renameMe, logout } from "./session";

  export let onOpenAdmin: () => void;

  const statusKey = {
    connecting: "connecting",
    live: "live",
    down: "reconnecting",
  } as const;

  let editing = false;
  let draft = "";

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

<header class="flex items-center gap-3.5 border-b border-line bg-surface px-5 py-[0.9rem]">
  <span class="text-[0.82rem] font-bold uppercase tracking-[0.22em]">Zenithar</span>
  <span class="font-mono text-[0.78rem] text-muted">{$t("room")}</span>

  <div class="ml-auto flex items-center gap-[1.1rem]">
    {#if $me}
      {#if editing}
        <input
          bind:value={draft}
          onblur={saveEdit}
          onkeydown={(e) => e.key === "Enter" && saveEdit()}
          maxlength="40"
          aria-label={$t("editNameAria")}
          class="w-36 rounded-md border border-line bg-surface-2 px-2 py-[0.2rem] font-mono text-[0.78rem] text-text focus-visible:outline-2 focus-visible:outline-beacon"
        />
      {:else}
        <button
          type="button"
          onclick={startEdit}
          title={$me.kind === "user" ? $t("editNameAria") : ""}
          class="font-mono text-[0.78rem] text-you"
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

    <span
      class="beacon flex items-center gap-2 font-mono text-[0.72rem] uppercase tracking-[0.08em] text-muted"
      data-state={$status}
    >
      <span class="beacon-dot"></span><span>{$t(statusKey[$status])}</span>
    </span>

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

    {#if $me}
      <button
        type="button"
        onclick={logout}
        class="cursor-pointer font-mono text-[0.72rem] uppercase tracking-[0.08em] text-muted hover:text-bad"
      >
        {$t("logout")}
      </button>
    {/if}
  </div>
</header>
