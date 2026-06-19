// A single message context menu (Telegram-style long-press / right-click). One
// menu is open at a time, anchored at the press coordinates.
import { writable } from "svelte/store";
import type { ChatMessage } from "./chat";

export interface MessageMenuState {
  message: ChatMessage;
  x: number;
  y: number;
}

export const messageMenu = writable<MessageMenuState | null>(null);

export function openMessageMenu(message: ChatMessage, x: number, y: number): void {
  messageMenu.set({ message, x, y });
}

export function closeMessageMenu(): void {
  messageMenu.set(null);
}
