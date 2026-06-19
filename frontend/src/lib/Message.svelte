<script lang="ts">
  import { isMine, type ChatMessage } from "./chat";
  import { t } from "./i18n";

  export let m: ChatMessage;

  const mine = isMine(m);

  function fmtTime(ms: number): string {
    const d = new Date(ms);
    const p = (n: number) => String(n).padStart(2, "0");
    return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
  }
</script>

<div class="line arrived" class:mine>
  <span class="time">{fmtTime(m.created_at)}</span>
  <span class="who">{mine ? $t("you") : m.author}</span>
  <span class="body">{m.body}</span>
</div>
