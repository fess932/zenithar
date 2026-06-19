<script lang="ts">
  import { lang, t, type Lang } from "./i18n";
  import { status } from "./chat";

  const statusKey = {
    connecting: "connecting",
    live: "live",
    down: "reconnecting",
  } as const;

  function pick(l: Lang): void {
    lang.set(l);
  }
</script>

<header class="flex items-baseline gap-3.5 border-b border-line bg-surface px-5 py-[0.9rem]">
  <span class="text-[0.82rem] font-bold uppercase tracking-[0.22em]">Zenithar</span>
  <span class="font-mono text-[0.78rem] text-muted">{$t("room")}</span>

  <div class="ml-auto flex items-center gap-[1.1rem]">
    <span
      class="beacon flex items-center gap-2 font-mono text-[0.72rem] uppercase tracking-[0.08em] text-muted"
      data-state={$status}
    >
      <span class="beacon-dot"></span><span>{$t(statusKey[$status])}</span>
    </span>

    <div
      class="flex overflow-hidden rounded-md border border-line"
      role="group"
      aria-label="Language"
    >
      <button
        type="button"
        aria-pressed={$lang === "ru"}
        on:click={() => pick("ru")}
        class="cursor-pointer bg-transparent px-2 py-[0.22rem] font-mono text-[0.68rem] tracking-[0.06em] text-muted hover:text-text aria-[pressed=true]:bg-surface-2 aria-[pressed=true]:text-beacon"
      >
        RU
      </button>
      <button
        type="button"
        aria-pressed={$lang === "en"}
        on:click={() => pick("en")}
        class="cursor-pointer border-l border-line bg-transparent px-2 py-[0.22rem] font-mono text-[0.68rem] tracking-[0.06em] text-muted hover:text-text aria-[pressed=true]:bg-surface-2 aria-[pressed=true]:text-beacon"
      >
        EN
      </button>
    </div>
  </div>
</header>
