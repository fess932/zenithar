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
  class="flex gap-2 border-t border-line bg-surface px-3 pt-[0.7rem] pb-[calc(0.7rem+env(safe-area-inset-bottom))] sm:px-5"
  autocomplete="off"
>
  <input
    bind:value={body}
    placeholder={$t("messagePlaceholder")}
    aria-label={$t("messageAria")}
    maxlength="4000"
    class="min-h-11 flex-1 rounded-md border border-line bg-surface-2 px-3 text-base text-text placeholder:text-muted focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-beacon sm:text-[15px]"
  />
  <button
    type="submit"
    class="min-h-11 shrink-0 cursor-pointer rounded-md border border-beacon bg-beacon px-4 font-semibold text-[#1a1206] hover:brightness-110 focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-beacon"
  >
    {$t("send")}
  </button>
</form>
