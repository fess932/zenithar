<script lang="ts">
  import { lang, t } from "./i18n";
  import { fontScale, FONT_SCALES } from "./uiscale";
  import { status, activeRoom } from "./chat";
  import { callState, startCall } from "./call";
  import { me, renameMe, logout, openInApp } from "./session";
  import { openProfile } from "./profile";
  import { selectedMic, listMics, type MicDevice } from "./mic";
  import {
    detectOS,
    pickAsset,
    OS_NAME,
    REPO,
    RELEASES_PAGE,
    RELEASE_API,
    type ReleaseAsset,
  } from "./util/download";
  import { isApp } from "./platform";
  import Connections from "./Connections.svelte";
  import Avatar from "./Avatar.svelte";
  import AvatarEditor from "./AvatarEditor.svelte";

  export let onOpenAdmin: () => void;
  export let isEmployee = false;
  export let roomTitle = "";
  export let unreadTotal = 0;
  export let onOpenRooms: () => void = () => {};
  // null = no dot (common/own room); true/false = client online/offline.
  export let roomOnline: boolean | null = null;
  // Layout context for the left side:
  //   drawer — desktop / client: ☰ room switcher + current room (default).
  //   list   — mobile chat-list home: just the app wordmark.
  //   room   — mobile open conversation: ‹ back + room title.
  export let mode: "drawer" | "list" | "room" = "drawer";
  export let onBack: () => void = () => {};
  // In a DM, the peer to show as a tappable avatar (→ their profile). null otherwise.
  export let peer: { id: string; name: string; avatar: string | null } | null = null;

  const statusKey = {
    connecting: "connecting",
    live: "live",
    down: "reconnecting",
  } as const;

  let editing = false;
  let draft = "";
  let menuOpen = false;

  // Microphone picker (calls + voice notes). Devices are enumerated when the menu
  // opens; labels need a prior permission grant, else they're numbered.
  let mics: MicDevice[] | null = null;
  async function loadMics(): Promise<void> {
    mics = await listMics();
  }
  $: if (menuOpen && mics === null) void loadMics();
  let showConnections = false;
  let showAvatarEditor = false;

  // Only real users (not anonymous clients) own an editable name + avatar.
  $: canEditProfile = $me?.kind === "user";

  function startEdit(): void {
    if (!canEditProfile || !$me) return;
    draft = $me.display_name;
    editing = true;
  }
  async function saveEdit(): Promise<void> {
    const name = draft.trim();
    if (name) await renameMe(name);
    editing = false;
  }
  function openAvatarEditor(): void {
    menuOpen = false;
    showAvatarEditor = true;
  }

  // Platform-aware download: resolve the right release asset for the current OS
  // lazily from the GitHub API when the menu opens (pure logic in download.ts).
  const os = detectOS(typeof navigator !== "undefined" ? navigator.userAgent : "");
  let downloadUrl = RELEASES_PAGE; // resolved to a direct asset once loaded
  let dlLoaded = false;
  async function loadDownload(): Promise<void> {
    if (dlLoaded) return;
    dlLoaded = true;
    try {
      const r = await fetch(RELEASE_API, { headers: { Accept: "application/vnd.github+json" } });
      if (!r.ok) return;
      const assets = ((await r.json()) as { assets?: ReleaseAsset[] }).assets ?? [];
      const hit = pickAsset(os, assets);
      if (hit) downloadUrl = hit;
    } catch {
      /* keep the releases-page fallback */
    }
  }
  $: if (menuOpen) void loadDownload();

  // "New version available" hint: compare the running build's commit (/api/version)
  // with the latest commit on main (GitHub). Checked once when the menu opens.
  let newVersion = false;
  let versionChecked = false;
  async function checkVersion(): Promise<void> {
    if (versionChecked) return;
    versionChecked = true;
    try {
      const [cur, gh] = await Promise.all([
        fetch("/api/version").then((r) => (r.ok ? r.json() : null)),
        fetch(`https://api.github.com/repos/${REPO}/commits?per_page=1`, {
          headers: { Accept: "application/vnd.github+json" },
        }).then((r) => (r.ok ? r.json() : null)),
      ]);
      const current = (cur as { commit?: string } | null)?.commit;
      const latest = Array.isArray(gh) ? (gh[0] as { sha?: string } | undefined)?.sha : undefined;
      // current is a short sha; latest a full one. Skip unknown/dev builds.
      if (current && current !== "unknown" && latest) newVersion = !latest.startsWith(current);
    } catch {
      /* offline / rate-limited — no badge */
    }
  }
  $: if (menuOpen) void checkVersion();

  // "What's new": the last commit subjects, pulled lazily from GitHub (open CORS,
  // same source as the download link). Fetched once when the panel first opens.
  const CHANGES_URL = "https://api.github.com/repos/fess932/zenithar/commits?per_page=10";
  let changesOpen = false;
  let commits: string[] | null = null;
  let loadingChanges = false;
  async function toggleChanges(): Promise<void> {
    changesOpen = !changesOpen;
    if (!changesOpen || commits !== null || loadingChanges) return;
    loadingChanges = true;
    try {
      const r = await fetch(CHANGES_URL, { headers: { Accept: "application/vnd.github+json" } });
      const data = r.ok ? ((await r.json()) as Array<{ commit?: { message?: string } }>) : [];
      commits = data.map((c) => (c.commit?.message ?? "").split("\n")[0]).filter(Boolean);
    } catch {
      commits = [];
    }
    loadingChanges = false;
  }
