<script lang="ts">
  import type { ChatMessage } from "./chat";
  import { me } from "./session";
  import { t } from "./i18n";
  import VoicePlayer from "./VoicePlayer.svelte";

  export let m: ChatMessage;

  $: mine = $me?.id === m.author_id;

  function fmtTime(ms: number): string {
    const d = new Date(ms);
    const p = (n: number) => String(n).padStart(2, "0");
    return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
  }

  function fmtSize(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }

  const orig = (id: string) => `/api/attachments/${id}`;
  const thumb = (id: string) => `/api/attachments/${id}/thumb`;
</script>

<div class="line arrived" class:mine>
  <span class="time">{fmtTime(m.created_at)}</span>
  <span class="who">{mine ? $t("you") : m.author_name}</span>
  <div class="body">
    {#if m.body}<span class="break-words">{m.body}</span>{/if}

    {#if m.attachments.length > 0}
      <div class="mt-1 flex flex-wrap items-start gap-2">
        {#each m.attachments as a (a.id)}
          {#if a.content_type.startsWith("image/")}
            <a href={orig(a.id)} target="_blank" rel="noreferrer">
              <img
                src={a.has_thumb ? thumb(a.id) : orig(a.id)}
                alt={a.filename}
                loading="lazy"
                class="max-h-48 max-w-[12rem] rounded-md border border-line object-cover"
              />
            </a>
          {:else if a.content_type.startsWith("audio/")}
            <VoicePlayer src={orig(a.id)} />
          {:else}
            <a
              href={orig(a.id)}
              target="_blank"
              rel="noreferrer"
              download={a.filename}
              class="inline-flex max-w-full items-center gap-2 rounded-md border border-line bg-surface-2 px-3 py-2 hover:border-beacon"
            >
              <span class="text-base leading-none">📎</span>
              <span class="min-w-0">
                <span class="block truncate text-[0.85rem] text-text">{a.filename}</span>
                <span class="block font-mono text-[0.7rem] text-muted">{fmtSize(a.size)}</span>
              </span>
            </a>
          {/if}
        {/each}
      </div>
    {/if}
  </div>
</div>
