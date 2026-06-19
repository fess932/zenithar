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
