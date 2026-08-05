import { create } from "zustand";

export type ShortcutAction = "toggle_game" | "toggle_chat" | "toggle_mic" | "restart_app";

export type ShortcutBindings = Record<ShortcutAction, string>;

export const DEFAULT_SHORTCUTS: ShortcutBindings = {
  toggle_game: "Ctrl+Alt+G",
  toggle_chat: "Ctrl+Alt+C",
  toggle_mic: "Ctrl+Alt+M",
  restart_app: "Ctrl+Alt+R",
};

const STORAGE_KEY = "sonux-global-shortcuts";

interface StoredShortcutSettings {
  enabled: boolean;
  bindings: ShortcutBindings;
}

function readSettings(): StoredShortcutSettings {
  try {
    const saved = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "null") as Partial<StoredShortcutSettings> | null;
    return {
      enabled: saved?.enabled ?? true,
      bindings: {
        toggle_game: saved?.bindings?.toggle_game || DEFAULT_SHORTCUTS.toggle_game,
        toggle_chat: saved?.bindings?.toggle_chat || DEFAULT_SHORTCUTS.toggle_chat,
        toggle_mic: saved?.bindings?.toggle_mic || DEFAULT_SHORTCUTS.toggle_mic,
        restart_app: saved?.bindings?.restart_app || DEFAULT_SHORTCUTS.restart_app,
      },
    };
  } catch {
    return { enabled: true, bindings: { ...DEFAULT_SHORTCUTS } };
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
    save({ enabled: true, bindings });
    set({ enabled: true, bindings });
  },
}));
