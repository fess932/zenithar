// Session + admin API. Cookies are sent automatically (same-origin).
import { writable } from "svelte/store";

export interface Principal {
  id: string;
  kind: "user" | "client";
  display_name: string;
  is_admin: boolean;
  // Emoji, "photo:<millis>", or null → the client renders a default (see Avatar).
  avatar?: string | null;
}

export interface PrincipalSummary {
  id: string;
  kind: string;
  display_name: string;
  is_admin: boolean;
  created_at: number;
  active: boolean;
}

export interface Link {
  principal_id: string;
  url: string;
}

export const me = writable<Principal | null>(null);
export const loaded = writable(false);

const json = { "Content-Type": "application/json" };

export async function loadMe(): Promise<void> {
  try {
    const r = await fetch("/api/me");
    me.set(r.ok ? ((await r.json()) as Principal | null) : null);
  } catch {
    me.set(null);
  }
  loaded.set(true);
}

export async function logout(): Promise<void> {
  await fetch("/api/auth/logout", { method: "POST" });
  me.set(null);
  location.href = "/";
}

/// Mint a one-time link and hand off to the desktop/mobile app, logged in as you.
/// The deep link carries THIS host's full login URL (built from `location.origin`)
/// so the app logs into the right server — we run several hosts, each its own.
/// Opens the app if installed; nothing happens otherwise.
export async function openInApp(): Promise<void> {
  try {
    const r = await fetch("/api/me/app-link", { method: "POST" });
    if (!r.ok) return;
    const { web } = (await r.json()) as { web?: string };
    if (!web) return;
    const target = location.origin + web; // https://<this host>/i/<token>
    location.href = `zenithar://login?u=${encodeURIComponent(target)}`;
  } catch {
    /* offline or app not installed — no-op */
  }
}

/// Open (or create) the 1:1 direct room with another employee; returns its id.
export async function startDm(withId: string): Promise<string | null> {
  try {
    const r = await fetch("/api/dm", {
      method: "POST",
      headers: json,
      body: JSON.stringify({ with: withId }),
    });
    if (!r.ok) return null;
    const { room_id } = (await r.json()) as { room_id?: string };
    return room_id ?? null;
  } catch {
    return null;
  }
}

export async function renameMe(display_name: string): Promise<boolean> {
  const r = await fetch("/api/me/name", {
    method: "POST",
    headers: json,
    body: JSON.stringify({ display_name }),
  });
  if (r.ok) me.update((m) => (m ? { ...m, display_name } : m));
  return r.ok;
}

/// Set an emoji avatar, or clear it (null → back to the default). The server
/// echoes the stored value so we patch `me` without a refetch.
export async function setAvatarEmoji(value: string | null): Promise<boolean> {
  const r = await fetch("/api/me/avatar", {
    method: "POST",
    headers: json,
    body: JSON.stringify({ value }),
  });
  if (r.ok) {
    const { avatar } = (await r.json()) as { avatar: string | null };
    me.update((m) => (m ? { ...m, avatar } : m));
  }
  return r.ok;
}

/// Upload a photo avatar (server crops it square). Returns success.
export async function setAvatarPhoto(file: File): Promise<boolean> {
  const fd = new FormData();
  fd.append("file", file);
  const r = await fetch("/api/me/avatar/photo", { method: "POST", body: fd });
  if (r.ok) {
    const { avatar } = (await r.json()) as { avatar: string | null };
    me.update((m) => (m ? { ...m, avatar } : m));
  }
  return r.ok;
}

/// Clear any custom avatar (emoji or photo) → revert to the default emoji.
export async function resetAvatar(): Promise<boolean> {
  return setAvatarEmoji(null);
}

export async function listPrincipals(): Promise<PrincipalSummary[]> {
  const r = await fetch("/api/principals");
  return r.ok ? ((await r.json()) as PrincipalSummary[]) : [];
}

export async function createPrincipal(
  kind: "user" | "client",
  display?: string,
): Promise<Link | null> {
  const r = await fetch("/api/principals", {
    method: "POST",
    headers: json,
    body: JSON.stringify({ kind, display: display || undefined }),
  });
  return r.ok ? ((await r.json()) as Link) : null;
}

export async function rotateLink(id: string): Promise<Link | null> {
  const r = await fetch(`/api/principals/${id}/rotate`, { method: "POST" });
  return r.ok ? ((await r.json()) as Link) : null;
}

export async function revokeLink(id: string): Promise<void> {
  await fetch(`/api/principals/${id}/revoke`, { method: "POST" });
}

// ---- integrations (REST API tokens) ---------------------------------------

export interface IntegrationSummary {
  id: string;
  name: string;
  created_at: number;
  last_used_at: number | null;
  active: boolean;
}

export interface IntegrationToken {
  id: string;
  name: string;
  token: string; // plaintext, shown once
}

export async function listIntegrations(): Promise<IntegrationSummary[]> {
  const r = await fetch("/api/integrations");
  return r.ok ? ((await r.json()) as IntegrationSummary[]) : [];
}

export async function createIntegration(
  name: string,
): Promise<IntegrationToken | null> {
  const r = await fetch("/api/integrations", {
    method: "POST",
    headers: json,
    body: JSON.stringify({ name }),
  });
  return r.ok ? ((await r.json()) as IntegrationToken) : null;
}

export async function rotateIntegration(
  id: string,
): Promise<IntegrationToken | null> {
  const r = await fetch(`/api/integrations/${id}/rotate`, { method: "POST" });
  return r.ok ? ((await r.json()) as IntegrationToken) : null;
}

export async function revokeIntegration(id: string): Promise<void> {
  await fetch(`/api/integrations/${id}/revoke`, { method: "POST" });
}

// ---- call recordings -------------------------------------------------------

export interface RecordingTrack {
  participant_id: string;
  participant_name: string;
  url: string;
}

export interface Recording {
  call_id: string;
  room_title: string | null; // client name; null = common room
  started_by_name: string | null;
  started_at: number;
  ended_at: number | null;
  tracks: RecordingTrack[];
}

export async function listRecordings(): Promise<Recording[]> {
  const r = await fetch("/api/admin/recordings");
  return r.ok ? ((await r.json()) as Recording[]) : [];
}

// ---- telemetry dashboard ---------------------------------------------------

export interface TelemetryInfo {
  enabled: boolean;
  port: number;
}

// ---- connections (presence list) -------------------------------------------

export interface Person {
  id: string;
  name: string;
  kind: string;
  online: boolean;
  last_seen: number | null; // unix millis of last activity
  ping_ms: number | null; // last WS round-trip (online only)
  avatar?: string | null; // emoji / "photo:<millis>" / null → default
}

export async function listPeople(): Promise<Person[]> {
  try {
    const r = await fetch("/api/people");
    return r.ok ? ((await r.json()) as Person[]) : [];
  } catch {
    return [];
  }
}

export async function getTelemetry(): Promise<TelemetryInfo> {
  try {
    const r = await fetch("/api/admin/telemetry");
    return r.ok ? ((await r.json()) as TelemetryInfo) : { enabled: false, port: 4000 };
  } catch {
    return { enabled: false, port: 4000 };
  }
}
