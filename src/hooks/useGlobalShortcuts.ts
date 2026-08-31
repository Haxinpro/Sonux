import { useEffect } from "react";
import { register, unregister } from "@tauri-apps/plugin-global-shortcut";
import { invoke } from "@tauri-apps/api/core";
import { useMixerStore } from "../store/mixer";
import { useShortcutSettings, type ShortcutAction } from "../store/shortcuts";
import { translate } from "../i18nCore";

let restartPending = false;

function currentTranslation(key: Parameters<typeof translate>[1], variables: Parameters<typeof translate>[2] = {}) {
  return translate(document.documentElement.lang || "en", key, variables);
}

export function restartApplication() {
  if (restartPending) return;
  restartPending = true;
  void invoke("restart_app").catch((cause) => {
    restartPending = false;
    useMixerStore.setState({ error: currentTranslation("errors.restartFailed", { cause: String(cause) }) });
  });
}

function runShortcut(action: ShortcutAction) {
  if (action === "restart_app") {
    restartApplication();
    return;
  }
  const mixer = useMixerStore.getState();
  if (action === "toggle_mic") {
    if (mixer.micConfig) void mixer.setMicConfig({ muted: !mixer.micConfig.muted });
    return;
  }

  const name = action === "toggle_game" ? "sink_game" : "sink_chat";
  const channel = mixer.channels.find((candidate) => candidate.name === name);
  if (channel) void mixer.toggleMute(channel.name, !channel.muted);
}

export function useGlobalShortcuts() {
  const enabled = useShortcutSettings((state) => state.enabled);
  const bindings = useShortcutSettings((state) => state.bindings);

  useEffect(() => {
    if (!enabled) return;

    const entries = (Object.entries(bindings) as [ShortcutAction, string][])
      .filter(([, shortcut]) => shortcut.length > 0);
    const normalized = entries.map(([, shortcut]) => shortcut.toLowerCase());
    if (new Set(normalized).size !== normalized.length) {
      useMixerStore.setState({ error: currentTranslation("errors.shortcutsDuplicate") });
      return;
    }

    let disposed = false;
    const registered: string[] = [];

    const setup = async () => {
      const failed: string[] = [];
      for (const [action, shortcut] of entries) {
        try {
          await register(shortcut, (event) => {
            if (event.state === "Pressed") runShortcut(action);
          });
          if (disposed) {
            await unregister(shortcut);
            return;
          }
          registered.push(shortcut);
        } catch {
          failed.push(shortcut);
        }
      }
      if (!disposed && failed.length > 0) {
        useMixerStore.setState({
          error: currentTranslation("errors.shortcutsRegistration", { shortcuts: failed.join(", ") }),
        });
      }
    };

    void setup();
    return () => {
      disposed = true;
      if (registered.length > 0) void unregister(registered).catch(() => {});
    };
  }, [enabled, bindings]);
}
