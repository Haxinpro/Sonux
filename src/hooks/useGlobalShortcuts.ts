import { useEffect } from "react";
import { register, unregister } from "@tauri-apps/plugin-global-shortcut";
import { invoke } from "@tauri-apps/api/core";
import { useMixerStore } from "../store/mixer";
import { useShortcutSettings, type ShortcutAction } from "../store/shortcuts";

let restartPending = false;

export function restartApplication() {
  if (restartPending) return;
  restartPending = true;
  void invoke("restart_app").catch((cause) => {
    restartPending = false;
    useMixerStore.setState({ error: `Could not restart Sonux: ${String(cause)}` });
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
      useMixerStore.setState({ error: "Each global shortcut must use a different key combination." });
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
          error: `Could not register ${failed.join(", ")} globally. Other shortcuts remain active.`,
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
