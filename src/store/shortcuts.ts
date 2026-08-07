import { create } from "zustand";

export type ShortcutAction = "toggle_game" | "toggle_chat" | "toggle_mic" | "restart_app";

export type ShortcutBindings = Record<ShortcutAction, string>;

export const DEFAULT_SHORTCUTS: ShortcutBindings = {
  toggle_game: "Ctrl+Alt+G",
  toggle_chat: "Ctrl+Alt+C",
  toggle_mic: "Ctrl+Alt+M",
  restart_app: "Ctrl+Alt+R",
};

export const EMPTY_SHORTCUTS: ShortcutBindings = {
  toggle_game: "",
  toggle_chat: "",
  toggle_mic: "",
  restart_app: "",
};

export const DEFAULT_SHORTCUTS_ENABLED = false;

type ShortcutKeyboardEvent = Pick<
  KeyboardEvent,
  "altKey" | "code" | "ctrlKey" | "key" | "metaKey" | "shiftKey"
>;

const MODIFIER_CODES = new Set([
  "AltLeft", "AltRight", "ControlLeft", "ControlRight",
  "MetaLeft", "MetaRight", "ShiftLeft", "ShiftRight",
]);

const NAMED_SHORTCUT_CODES = new Set([
  "ArrowDown", "ArrowLeft", "ArrowRight", "ArrowUp",
  "Backquote", "Backslash", "BracketLeft", "BracketRight",
  "CapsLock", "Comma", "End", "Enter", "Equal", "Escape",
  "Home", "Insert", "Minus", "PageDown", "PageUp", "Pause",
  "Period", "PrintScreen", "Quote", "ScrollLock", "Semicolon",
  "Slash", "Space", "Tab",
  "AudioVolumeDown", "AudioVolumeMute", "AudioVolumeUp",
  "MediaPause", "MediaPlay", "MediaPlayPause", "MediaStop",
  "MediaTrackNext", "MediaTrackPrevious",
  "NumLock", "Numpad0", "Numpad1", "Numpad2", "Numpad3", "Numpad4",
  "Numpad5", "Numpad6", "Numpad7", "Numpad8", "Numpad9",
  "NumpadAdd", "NumpadDecimal", "NumpadDivide", "NumpadEnter",
  "NumpadEqual", "NumpadMultiply", "NumpadSubtract",
]);

function shortcutKey(code: string): string | null {
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit[0-9]$/.test(code)) return code.slice(5);
  if (/^F(?:[1-9]|1[0-9]|2[0-4])$/.test(code)) return code;
  return NAMED_SHORTCUT_CODES.has(code) ? code : null;
}

/** Convert a physical key press to syntax accepted by Tauri's global-hotkey
 * parser. `null` means the recorder should keep waiting for a main key; an
 * empty string means the user explicitly cleared the binding. */
export function shortcutFromKeyboardEvent(event: ShortcutKeyboardEvent): string | null {
  const hasModifier = event.ctrlKey || event.altKey || event.shiftKey || event.metaKey;
  if (!hasModifier && (event.key === "Backspace" || event.key === "Delete")) return "";
  if (!hasModifier && event.key === "Escape") return null;
  if (MODIFIER_CODES.has(event.code)) return null;

  const key = event.code === "Backspace" || event.code === "Delete"
    ? event.code
    : shortcutKey(event.code);
  if (!key) return null;

  const modifiers: string[] = [];
  if (event.ctrlKey) modifiers.push("Ctrl");
  if (event.altKey) modifiers.push("Alt");
  if (event.shiftKey) modifiers.push("Shift");
  if (event.metaKey) modifiers.push("Super");
  return [...modifiers, key].join("+");
}

const STORAGE_KEY = "sonux-global-shortcuts";
const LEGACY_STORAGE_KEY = "sink-global-shortcuts";

interface StoredShortcutSettings {
  enabled: boolean;
  bindings: ShortcutBindings;
}

function readSettings(): StoredShortcutSettings {
  try {
    const raw = localStorage.getItem(STORAGE_KEY) ?? localStorage.getItem(LEGACY_STORAGE_KEY);
    const saved = JSON.parse(raw ?? "null") as Partial<StoredShortcutSettings> | null;
    const readBinding = (action: ShortcutAction) => {
      const binding = saved?.bindings?.[action];
      return typeof binding === "string" ? binding : DEFAULT_SHORTCUTS[action];
    };
    return {
      enabled: typeof saved?.enabled === "boolean" ? saved.enabled : DEFAULT_SHORTCUTS_ENABLED,
      bindings: {
        toggle_game: readBinding("toggle_game"),
        toggle_chat: readBinding("toggle_chat"),
        toggle_mic: readBinding("toggle_mic"),
        restart_app: readBinding("restart_app"),
      },
    };
  } catch {
    return { enabled: DEFAULT_SHORTCUTS_ENABLED, bindings: { ...DEFAULT_SHORTCUTS } };
  }
}

function save(settings: StoredShortcutSettings) {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(settings));
}

interface ShortcutState extends StoredShortcutSettings {
  setEnabled: (enabled: boolean) => void;
  setBindings: (bindings: ShortcutBindings) => void;
  reset: () => void;
}

export const useShortcutSettings = create<ShortcutState>((set, get) => ({
  ...readSettings(),
  setEnabled: (enabled) => {
    const next = { enabled, bindings: get().bindings };
    save(next);
    set({ enabled });
  },
  setBindings: (bindings) => {
    const normalized = Object.fromEntries(
      Object.entries(bindings).map(([action, shortcut]) => [action, shortcut.trim()]),
    ) as ShortcutBindings;
    save({ enabled: get().enabled, bindings: normalized });
    set({ bindings: normalized });
  },
  reset: () => {
    const bindings = { ...DEFAULT_SHORTCUTS };
    save({ enabled: DEFAULT_SHORTCUTS_ENABLED, bindings });
    set({ enabled: DEFAULT_SHORTCUTS_ENABLED, bindings });
  },
}));
