// Session + admin API. Cookies are sent automatically (same-origin).
import { writable } from "svelte/store";

export interface Principal {
  id: string;
  kind: "user" | "client";
  display_name: string;
  is_admin: boolean;
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

export async function renameMe(display_name: string): Promise<boolean> {
  const r = await fetch("/api/me/name", {
    method: "POST",
    headers: json,
    body: JSON.stringify({ display_name }),
  });
  if (r.ok) me.update((m) => (m ? { ...m, display_name } : m));
  return r.ok;
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

export async function getTelemetry(): Promise<TelemetryInfo> {
  try {
    const r = await fetch("/api/admin/telemetry");
    return r.ok ? ((await r.json()) as TelemetryInfo) : { enabled: false, port: 4000 };
  } catch {
    return { enabled: false, port: 4000 };
  }
}
