import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { publishLevels, type Levels } from "../lib/liveMeters";
import { useMixerStore } from "../store/mixer";

const FAST_POLL_INTERVAL_MS = 500;
const SLOW_POLL_INTERVAL_MS = 2000;

/**
 * Boots the audio layer: creates the virtual sinks on mount, polls the app
 * stream/channel controls quickly while visible, polls slower device/history
 * state every 2s, subscribes to live VU levels, and auto-loads profiles bound
 * to newly connected devices (Phase 5). A native worker keeps application
 * discovery and auto-routing alive while the window is hidden in the tray.
 */
export function useAudio() {
  const initialize = useMixerStore((s) => s.initialize);
  const fetchAppStreams = useMixerStore((s) => s.fetchAppStreams);
  const fetchChannels = useMixerStore((s) => s.fetchChannels);
  const fetchOutputs = useMixerStore((s) => s.fetchOutputs);
  const fetchMicClients = useMixerStore((s) => s.fetchMicClients);
  const fetchSeenApps = useMixerStore((s) => s.fetchSeenApps);
  const synchronizeStartupState = useMixerStore((s) => s.synchronizeStartupState);

  useEffect(() => {
    void initialize();
    let fastId: ReturnType<typeof setInterval> | undefined;
    let slowId: ReturnType<typeof setInterval> | undefined;
    let fastInFlight = false;
    let slowInFlight = false;
    const fastPoll = async () => {
      if (fastInFlight) return;
      fastInFlight = true;
      try {
        await Promise.all([fetchAppStreams(), fetchChannels()]);
      } finally {
        fastInFlight = false;
      }
    };
    const slowPoll = async () => {
      if (slowInFlight) return;
      slowInFlight = true;
      try {
        const state = useMixerStore.getState();
        await Promise.all([
          fetchOutputs(),
          fetchMicClients(),
          fetchSeenApps(),
          ...(!state.initialized
            ? [initialize()]
            : !state.startupSynchronized
              ? [synchronizeStartupState()]
              : []),
        ]);
      } finally {
        slowInFlight = false;
      }
    };
    const start = () => {
      if (fastId === undefined) {
        void fastPoll(); // refresh immediately so a returning window isn't stale
        void slowPoll();
        fastId = setInterval(() => void fastPoll(), FAST_POLL_INTERVAL_MS);
        slowId = setInterval(() => void slowPoll(), SLOW_POLL_INTERVAL_MS);
      }
    };
    const stop = () => {
      if (fastId !== undefined) {
        clearInterval(fastId);
        fastId = undefined;
      }
      if (slowId !== undefined) {
        clearInterval(slowId);
        slowId = undefined;
      }
    };
    // Pause polling while hidden in the tray - the product's dominant idle
    // state - instead of round-tripping forever (TD-009).
    const onVisibility = () => (document.hidden ? stop() : start());
    if (!document.hidden) start();
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      stop();
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [
    initialize,
    fetchAppStreams,
    fetchChannels,
    fetchOutputs,
    fetchMicClients,
    fetchSeenApps,
    synchronizeStartupState,
  ]);

  useEffect(() => {
    const unlisten = listen<Levels>("levels", (event) => {
      // Meter peaks bypass Zustand: changing a level must not re-render every
      // control and app list in its mixer strip.
      if (useMixerStore.getState().meterMode !== "off") publishLevels(event.payload);
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, []);

  // Profile switched from the tray menu - sync the whole UI.
  const onProfileChanged = useMixerStore((s) => s.onProfileChanged);
  useEffect(() => {
    const unlisten = listen<string>("profile-changed", (event) => {
      void onProfileChanged(event.payload);
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [onProfileChanged]);
}
