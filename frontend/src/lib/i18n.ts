// Localization as Svelte stores. Default language is Russian; English is the
// fallback/second. Use `$lang` for the current language and `$t('key')` in markup.
import { writable, derived } from "svelte/store";

export type Lang = "ru" | "en";

const ru = {
  title: "Zenithar — командная",
  room: "командная",
  rooms: "Чаты",
  noRooms: "Пока нет чатов.",
  connecting: "подключение",
  live: "на связи",
  reconnecting: "переподключение",
  empty: "Пока тишина. Напишите команде.",
  you: "вы",
  messagePlaceholder: "Написать команде…",
  messageAria: "Сообщение",
  send: "Отправить",
  attach: "Прикрепить файл",
  emoji: "Эмодзи",
  recordVoice: "Записать голосовое",
  stopRecording: "Остановить и отправить",
  cancelRecording: "Отмена",
  recording: "Запись…",
  uploading: "Загрузка…",
  removeAttachment: "Убрать вложение",
  voiceMessage: "Голосовое сообщение",
  download: "Скачать",
  prevImage: "Предыдущее",
  nextImage: "Следующее",
  reply: "Ответить",
  cancelReply: "Отменить ответ",
  attachment: "Вложение",
  errSend: "Нет соединения — сообщение не отправлено",
  errUpload: "Не удалось загрузить файл",
  errTooMany: "Можно прикрепить до 5 файлов",
  dismiss: "Закрыть",
  // calls
  call: "Позвонить",
  callConnecting: "Соединение…",
  callRingingTitle: "Входящий звонок",
  callFrom: "звонит",
  inCall: "Звонок",
  accept: "Принять",
  decline: "Отклонить",
  hangup: "Завершить",
  mute: "Заглушить",
  unmute: "Включить микрофон",
  // session
  landingTitle: "Вход по ссылке",
  landingHint: "Откройте свою персональную ссылку-приглашение, чтобы войти.",
  logout: "Выйти",
  menu: "Меню",
  editNameAria: "Изменить имя",
  saveName: "Сохранить",
  // admin
  adminLinks: "Ссылки",
  back: "Назад",
  newLink: "Новая ссылка",
  roleUser: "Сотрудник",
  roleClient: "Анонимный клиент",
  nameOptional: "Имя (необязательно)",
  create: "Создать",
  copy: "Копировать",
  copied: "Скопировано",
  rotate: "Перевыпустить",
  revoke: "Отозвать",
  active: "активна",
  revoked: "отозвана",
  noPrincipals: "Пока нет ни одной ссылки. Создайте первую.",
  freshLink: "Свежая ссылка (показывается один раз)",
} as const;

const en: Record<keyof typeof ru, string> = {
  title: "Zenithar — team room",
  room: "team room",
  rooms: "Chats",
  noRooms: "No chats yet.",
  connecting: "connecting",
  live: "live",
  reconnecting: "reconnecting",
  empty: "No signals yet. Say something to the team.",
  you: "you",
  messagePlaceholder: "Message the team…",
  messageAria: "Message",
  send: "Send",
  attach: "Attach a file",
  emoji: "Emoji",
  recordVoice: "Record a voice message",
  stopRecording: "Stop and send",
  cancelRecording: "Cancel",
  recording: "Recording…",
  uploading: "Uploading…",
  removeAttachment: "Remove attachment",
  voiceMessage: "Voice message",
  download: "Download",
  prevImage: "Previous",
  nextImage: "Next",
  reply: "Reply",
  cancelReply: "Cancel reply",
  attachment: "Attachment",
  errSend: "No connection — message not sent",
  errUpload: "Upload failed",
  errTooMany: "Up to 5 attachments",
  dismiss: "Dismiss",
  call: "Call",
  callConnecting: "Connecting…",
  callRingingTitle: "Incoming call",
  callFrom: "is calling",
  inCall: "Call",
  accept: "Accept",
  decline: "Decline",
  hangup: "Hang up",
  mute: "Mute",
  unmute: "Unmute",
  landingTitle: "Sign in by link",
  landingHint: "Open your personal invite link to enter.",
  logout: "Log out",
  menu: "Menu",
  editNameAria: "Edit name",
  saveName: "Save",
  adminLinks: "Links",
  back: "Back",
  newLink: "New link",
  roleUser: "Employee",
  roleClient: "Anonymous client",
  nameOptional: "Name (optional)",
  create: "Create",
  copy: "Copy",
  copied: "Copied",
  rotate: "Reissue",
  revoke: "Revoke",
  active: "active",
  revoked: "revoked",
  noPrincipals: "No links yet. Create the first one.",
  freshLink: "Fresh link (shown once)",
};

export type Key = keyof typeof ru;

const dicts: Record<Lang, Record<Key, string>> = { ru, en };

const LANG_KEY = "zenithar.lang";

function readStored(): Lang {
  const s = localStorage.getItem(LANG_KEY);
  return s === "en" || s === "ru" ? s : "ru"; // default Russian
}

export const lang = writable<Lang>(readStored());

lang.subscribe((l) => {
  try {
    localStorage.setItem(LANG_KEY, l);
  } catch {
    /* private mode, ignore */
  }
  document.documentElement.lang = l;
  document.title = dicts[l].title;
});

export const t = derived(
  lang,
  ($l) =>
    (key: Key): string =>
      dicts[$l][key] ?? en[key] ?? String(key),
);
