<script lang="ts">
  import { lang, t } from "./i18n";
</script>

<div class="grid h-dvh place-items-center bg-ink px-6 text-text">
  <div class="max-w-sm text-center">
    <div class="mb-3 text-[0.82rem] font-bold uppercase tracking-[0.22em]">Zenithar</div>
    <h1 class="mb-2 text-lg font-semibold">{$t("landingTitle")}</h1>
    <p class="font-mono text-[0.85rem] leading-relaxed text-muted">{$t("landingHint")}</p>

    <div class="mt-6 inline-flex overflow-hidden rounded-md border border-line">
      <button
        type="button"
        aria-pressed={$lang === "ru"}
        onclick={() => lang.set("ru")}
        class="cursor-pointer bg-transparent px-3 py-1 font-mono text-[0.7rem] text-muted hover:text-text aria-[pressed=true]:bg-surface-2 aria-[pressed=true]:text-beacon"
      >
        RU
      </button>
      <button
        type="button"
        aria-pressed={$lang === "en"}
        onclick={() => lang.set("en")}
        class="cursor-pointer border-l border-line bg-transparent px-3 py-1 font-mono text-[0.7rem] text-muted hover:text-text aria-[pressed=true]:bg-surface-2 aria-[pressed=true]:text-beacon"
      >
        EN
      </button>
    </div>
  </div>
</div>
