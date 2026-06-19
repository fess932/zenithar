<script lang="ts">
  import { t } from "./i18n";
  import { send } from "./chat";

  const NAME_KEY = "zenithar.name";
  let name = localStorage.getItem(NAME_KEY) ?? "";
  let body = "";

  function submit(): void {
    const text = body.trim();
    if (!text) return;
    if (send(text, name.trim() || "anon")) body = "";
  }

  function saveName(): void {
    localStorage.setItem(NAME_KEY, name.trim());
  }
</script>

<form
  on:submit|preventDefault={submit}
  class="flex gap-[0.6rem] border-t border-line bg-surface px-5 py-[0.85rem]"
  autocomplete="off"
>
  <input
    bind:value={name}
    on:change={saveName}
    placeholder={$t("namePlaceholder")}
    aria-label={$t("nameAria")}
    maxlength="40"
    class="w-32 rounded-md border border-line bg-surface-2 px-3 py-[0.6rem] font-mono text-[0.85rem] text-text focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-beacon max-[560px]:hidden"
  />
  <input
    bind:value={body}
    placeholder={$t("messagePlaceholder")}
    aria-label={$t("messageAria")}
    maxlength="4000"
    class="flex-1 rounded-md border border-line bg-surface-2 px-3 py-[0.6rem] text-text placeholder:text-muted focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-beacon"
  />
  <button
    type="submit"
    class="cursor-pointer rounded-md border border-beacon bg-beacon px-3 py-[0.6rem] font-semibold text-[#1a1206] hover:brightness-110 focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-beacon"
  >
    {$t("send")}
  </button>
</form>
