<script lang="ts">
  import { onMount, afterUpdate, tick } from "svelte";
  import Header from "./Header.svelte";
  import Composer from "./Composer.svelte";
  import Message from "./Message.svelte";
  import Principals from "./Principals.svelte";
  import { fly } from "svelte/transition";
  import Call from "./Call.svelte";
  import Lightbox from "./Lightbox.svelte";
  import MessageMenu from "./MessageMenu.svelte";
  import ChatList from "./ChatList.svelte";
  import Profile from "./Profile.svelte";
  import { profileTarget, closeProfile } from "./profile";
  import { closeMessageMenu } from "./messageMenu";
  import { sameDay } from "./util/time";
  import { roomLabel } from "./util/rooms";
  import { initCallSignaling } from "./call";
  import {
    messages,
    connect,
    rooms,
    activeRoom,
    joinRoom,
    loadRooms,
    notice,
    dismissNotice,
    unread,
    online,
    loadOlder,
    type RoomSummary,
  } from "./chat";
  import {
    initNotifications,
    toasts,
    openToast,
    dismissToast,
    reactionToasts,
    openReactionToast,
    dismissReactionToast,
    mutedRooms,
    toggleMute,
  } from "./notify";
  import { me } from "./session";
  import { t, lang } from "./i18n";

  let logEl: HTMLElement;
  let pinned = true;
  let view: "chat" | "admin" = "chat";
  let drawerOpen = false;

  // Mobile chat-list model (Telegram-style): the rooms list is the home screen
  // and a conversation is pushed over it. `roomOpen` is driven by the History
  // API so the Android back gesture pops room → list (and minimizes at the list,
  // handled natively). Desktop keeps the drawer; clients have one room, no list.
  let mobile = typeof window !== "undefined" && window.matchMedia("(max-width: 640px)").matches;
  let roomOpen = false;

  $: isEmployee = $me?.kind === "user";

  // Telegram-style day dividers: a label before the first message of each day.
  function dayLabel(ms: number): string {
    const now = Date.now();
    if (sameDay(ms, now)) return $t("today");
    if (sameDay(ms, now - 86400000)) return $t("yesterday");
    const d = new Date(ms);
    const opts: Intl.DateTimeFormatOptions = { day: "numeric", month: "long" };
    if (d.getFullYear() !== new Date().getFullYear()) opts.year = "numeric";
    return d.toLocaleDateString($lang === "ru" ? "ru-RU" : "en-US", opts);
  }

  onMount(() => {
    connect();
    initCallSignaling();
    initNotifications();
    if (isEmployee) loadRooms();

    // Track the mobile breakpoint and the in-app back stack. Opening a chat pushes
    // a history entry; the back gesture pops it (popstate) → we drop to the list.
    const mq = window.matchMedia("(max-width: 640px)");
    const onMq = (): void => {
      mobile = mq.matches;
    };
    const onPop = (): void => {
      roomOpen = !!history.state?.zRoom;
    };
    mq.addEventListener("change", onMq);
    window.addEventListener("popstate", onPop);
    return () => {
      mq.removeEventListener("change", onMq);
      window.removeEventListener("popstate", onPop);
    };
  });

  $: totalUnread = Object.values($unread).reduce((a, b) => a + b, 0);

  let loadingMore = false;

  async function onScroll(): Promise<void> {
    pinned = logEl.scrollHeight - logEl.scrollTop - logEl.clientHeight < 80;
    closeMessageMenu(); // a floating menu shouldn't trail the scroll
    // Near the top → lazily load older history, keeping the viewport stable.
    if (logEl.scrollTop < 120 && !loadingMore) {
      loadingMore = true;
      const prevHeight = logEl.scrollHeight;
      const added = await loadOlder();
      if (added > 0) {
        await tick(); // wait for the prepended messages to render
        logEl.scrollTop = logEl.scrollHeight - prevHeight; // restore position
      }
      loadingMore = false;
    }
  }

  function scrollToBottom(): void {
    if (logEl) {
      logEl.scrollTo({ top: logEl.scrollHeight, behavior: "smooth" });
      pinned = true;
    }
  }

  // Keep pinned to the newest line after each DOM update (not a reactive
  // dependency, so updating scroll can't re-trigger itself).
  afterUpdate(() => {
    if (pinned && logEl) logEl.scrollTop = logEl.scrollHeight;
  });

  $: current = $rooms.find((r) => r.id === $activeRoom) ?? null;
  $: currentTitle = current ? roomLabel(current, $t("room")) : $t("room");

  // In a DM, the peer (other member) → a tappable avatar in the header. The avatar
  // value comes from their messages (resolved live), default emoji until then.
  $: peerId = current?.kind === "direct" ? (current.client_id ?? null) : null;
  $: peer = peerId
    ? {
        id: peerId,
        name: current?.title ?? "?",
        avatar: $messages.find((m) => m.author_id === peerId)?.author_avatar ?? null,
      }
    : null;

  // Presence helpers.
  $: onlineEmployees = Object.values($online).filter((k) => k === "user").length;
  const isClientOnline = (r: RoomSummary): boolean => !!(r.client_id && $online[r.client_id]);
  // Online dot for the header: meaningful in a client room or a DM (the peer).
  $: currentRoomOnline =
    current && (current.kind === "client" || current.kind === "direct")
      ? isClientOnline(current)
      : null;

  function openDrawer(): void {
    drawerOpen = true;
    loadRooms(); // refresh in case new client rooms appeared
  }
  function pick(id: string): void {
    joinRoom(id);
    drawerOpen = false;
  }
  // Mobile: open a chat as a pushed screen (back returns to the list).
  function openRoom(id: string): void {
    joinRoom(id);
    history.pushState({ zRoom: id }, "");
    roomOpen = true;
  }
  function mobileBack(): void {
    history.back(); // popstate handler drops roomOpen → list
  }
