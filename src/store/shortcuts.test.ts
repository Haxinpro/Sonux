import { beforeEach, describe, expect, it, vi } from "vitest";

describe("shortcut defaults", () => {
  beforeEach(() => {
    const values = new Map<string, string>();
    const storage: Storage = {
      get length() {
        return values.size;
      },
      clear: () => values.clear(),
      getItem: (key) => values.get(key) ?? null,
      key: (index) => [...values.keys()][index] ?? null,
      removeItem: (key) => void values.delete(key),
      setItem: (key, value) => void values.set(key, value),
    };
    Object.defineProperty(globalThis, "localStorage", {
      configurable: true,
      value: storage,
    });
    vi.resetModules();
  });

  it("requires an explicit opt-in on a fresh install", async () => {
    const { useShortcutSettings } = await import("./shortcuts");
    expect(useShortcutSettings.getState().enabled).toBe(false);
  });

  it("falls back to disabled when saved settings are corrupt", async () => {
    localStorage.setItem("sonux-global-shortcuts", "not-json");
    const { useShortcutSettings } = await import("./shortcuts");
    expect(useShortcutSettings.getState().enabled).toBe(false);
  });

  it("reset restores bindings without silently opting in", async () => {
    const { DEFAULT_SHORTCUTS, useShortcutSettings } = await import("./shortcuts");
    useShortcutSettings.getState().setEnabled(true);
    useShortcutSettings.getState().reset();
    expect(useShortcutSettings.getState().enabled).toBe(false);
    expect(useShortcutSettings.getState().bindings).toEqual(DEFAULT_SHORTCUTS);
  });

  it("preserves cleared bindings across reloads", async () => {
    const { EMPTY_SHORTCUTS, useShortcutSettings } = await import("./shortcuts");
    useShortcutSettings.getState().setBindings(EMPTY_SHORTCUTS);

    vi.resetModules();
    const reloaded = await import("./shortcuts");
    expect(reloaded.useShortcutSettings.getState().bindings).toEqual(EMPTY_SHORTCUTS);
  });

  it("allows one action to be cleared without changing the others", async () => {
    const { DEFAULT_SHORTCUTS, useShortcutSettings } = await import("./shortcuts");
    useShortcutSettings.getState().setBindings({ ...DEFAULT_SHORTCUTS, toggle_game: "   " });
    expect(useShortcutSettings.getState().bindings).toEqual({ ...DEFAULT_SHORTCUTS, toggle_game: "" });
  });

  it("reads settings saved under the legacy storage key", async () => {
    localStorage.setItem("sink-global-shortcuts", JSON.stringify({
      enabled: true,
      bindings: { toggle_game: "", toggle_chat: "Alt+C", toggle_mic: "Alt+M", restart_app: "" },
    }));
    const { useShortcutSettings } = await import("./shortcuts");
    expect(useShortcutSettings.getState().bindings.toggle_game).toBe("");
    expect(useShortcutSettings.getState().bindings.toggle_chat).toBe("Alt+C");
  });

  it("captures modifiers followed by a main key", async () => {
    const { shortcutFromKeyboardEvent } = await import("./shortcuts");
    expect(shortcutFromKeyboardEvent({
      altKey: true, code: "KeyM", ctrlKey: true, key: "m", metaKey: false, shiftKey: false,
    })).toBe("Ctrl+Alt+M");
  });

  it("waits when only a modifier is pressed", async () => {
    const { shortcutFromKeyboardEvent } = await import("./shortcuts");
    expect(shortcutFromKeyboardEvent({
      altKey: false, code: "ControlLeft", ctrlKey: true, key: "Control", metaKey: false, shiftKey: false,
    })).toBeNull();
  });

  it("uses the physical digit when Shift changes its printed character", async () => {
    const { shortcutFromKeyboardEvent } = await import("./shortcuts");
    expect(shortcutFromKeyboardEvent({
      altKey: false, code: "Digit1", ctrlKey: true, key: "!", metaKey: false, shiftKey: true,
    })).toBe("Ctrl+Shift+1");
  });

  it("clears on bare Backspace but allows modified Delete", async () => {
    const { shortcutFromKeyboardEvent } = await import("./shortcuts");
    expect(shortcutFromKeyboardEvent({
      altKey: false, code: "Backspace", ctrlKey: false, key: "Backspace", metaKey: false, shiftKey: false,
    })).toBe("");
    expect(shortcutFromKeyboardEvent({
      altKey: false, code: "Delete", ctrlKey: true, key: "Delete", metaKey: false, shiftKey: false,
    })).toBe("Ctrl+Delete");
  });
});
