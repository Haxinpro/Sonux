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
    const focusedRestart = (event: KeyboardEvent) => {
      if (event.ctrlKey && event.altKey && !event.shiftKey && event.key.toLowerCase() === "r") {
        event.preventDefault();
        restartApplication();
      }
    };
    window.addEventListener("keydown", focusedRestart);

    if (!enabled) {
      return () => window.removeEventListener("keydown", focusedRestart);
    }

    const entries = Object.entries(bindings) as [ShortcutAction, string][];
    const normalized = entries.map(([, shortcut]) => shortcut.toLowerCase());
    if (new Set(normalized).size !== normalized.length) {
      useMixerStore.setState({ error: "Each global shortcut must use a different key combination." });
      return () => window.removeEventListener("keydown", focusedRestart);
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
          error: `Could not register ${failed.join(", ")} globally. Other shortcuts remain active; Ctrl+Alt+R still works while Sonux is focused.`,
        });
      }
    };

    void setup();
    return () => {
      disposed = true;
      window.removeEventListener("keydown", focusedRestart);
      if (registered.length > 0) void unregister(registered).catch(() => {});
    };
  }, [enabled, bindings]);
}
