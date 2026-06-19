<script lang="ts">
  import { t } from "./i18n";
  import { send } from "./chat";

  let body = "";

  function submit(e: SubmitEvent): void {
    e.preventDefault();
    const text = body.trim();
    if (!text) return;
    if (send(text)) body = "";
  }
</script>

<form
  onsubmit={submit}
  class="flex gap-[0.6rem] border-t border-line bg-surface px-5 py-[0.85rem]"
  autocomplete="off"
>
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
