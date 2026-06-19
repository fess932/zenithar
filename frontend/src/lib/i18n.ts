// Localization as Svelte stores. Default language is Russian; English is the
// fallback/second. Use `$lang` for the current language and `$t('key')` in markup.
import { writable, derived } from "svelte/store";

export type Lang = "ru" | "en";

const ru = {
  title: "Zenithar — командная",
  room: "командная",
  connecting: "подключение",
  live: "на связи",
  reconnecting: "переподключение",
  empty: "Пока тишина. Напишите команде.",
  you: "вы",
  namePlaceholder: "вы",
  nameAria: "Ваше имя",
  messagePlaceholder: "Написать команде…",
  messageAria: "Сообщение",
  send: "Отправить",
} as const;

const en: Record<keyof typeof ru, string> = {
  title: "Zenithar — team room",
  room: "team room",
  connecting: "connecting",
  live: "live",
  reconnecting: "reconnecting",
  empty: "No signals yet. Say something to the team.",
  you: "you",
  namePlaceholder: "you",
  nameAria: "Your name",
  messagePlaceholder: "Message the team…",
  messageAria: "Message",
  send: "Send",
};

export type Key = keyof typeof ru;

const dicts: Record<Lang, Record<Key, string>> = { ru, en };

const LANG_KEY = "zenithar.lang";

function readStored(): Lang {
  const s = localStorage.getItem(LANG_KEY);
  return s === "en" || s === "ru" ? s : "ru"; // default Russian
}

export const lang = writable<Lang>(readStored());

// Persist the choice and keep document chrome in sync.
lang.subscribe((l) => {
  try {
    localStorage.setItem(LANG_KEY, l);
  } catch {
    /* private mode, ignore */
  }
  document.documentElement.lang = l;
  document.title = dicts[l].title;
});

/** Reactive translator: `$t('send')` re-renders when the language changes. */
export const t = derived(
  lang,
  ($l) =>
    (key: Key): string =>
      dicts[$l][key] ?? en[key] ?? String(key),
);