</script>

{#if view === "admin"}
  <Principals onBack={() => (view = "chat")} />
{:else}
  <div class="relative h-dvh w-full max-w-full overflow-hidden bg-ink">
    <div
      class="grid h-dvh w-full max-w-full grid-cols-[minmax(0,1fr)] grid-rows-[auto_1fr_auto] overflow-x-hidden bg-ink font-sans text-[0.9375rem] text-text"
      inert={mobile && isEmployee && !roomOpen}
    >
      <Header
        onOpenAdmin={() => (view = "admin")}
        {isEmployee}
        roomTitle={currentTitle}
        unreadTotal={totalUnread}
        onOpenRooms={openDrawer}
        roomOnline={currentRoomOnline}
        mode={mobile && isEmployee ? "room" : "drawer"}
        onBack={mobileBack}
        {peer}
      />

    <main
      bind:this={logEl}
      onscroll={onScroll}
      class="overflow-x-hidden overflow-y-auto py-3"
      aria-live="polite"
    >
      {#if $messages.length === 0}
        <p class="px-6 py-10 font-mono text-[0.82rem] text-muted">{$t("empty")}</p>
      {:else}
        {#each $messages as m, i (m.id)}
          {@const prev = i > 0 ? $messages[i - 1] : null}
          {@const newDay = !prev || !sameDay(prev.created_at, m.created_at)}
          {#if newDay}
            <div class="my-2 flex justify-center">
              <span
                class="rounded-full bg-surface-2 px-3 py-0.5 font-mono text-[0.7rem] text-muted"
              >
                {dayLabel(m.created_at)}
              </span>
            </div>
          {/if}
          <!-- Start a new author group on a new day, a new author, or after a
               5-minute lull — so the name + gap only print once per run. -->
          {@const firstInGroup =
            newDay ||
            !prev ||
            prev.author_id !== m.author_id ||
            m.created_at - prev.created_at > 5 * 60 * 1000}
          <Message {m} {firstInGroup} />
        {/each}
      {/if}
    </main>

      <Composer />
    </div>

    <!-- Mobile chat-list home: slides over the conversation; tapping a chat
         pushes it (history), back pops to here, back here minimizes (native). -->
    {#if mobile && isEmployee}
      <div
        class="absolute inset-0 z-30 grid grid-rows-[auto_1fr] overflow-hidden bg-ink font-sans text-[0.9375rem] text-text transition-transform duration-200 will-change-transform"
        style:transform={roomOpen ? "translateX(-100%)" : "translateX(0)"}
        inert={roomOpen}
      >
        <Header onOpenAdmin={() => (view = "admin")} {isEmployee} unreadTotal={totalUnread} mode="list" />
        <ChatList onPick={openRoom} />
      </div>
    {/if}
  </div>

  <!-- Jump to latest: shown only when scrolled up from the bottom -->
  {#if !pinned && (!mobile || roomOpen)}
    <button
      type="button"
      onclick={scrollToBottom}
      aria-label={$t("scrollToBottom")}
      title={$t("scrollToBottom")}
      transition:fly={{ y: 12, duration: 150 }}
      class="fixed bottom-24 right-4 z-30 grid size-11 cursor-pointer place-items-center rounded-full border border-line bg-surface-2 text-lg text-muted shadow-lg hover:border-beacon hover:text-beacon"
    >
      ↓
    </button>
  {/if}

  <!-- Voice call: floating button / active bar / incoming ring (all fixed) -->
  <Call />

  <!-- In-app image viewer (opened from message attachments) -->
  <Lightbox />

  <!-- Long-press / click context menu for a message (reply, …) -->
  <MessageMenu />

  <!-- Profile viewer (avatar + saved items) -->
  {#if $profileTarget}
    <Profile target={$profileTarget} onClose={closeProfile} />
  {/if}

  <!-- Notification toasts: new client messages, plus quiet reaction nudges -->
  {#if $toasts.length > 0 || $reactionToasts.length > 0}
    <div
      class="pointer-events-none fixed left-1/2 top-[calc(0.6rem+env(safe-area-inset-top))] z-50 flex w-[min(26rem,92vw)] -translate-x-1/2 flex-col gap-2"
    >
      {#each $toasts as toast (toast.id)}
        <div
          transition:fly={{ y: -16, duration: 180 }}
          class="pointer-events-auto flex items-center gap-2 rounded-md border border-beacon bg-surface px-3 py-2 shadow-lg"
        >
          <button
            type="button"
            onclick={() => openToast(toast)}
            class="flex min-w-0 flex-1 items-center gap-2 text-left"
          >
            <span class="text-base leading-none">💬</span>
            <span class="min-w-0">
              <span class="block truncate font-mono text-[0.78rem] text-beacon">{toast.from_name}</span>
              <span class="block truncate text-[0.82rem] text-muted">{toast.preview}</span>
            </span>
          </button>
          <button
            type="button"
            onclick={() => toggleMute(toast.room_id)}
            aria-label={$t("muteRoom")}
            title={$t("muteRoom")}
            class="grid size-7 shrink-0 cursor-pointer place-items-center rounded text-muted hover:text-text"
          >
            🔕
          </button>
          <button
            type="button"
            onclick={() => dismissToast(toast.id)}
            aria-label={$t("dismiss")}
            class="grid size-7 shrink-0 cursor-pointer place-items-center rounded text-muted hover:text-text"
          >
            ✕
          </button>
        </div>
      {/each}
      <!-- Reaction nudges: a light heart + who, no message preview or mute. -->
      {#each $reactionToasts as rt (rt.id)}
        <div
          transition:fly={{ y: -16, duration: 180 }}
          class="pointer-events-auto flex items-center gap-2 self-center rounded-full border border-line bg-surface px-3 py-1.5 shadow-lg"
        >
          <button
            type="button"
            onclick={() => openReactionToast(rt)}
            class="flex min-w-0 items-center gap-2 text-left"
          >
            <span class="text-lg leading-none">{rt.emoji}</span>
            <span class="truncate text-[0.82rem] text-muted">
              <span class="text-beacon">{rt.from_name}</span>
            </span>
          </button>
          <button
            type="button"
            onclick={() => dismissReactionToast(rt.id)}
            aria-label={$t("dismiss")}
            class="grid size-6 shrink-0 cursor-pointer place-items-center rounded text-muted hover:text-text"
          >
            ✕
          </button>
        </div>
      {/each}
    </div>
  {/if}

  <!-- Error toast -->
  {#if $notice}
    <div
      role="alert"
      class="fixed bottom-20 left-1/2 z-50 flex max-w-[90vw] -translate-x-1/2 items-center gap-3 rounded-md border border-bad bg-surface-2 px-4 py-2 text-[0.85rem] text-bad shadow-lg"
    >
      <span>{$notice}</span>
      <button
        type="button"
        onclick={dismissNotice}
        aria-label={$t("dismiss")}
        class="grid size-6 shrink-0 cursor-pointer place-items-center rounded text-muted hover:text-text"
      >
        ✕
      </button>
    </div>
  {/if}

  <!-- Rooms drawer (employees, desktop). Mobile uses the full-screen ChatList. -->
  {#if drawerOpen && !mobile}
    <button
      type="button"
      aria-label={$t("dismiss")}
      onclick={() => (drawerOpen = false)}
      class="fixed inset-0 z-30 cursor-default bg-black/40"
    ></button>
    <aside
      class="fixed left-0 top-0 z-40 flex h-dvh w-72 max-w-[85vw] flex-col gap-1 overflow-y-auto border-r border-line bg-surface p-3 pt-[calc(0.75rem+env(safe-area-inset-top))] text-text"
    >
      <div class="mb-2 px-1 font-mono text-[0.72rem] uppercase tracking-[0.1em] text-muted">
        {$t("rooms")}
      </div>
      {#if $rooms.length === 0}
        <p class="px-1 font-mono text-[0.8rem] text-muted">{$t("noRooms")}</p>
      {:else}
        {#each $rooms as r (r.id)}
          {@const count = $unread[r.id] ?? 0}
          {@const muted = $mutedRooms.has(r.id)}
          {@const live = r.kind === "common" ? onlineEmployees > 0 : isClientOnline(r)}
          <div class="flex items-center gap-1">
            <button
              type="button"
              onclick={() => pick(r.id)}
              aria-current={r.id === $activeRoom}
              class="flex flex-1 items-center gap-2 rounded-md px-3 py-2 text-left text-[0.9rem] hover:bg-surface-2 aria-[current=true]:bg-surface-2 aria-[current=true]:text-beacon"
            >
              <span
                class="size-2 shrink-0 rounded-full {live ? 'bg-emerald-400' : 'bg-muted/40'}"
                title={live ? "online" : "offline"}
              ></span>
              <span class="flex-1 truncate">{roomLabel(r, $t("room"))}</span>
              {#if r.kind === "common" && onlineEmployees > 0}
                <span class="shrink-0 font-mono text-[0.72rem] text-muted">{onlineEmployees}</span>
              {/if}
              {#if count > 0}
                <span
                  class="grid min-w-5 shrink-0 place-items-center rounded-full bg-beacon px-1.5 text-[0.7rem] font-medium text-[#1a1206]"
                  class:opacity-50={muted}
                >
                  {count}
                </span>
              {/if}
            </button>
            {#if r.kind === "client"}
              <button
                type="button"
                onclick={() => toggleMute(r.id)}
                aria-label={muted ? $t("unmuteRoom") : $t("muteRoom")}
                aria-pressed={muted}
                title={muted ? $t("unmuteRoom") : $t("muteRoom")}
                class="grid size-8 shrink-0 cursor-pointer place-items-center rounded-md text-muted hover:text-text"
              >
                {muted ? "🔕" : "🔔"}
              </button>
            {/if}
          </div>
        {/each}
      {/if}
    </aside>
  {/if}
{/if}