</script>

<header
  class="relative flex items-center gap-2 border-b border-line bg-surface px-3 pt-[calc(0.7rem+env(safe-area-inset-top))] pb-[0.7rem] sm:gap-3.5 sm:px-5"
>
  {#if mode === "list"}
    <!-- Chat-list home (mobile): the app wordmark, no room. -->
    <span class="pl-0.5 font-mono text-[0.82rem] uppercase tracking-[0.28em] text-beacon">Zenithar</span>
  {:else if mode === "room"}
    <!-- Open conversation (mobile): ‹ back to the list + room title. -->
    <button
      type="button"
      onclick={onBack}
      aria-label={$t("back")}
      class="-ml-1.5 grid size-9 shrink-0 cursor-pointer place-items-center rounded-md text-muted hover:text-text"
    >
      <span class="text-2xl leading-none">‹</span>
    </button>
    {#if peer}
      <button
        type="button"
        onclick={() => peer && openProfile(peer)}
        aria-label={peer.name}
        class="shrink-0 cursor-pointer rounded-full hover:opacity-80"
      >
        <Avatar id={peer.id} name={peer.name} avatar={peer.avatar} size={30} />
      </button>
    {/if}
    <span class="truncate font-mono text-[0.9rem] text-text">{roomTitle}</span>
    {#if roomOnline !== null}
      <span
        class="size-1.5 shrink-0 rounded-full {roomOnline ? 'bg-emerald-400' : 'bg-muted/50'}"
        title={roomOnline ? "online" : "offline"}
      ></span>
    {/if}
  {:else if isEmployee}
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
      <span class="truncate font-mono text-[0.9rem] text-text">{roomTitle}</span>
      {#if roomOnline !== null}
        <span
          class="size-1.5 shrink-0 rounded-full {roomOnline ? 'bg-emerald-400' : 'bg-muted/50'}"
          title={roomOnline ? "online" : "offline"}
        ></span>
      {/if}
    </button>
    {#if peer}
      <!-- DM peer avatar, right next to the name → tap for their profile. -->
      <button
        type="button"
        onclick={() => peer && openProfile(peer)}
        aria-label={peer.name}
        class="shrink-0 cursor-pointer rounded-full hover:opacity-80"
      >
        <Avatar id={peer.id} name={peer.name} avatar={peer.avatar} size={30} />
      </button>
    {/if}
  {:else}
    <span class="truncate font-mono text-[0.9rem] text-text">{roomTitle}</span>
  {/if}

  <div class="ml-auto flex items-center gap-2 sm:gap-3">
    <!-- Place a call within the open room. Hidden once a call is up, and on the
         chat-list home (no room context there). -->
    {#if $callState === "idle" && $activeRoom && mode !== "list"}
      <button
        type="button"
        onclick={() => $activeRoom && startCall($activeRoom)}
        aria-label={$t("call")}
        title={$t("call")}
        class="grid size-9 cursor-pointer place-items-center rounded-md border border-line text-muted hover:border-beacon hover:text-beacon"
      >
        <span class="text-base leading-none">📞</span>
      </button>
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

    <!-- Profile: avatar button opens a menu holding name, avatar, admin, settings. -->
    {#if $me}
      <button
        type="button"
        onclick={() => (menuOpen = !menuOpen)}
        aria-label={$t("profile")}
        aria-expanded={menuOpen}
        class="shrink-0 cursor-pointer rounded-full ring-offset-2 ring-offset-surface hover:ring-2 hover:ring-line aria-[expanded=true]:ring-2 aria-[expanded=true]:ring-beacon"
      >
        <Avatar id={$me.id} name={$me.display_name} avatar={$me.avatar} size={32} />
      </button>
    {/if}
  </div>

  {#if menuOpen}
    <!-- backdrop closes the menu -->
    <button
      type="button"
      aria-label={$t("close")}
      onclick={() => (menuOpen = false)}
      class="fixed inset-0 z-10 cursor-default"
    ></button>
    <div
      class="absolute right-2 top-full z-20 mt-1 flex w-60 max-w-[88vw] flex-col gap-2 rounded-lg border border-line bg-surface-2 p-3 shadow-lg"
    >
      <!-- Identity: avatar + name (editable for users). -->
      <div class="flex items-center gap-3 px-1 pb-1">
        {#if $me}
          <Avatar id={$me.id} name={$me.display_name} avatar={$me.avatar} size={40} />
        {/if}
        <div class="min-w-0 flex-1">
          {#if editing}
            <input
              bind:value={draft}
              onblur={saveEdit}
              onkeydown={(e) => e.key === "Enter" && saveEdit()}
              maxlength="40"
              aria-label={$t("editNameAria")}
              class="w-full rounded-md border border-line bg-surface px-2 py-[0.3rem] font-mono text-[0.82rem] text-text focus-visible:outline-2 focus-visible:outline-beacon"
            />
          {:else}
            <button
              type="button"
              onclick={startEdit}
              title={canEditProfile ? $t("editNameAria") : ""}
              class="block max-w-full truncate text-left font-mono text-[0.9rem] text-you"
              class:cursor-pointer={canEditProfile}
              class:cursor-default={!canEditProfile}
            >
              {$me?.display_name}
            </button>
          {/if}
          {#if canEditProfile}
            <button
              type="button"
              onclick={openAvatarEditor}
              class="cursor-pointer font-mono text-[0.72rem] text-muted hover:text-beacon"
            >
              {$t("changeAvatar")}
            </button>
          {/if}
        </div>
      </div>

      <button
        type="button"
        onclick={() => {
          menuOpen = false;
          if ($me) openProfile({ id: $me.id, name: $me.display_name, avatar: $me.avatar ?? null });
        }}
        class="cursor-pointer rounded-md px-1 py-2 text-left font-mono text-[0.78rem] uppercase tracking-[0.08em] text-muted hover:text-text"
      >
        {$t("myProfile")}
      </button>

      {#if $me?.is_admin}
        <button
          type="button"
          onclick={() => {
            menuOpen = false;
            onOpenAdmin();
          }}
          class="cursor-pointer rounded-md px-1 py-2 text-left font-mono text-[0.78rem] uppercase tracking-[0.08em] text-muted hover:text-text"
        >
          {$t("adminLinks")}
        </button>
      {/if}

      <div class="my-1 border-t border-line"></div>

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

      <!-- Microphone for calls + voice notes (handy on desktop with several mics). -->
      <div class="flex items-center gap-2 rounded-md border border-line px-2 py-1">
        <span class="shrink-0 text-base leading-none">🎤</span>
        <select
          bind:value={$selectedMic}
          aria-label={$t("microphone")}
          class="min-w-0 flex-1 cursor-pointer bg-surface-2 py-1 font-mono text-[0.72rem] text-text outline-none"
        >
          <option value="">{$t("micDefault")}</option>
          {#each mics ?? [] as m (m.deviceId)}
            <option value={m.deviceId}>{m.label}</option>
          {/each}
        </select>
      </div>

      <!-- "Open in app" hands off to the desktop/mobile app — pointless (and
           hidden) when we're already running inside it. Web-only. -->
      {#if !isApp}
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
      {/if}
      {#if newVersion}
        <div
          class="flex items-center justify-center gap-1.5 rounded-md bg-beacon/10 py-1.5 text-center font-mono text-[0.7rem] tracking-[0.04em] text-beacon"
        >
          <span class="text-sm leading-none">🆕</span>
          {$t("newVersion")}
        </div>
      {/if}
      <!-- Latest app build for THIS OS (Windows/macOS/Android), resolved from the
           GitHub release; falls back to the releases page. Plain external link so
           it works in the browser and inside the app. -->
      <a
        href={downloadUrl}
        target="_blank"
        rel="noopener noreferrer"
        onclick={() => (menuOpen = false)}
        class="block cursor-pointer rounded-md py-2 text-center font-mono text-[0.74rem] uppercase tracking-[0.08em] text-muted hover:text-text"
      >
        {$t("downloadApp")}{#if OS_NAME[os]} · {OS_NAME[os]}{/if}
      </a>
      <!-- What's new: the latest commit subjects, lazily fetched from GitHub. -->
      <button
        type="button"
        onclick={toggleChanges}
        aria-expanded={changesOpen}
        class="cursor-pointer rounded-md py-2 font-mono text-[0.74rem] uppercase tracking-[0.08em] text-muted hover:text-text"
      >
        {$t("recentChanges")}
      </button>
      {#if changesOpen}
        <div class="max-h-44 overflow-y-auto rounded-md border border-line bg-surface px-2.5 py-2">
          {#if loadingChanges}
            <p class="font-mono text-[0.72rem] text-muted">…</p>
          {:else if commits && commits.length}
            <ul class="flex flex-col gap-1.5">
              {#each commits as c}
                <li class="font-mono text-[0.72rem] leading-snug text-text">
                  <span class="text-muted">·</span>
                  {c}
                </li>
              {/each}
            </ul>
          {:else}
            <p class="font-mono text-[0.72rem] text-muted">—</p>
          {/if}
        </div>
      {/if}
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
    </div>
  {/if}

  {#if showConnections}
    <Connections onClose={() => (showConnections = false)} />
  {/if}

  {#if showAvatarEditor}
    <AvatarEditor onClose={() => (showAvatarEditor = false)} />
  {/if}
</header>
