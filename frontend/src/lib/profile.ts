// The profile viewer overlay: who to show (id + display info), or null when
// closed. We pass the name/avatar in (from `me`, or from a message's author) so
// the viewer needs no extra "get principal" endpoint.
import { writable } from "svelte/store";

export interface ProfileTarget {
  id: string;
  name: string;
  avatar: string | null;
}

export const profileTarget = writable<ProfileTarget | null>(null);

export function openProfile(t: ProfileTarget): void {
  profileTarget.set(t);
}

export function closeProfile(): void {
  profileTarget.set(null);
}
