<script lang="ts">
  import type { ChatMessage } from "./chat";
  import { me } from "./session";
  import { t } from "./i18n";

  export let m: ChatMessage;

  $: mine = $me?.id === m.author_id;

  function fmtTime(ms: number): string {
    const d = new Date(ms);
    const p = (n: number) => String(n).padStart(2, "0");
    return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
  }
</script>

<div class="line arrived" class:mine>
  <span class="time">{fmtTime(m.created_at)}</span>
  <span class="who">{mine ? $t("you") : m.author_name}</span>
  <span class="body">{m.body}</span>
</div>
