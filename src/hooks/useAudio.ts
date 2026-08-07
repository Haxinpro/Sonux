import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { useMixerStore, type Levels } from "../store/mixer";

const FAST_POLL_INTERVAL_MS = 500;
const SLOW_POLL_INTERVAL_MS = 2000;

/**
 * Boots the audio layer: creates the virtual sinks on mount, polls the app
 * stream/channel controls quickly (also the auto-route enforcement trigger),
 * polls slower device/history state every 2s, subscribes to live VU levels,
 * and auto-loads profiles bound to newly connected devices (Phase 5).
 */
export function useAudio() {
  const initialize = useMixerStore((s) => s.initialize);
  const fetchAppStreams = useMixerStore((s) => s.fetchAppStreams);
  const fetchChannels = useMixerStore((s) => s.fetchChannels);
  const fetchOutputs = useMixerStore((s) => s.fetchOutputs);
  const fetchMicClients = useMixerStore((s) => s.fetchMicClients);
  const fetchSeenApps = useMixerStore((s) => s.fetchSeenApps);
  const setLevels = useMixerStore((s) => s.setLevels);
  const outputDevices = useMixerStore((s) => s.outputDevices);
  const profiles = useMixerStore((s) => s.profiles);
  const loadProfile = useMixerStore((s) => s.loadProfile);

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
        await Promise.all([fetchOutputs(), fetchMicClients(), fetchSeenApps()]);
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
  }, [initialize, fetchAppStreams, fetchChannels, fetchOutputs, fetchMicClients, fetchSeenApps]);

  useEffect(() => {
    const unlisten = listen<Levels>("levels", (event) => setLevels(event.payload));
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [setLevels]);

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

  // Hardware profile auto-switch: when a device with a bound profile
  // appears, load that profile.
  const seenDevices = useRef<Set<string> | null>(null);
  useEffect(() => {
    const names = new Set(outputDevices.map((d) => d.name));
    if (seenDevices.current === null) {
      // First sample: just learn the current device set.
      if (names.size > 0) seenDevices.current = names;
      return;
    }
    for (const name of names) {
      if (!seenDevices.current.has(name)) {
        const bound = profiles.find((p) => p.trigger_device === name);
        if (bound) void loadProfile(bound.name);
      }
    }
    seenDevices.current = names;
  }, [outputDevices, profiles, loadProfile]);
}
