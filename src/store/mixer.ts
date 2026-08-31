import { create } from "zustand";
import { clearPublishedLevels } from "../lib/liveMeters";
import { invoke } from "@tauri-apps/api/core";
import type {
  AppStream,
  BusDef,
  EqConfig,
  MicClient,
  MicConfig,
  MeterMode,
  OutputDevice,
  ProfileInfo,
  SeenApp,
  AppIdentity,
  VirtualSink,
} from "../types";

// Faders fire on every pointer move; debounce backend calls per target so a
// drag doesn't spawn a pactl subprocess per pixel. UI state updates
// optimistically and immediately.
interface PendingInvoke {
  timer: number;
  run: () => Promise<void>;
}

const pendingInvokes = new Map<string, PendingInvoke>();
function debouncedInvoke(key: string, cmd: string, args: Record<string, unknown>, onError: (e: unknown) => void) {
  const existing = pendingInvokes.get(key);
  if (existing) clearTimeout(existing.timer);
  const run = async () => {
    try {
      await invoke(cmd, args);
    } catch (error) {
      onError(error);
    }
  };
  const pending: PendingInvoke = {
    timer: window.setTimeout(() => {
      if (pendingInvokes.get(key) === pending) pendingInvokes.delete(key);
      void run();
    }, 90),
    run,
  };
  pendingInvokes.set(key, pending);
}

async function flushPendingInvokes() {
  const pending = [...pendingInvokes.values()];
  pendingInvokes.clear();
  for (const entry of pending) clearTimeout(entry.timer);
  await Promise.all(pending.map((entry) => entry.run()));
}

function cancelPendingInvokes() {
  for (const entry of pendingInvokes.values()) clearTimeout(entry.timer);
  pendingInvokes.clear();
}

let profileRefreshVersion = 0;
let balanceChannelsVersion = 0;
let balanceVisibilityVersion = 0;
let balanceChannelsQueue = Promise.resolve();
let balanceVisibilityQueue = Promise.resolve();
let pendingBalanceChannels = 0;
let pendingBalanceVisibility = 0;
let confirmedBalanceChannels: { balanceA: string | null; balanceB: string | null } | null = null;
let confirmedBalanceVisibility: boolean | null = null;

interface ProfileSnapshot {
  activeProfile: string | null;
  channels: VirtualSink[];
  appStreams: AppStream[];
  outputDevices: OutputDevice[];
  channelOutputs: Record<string, string | null>;
  resolvedOutputs: Record<string, string | null>;
  channelFailover: Record<string, boolean>;
  eqConfigs: Record<string, EqConfig>;
  micConfig: MicConfig | null;
  micConfigs: MicConfig[];
  inputDevices: OutputDevice[];
  selectedMicNode: string;
  micClients: MicClient[];
  seenApps: SeenApp[];
  profiles: ProfileInfo[];
  buses: BusDef[];
}

interface StartupPrefs {
  onboarded: boolean;
  balance_a: string | null;
  balance_b: string | null;
  show_balance: boolean;
  multiple_mics: boolean;
  meter_mode: MeterMode;
}

async function readProfileSnapshot(selectedMicNode: string): Promise<ProfileSnapshot> {
  const snapshot = await invoke<Omit<ProfileSnapshot, "micConfig" | "selectedMicNode">>(
    "get_profile_snapshot",
  );
  const { micConfigs } = snapshot;
  const micConfig = micConfigs.find((mic) => mic.node_name === "sink_mic")
    ?? micConfigs[0]
    ?? null;
  return {
    ...snapshot,
    micConfig,
    selectedMicNode: micConfigs.some((mic) => mic.node_name === selectedMicNode)
      ? selectedMicNode
      : "sink_mic",
  };
}

interface MixerStore {
  channels: VirtualSink[];
  appStreams: AppStream[];
  /** Visual-only live meter policy. */
  meterMode: MeterMode;
  setMeterMode: (mode: MeterMode) => Promise<void>;
  /** Physical output devices. */
  outputDevices: OutputDevice[];
  /** Channel -> chosen output node name (null = follow system default). */
  channelOutputs: Record<string, string | null>;
  /**
   * Channel -> the device node name it is actually routed to right now (after
   * default/fallback resolution). Lets a follow-default strip show where its
   * audio really goes, and reflects failover. Empty on the pactl fallback.
   */
  resolvedOutputs: Record<string, string | null>;
  /**
   * Channel -> whether it fails over to another device when its chosen device
   * (or the default) is gone. Off = play only on the chosen device / exact
   * default, silence otherwise. Defaults to on (absent treated as true).
   */
  channelFailover: Record<string, boolean>;
  fetchOutputs: () => Promise<void>;
  setChannelOutput: (sinkName: string, outputName: string | null) => Promise<void>;
  setChannelFailover: (sinkName: string, enabled: boolean) => Promise<void>;
  /** Use the same output device on all channels. */
  setAllOutputs: (outputName: string | null) => Promise<void>;
  /** Channel -> parametric EQ (absent = never configured, i.e. default). */
  eqConfigs: Record<string, EqConfig>;
  fetchEq: () => Promise<void>;
  setChannelEq: (sinkName: string, config: EqConfig) => Promise<void>;
  /** Mic chain (Phase 3). Null until loaded. */
  micConfig: MicConfig | null;
  micConfigs: MicConfig[];
  selectedMicNode: string;
  multipleMics: boolean;
  micCreationOpen: boolean;
  micClients: MicClient[];
  inputDevices: OutputDevice[];
  fetchMic: () => Promise<void>;
  fetchMicClients: () => Promise<void>;
  setMicConfig: (patch: Partial<MicConfig>) => Promise<void>;
  setMicChannelConfig: (nodeName: string, patch: Partial<MicConfig>) => Promise<void>;
  addMicChannel: (label: string, inputDevice: string | null, copyFrom: string | null) => Promise<void>;
  removeMicChannel: (nodeName: string) => Promise<void>;
  moveMicChannel: (from: string, to: string) => void;
  commitMicChannelOrder: () => Promise<void>;
  selectMic: (nodeName: string) => void;
  setMultipleMics: (enabled: boolean) => Promise<void>;
  setMicCreationOpen: (open: boolean) => void;
  profiles: ProfileInfo[];
  /** Bind/clear an output device that auto-loads a profile (Phase 5). */
  setProfileTrigger: (name: string, device: string | null) => Promise<void>;
  /** Create a clean-slate profile (saved, not applied). */
  createBlankProfile: (name: string, micEnabled: boolean) => Promise<boolean>;
  /** Copy an existing profile's audio setup into a new profile. */
  copyProfile: (sourceName: string, name: string) => Promise<boolean>;
  renameProfile: (name: string, newName: string) => Promise<boolean>;
  /** A profile was switched outside the UI (tray) - sync everything. */
  onProfileChanged: (name: string) => Promise<void>;
  /** App history (live + gone + ignored). */
  seenApps: SeenApp[];
  fetchSeenApps: () => Promise<void>;
  setAppIgnored: (app: { match_prop: string; match_value: string }, ignored: boolean) => Promise<void>;
  setAppGroupIgnored: (identities: AppIdentity[], ignored: boolean) => Promise<void>;
  forgetApp: (app: { match_prop: string; match_value: string }) => Promise<void>;
  forgetAppGroup: (identities: AppIdentity[]) => Promise<void>;
  /** Pre-route an app that isn't currently running (null clears). */
  setAppAssignment: (
    app: { match_prop: string; match_value: string },
    sinkName: string | null,
  ) => Promise<void>;
  setAppGroupAssignment: (identities: AppIdentity[], sinkName: string | null) => Promise<void>;
  /** Channel management: labels are free-form, sink names are stable. */
  addChannel: (label: string, icon: string | null, spatial?: boolean) => Promise<void>;
  renameChannel: (sinkName: string, label: string) => Promise<void>;
  removeChannel: (sinkName: string) => Promise<void>;
  /** Visual-only reorder while dragging a strip. */
  moveChannel: (from: string, to: string) => void;
  /** Persist the current strip order (called on drag end). */
  commitChannelOrder: () => Promise<void>;
  setChannelIcon: (sinkName: string, icon: string) => Promise<void>;
  /** User-defined mixes (record buses). */
  buses: BusDef[];
  fetchBuses: () => Promise<void>;
  addBus: (label: string) => Promise<void>;
  renameBus: (name: string, label: string) => Promise<void>;
  removeBus: (name: string) => Promise<void>;
  setBusMembers: (name: string, channels: string[]) => Promise<void>;
  /** Manual vs auto-include mode (carried set preserved). */
  setBusExclude: (name: string, exclude: boolean) => Promise<void>;
  /** A mix's playback level for recorders (0-150%); persisted. */
  setBusVolume: (name: string, volume: number) => Promise<void>;
  /** Mute a mix for recorders; persisted. */
  setBusMute: (name: string, muted: boolean) => Promise<void>;
  /** Session-scoped "listen on default output" toggles per node. */
  monitors: Record<string, boolean>;
  toggleMonitor: (name: string) => Promise<void>;
  /** Name of the most recently saved/loaded profile this session. */
  activeProfile: string | null;
  /** Fatal error surfaced to the UI (e.g. pactl missing, PipeWire down). */
  error: string | null;
  /** Dismiss the error banner. */
  clearError: () => void;
  initialized: boolean;
  initializing: boolean;
  /** Initial configuration snapshot completed. Retried by the visible-window
   * slow poll after a transient startup IPC failure. */
  startupSynchronized: boolean;
  startupSynchronizing: boolean;
  startupSyncError: string | null;
  /** True on the native PipeWire backend; false on the pactl fallback
   * (mixes/mic/monitoring unavailable). Null until known. */
  backendNative: boolean | null;
  /** First-run tutorial visible. */
  showOnboarding: boolean;
  /** True when the tutorial was reopened from Settings (no setup choice). */
  onboardingReplay: boolean;
  /** Close the tutorial; blank = collapse to a single starter channel. */
  finishOnboarding: (blank: boolean) => Promise<void>;
  /** Reopen the tutorial (view-only - no starting-point choice). */
  replayOnboarding: () => void;
  /** Balance slider channel picks (null = auto Game/Chat or first two). */
  balanceA: string | null;
  balanceB: string | null;
  setBalanceChannels: (a: string | null, b: string | null) => Promise<void>;
  showBalance: boolean;
  setBalanceVisible: (visible: boolean) => Promise<void>;

  /** Create the virtual sinks and load initial state. */
  initialize: () => Promise<void>;
  synchronizeStartupState: () => Promise<void>;
  fetchChannels: () => Promise<void>;
  fetchAppStreams: () => Promise<void>;
  setChannelVolume: (sinkName: string, volume: number) => Promise<void>;
  toggleMute: (sinkName: string, muted: boolean) => Promise<void>;
  routeApp: (streamIndex: number, sinkName: string) => Promise<void>;
  routeAppGroup: (
    streamIndices: number[],
    identities: AppIdentity[],
    desktopId: string | null,
    sinkName: string,
  ) => Promise<void>;
  setAppVolume: (streamIndex: number, volume: number) => Promise<void>;
  fetchProfiles: () => Promise<void>;
  loadProfile: (name: string) => Promise<boolean>;
  deleteProfile: (name: string) => Promise<boolean>;
  /** Set or clear (empty string) a persistent display name for an app. */
  renameApp: (stream: AppStream, alias: string) => Promise<void>;
}

/** Structural equality via JSON, to skip no-op store writes on each poll and
 *  avoid re-rendering the whole board when nothing changed (TD-029). */
const jsonEqual = (a: unknown, b: unknown): boolean =>
  JSON.stringify(a) === JSON.stringify(b);

export const useMixerStore = create<MixerStore>((set, get) => ({
  channels: [],
  appStreams: [],
  meterMode: "fps_60",
  setMeterMode: async (mode) => {
    const previous = get().meterMode;
    if (mode === "off") clearPublishedLevels();
    set({ meterMode: mode });
    try {
      await invoke("set_meter_mode", { mode });
    } catch (e) {
      set({ meterMode: previous, error: String(e) });
    }
  },
  outputDevices: [],
  channelOutputs: {},
  resolvedOutputs: {},
  channelFailover: {},
  micConfig: null,
  micClients: [],
  inputDevices: [],
  seenApps: [],
  profiles: [],
  activeProfile: null,
  error: null,
  clearError: () => set({ error: null }),
  initialized: false,
  initializing: false,
  startupSynchronized: false,
  startupSynchronizing: false,
  startupSyncError: null,
  backendNative: null,
  showOnboarding: false,
  onboardingReplay: false,

  replayOnboarding: () => set({ showOnboarding: true, onboardingReplay: true }),

  balanceA: null,
  balanceB: null,
  showBalance: true,

  setBalanceChannels: async (a, b) => {
    const version = ++balanceChannelsVersion;
    if (pendingBalanceChannels === 0) {
      confirmedBalanceChannels = { balanceA: get().balanceA, balanceB: get().balanceB };
    }
    pendingBalanceChannels += 1;
    set({ balanceA: a, balanceB: b });
    const write = balanceChannelsQueue.then(() => invoke("set_balance_channels", { a, b }));
    balanceChannelsQueue = write.then(() => undefined, () => undefined);
    try {
      await write;
      confirmedBalanceChannels = { balanceA: a, balanceB: b };
    } catch (e) {
      if (version === balanceChannelsVersion) {
        set({ ...confirmedBalanceChannels!, error: String(e) });
      }
    } finally {
      pendingBalanceChannels -= 1;
      if (pendingBalanceChannels === 0) confirmedBalanceChannels = null;
    }
  },

  setBalanceVisible: async (visible) => {
    const version = ++balanceVisibilityVersion;
    if (pendingBalanceVisibility === 0) confirmedBalanceVisibility = get().showBalance;
    pendingBalanceVisibility += 1;
    set({ showBalance: visible });
    const write = balanceVisibilityQueue.then(() => invoke("set_balance_visible", { visible }));
    balanceVisibilityQueue = write.then(() => undefined, () => undefined);
    try {
      await write;
      confirmedBalanceVisibility = visible;
    } catch (e) {
      if (version === balanceVisibilityVersion) {
        set({ showBalance: confirmedBalanceVisibility!, error: String(e) });
      }
    } finally {
      pendingBalanceVisibility -= 1;
      if (pendingBalanceVisibility === 0) confirmedBalanceVisibility = null;
    }
  },

  finishOnboarding: async (blank) => {
    const replay = get().onboardingReplay;
    set({ showOnboarding: false, onboardingReplay: false });
    if (replay) return; // view-only: nothing to persist or change
    try {
      await invoke("set_onboarded");
      if (blank) {
        // Collapse the seeded defaults to a single starter channel; the
        // active profile autosaves the result.
        const channels = get().channels;
        for (const c of channels.slice(1)) {
          await get().removeChannel(c.name);
        }
        if (channels.length > 0) {
          await get().renameChannel(channels[0].name, "Main");
          await get().setChannelIcon(channels[0].name, "graphic_eq");
        }
      }
    } catch (e) {
      set({ error: String(e) });
    }
  },

  initialize: async () => {
    if (get().initialized || get().initializing) return;
    set({ initializing: true });
    try {
      await invoke("init_virtual_devices");
      set({ initialized: true, initializing: false, error: null });
      await get().synchronizeStartupState();
    } catch (e) {
      set({ initializing: false, error: String(e) });
    }
  },

  synchronizeStartupState: async () => {
    if (!get().initialized || get().startupSynchronized || get().startupSynchronizing) return;
    const refreshVersion = profileRefreshVersion;
    set({ startupSynchronizing: true });
    try {
      const results = await Promise.allSettled([
        readProfileSnapshot(get().selectedMicNode),
        invoke<{ native: boolean }>("get_backend_info"),
        invoke<StartupPrefs>("get_prefs"),
      ]);
      const failed = results.find((result) => result.status === "rejected");
      if (failed?.status === "rejected") throw failed.reason;
      const [snapshot, backendInfo, prefs] = results.map((result) => {
        if (result.status !== "fulfilled") throw result.reason;
        return result.value;
      }) as [ProfileSnapshot, { native: boolean }, StartupPrefs];
      if (refreshVersion !== profileRefreshVersion) {
        set({ startupSynchronizing: false });
        return;
      }
      const current = get();
      if (prefs.meter_mode === "off") clearPublishedLevels();
      set({
        ...snapshot,
        activeProfile: snapshot.activeProfile
          ?? (snapshot.profiles.some((profile) => profile.name === "Default") ? "Default" : null),
        backendNative: backendInfo.native,
        balanceA: prefs.balance_a,
        balanceB: prefs.balance_b,
        showBalance: prefs.show_balance,
        multipleMics: prefs.multiple_mics,
        meterMode: prefs.meter_mode,
        showOnboarding: current.onboardingReplay ? current.showOnboarding : !prefs.onboarded,
        startupSynchronized: true,
        startupSynchronizing: false,
        startupSyncError: null,
        error: current.startupSyncError !== null && current.error === current.startupSyncError
          ? null
          : current.error,
      });
    } catch (e) {
      if (refreshVersion !== profileRefreshVersion) {
        set({ startupSynchronizing: false });
        return;
      }
      const startupSyncError = String(e);
      set({
        startupSynchronized: false,
        startupSynchronizing: false,
        startupSyncError,
        error: startupSyncError,
      });
    }
  },

  fetchChannels: async () => {
    const refreshVersion = profileRefreshVersion;
    try {
      const channels = await invoke<VirtualSink[]>("get_virtual_devices");
      if (refreshVersion !== profileRefreshVersion) return;
      set({ channels });
    } catch (e) {
      if (refreshVersion === profileRefreshVersion) set({ error: String(e) });
    }
  },

  fetchAppStreams: async () => {
    const refreshVersion = profileRefreshVersion;
    try {
      const appStreams = await invoke<AppStream[]>("get_app_streams");
      if (refreshVersion !== profileRefreshVersion) return;
      const s = get();
      const patch: Partial<MixerStore> = {};
      if (!jsonEqual(s.appStreams, appStreams)) patch.appStreams = appStreams;
      if (Object.keys(patch).length) set(patch);
    } catch (e) {
      if (refreshVersion === profileRefreshVersion) set({ error: String(e) });
    }
  },

  setChannelVolume: async (sinkName, volume) => {
    set((s) => ({
      channels: s.channels.map((c) =>
        c.name === sinkName ? { ...c, volume_percent: volume } : c,
      ),
    }));
    debouncedInvoke(
      `chvol:${sinkName}`,
      "set_channel_volume",
      { sinkName, volume, expectedProfile: get().activeProfile },
      (e) => {
        set({ error: String(e) });
        void get().fetchChannels();
      },
    );
  },

  toggleMute: async (sinkName, muted) => {
    set((s) => ({
      channels: s.channels.map((c) =>
        c.name === sinkName ? { ...c, muted } : c,
      ),
    }));
    try {
      await invoke("toggle_channel_mute", { sinkName, muted, expectedProfile: get().activeProfile });
    } catch (e) {
      set({ error: String(e) });
      await get().fetchChannels();
    }
  },

  routeApp: async (streamIndex, sinkName) => {
    set((s) => ({
      appStreams: s.appStreams.map((a) =>
        a.index === streamIndex
          ? { ...a, assigned_sink: sinkName === "" ? null : sinkName }
          : a,
      ),
    }));
    try {
      await invoke("route_app_to_channel", {
        streamIndex,
        sinkName,
        expectedProfile: get().activeProfile,
      });
    } catch (e) {
      set({ error: String(e) });
    } finally {
      await get().fetchAppStreams();
    }
  },

  routeAppGroup: async (streamIndices, identities, desktopId, sinkName) => {
    const indexSet = new Set(streamIndices);
    set((state) => ({
      appStreams: state.appStreams.map((app) =>
        indexSet.has(app.index)
          ? { ...app, assigned_sink: sinkName === "" ? null : sinkName }
          : app,
      ),
    }));
    try {
      await invoke("route_app_group_to_channel", {
        streamIndices,
        identities,
        desktopId,
        sinkName,
        expectedProfile: get().activeProfile,
      });
    } catch (e) {
      set({ error: String(e) });
    } finally {
      await Promise.all([get().fetchAppStreams(), get().fetchSeenApps()]);
    }
  },

  setAppVolume: async (streamIndex, volume) => {
    set((s) => ({
      appStreams: s.appStreams.map((a) =>
        a.index === streamIndex ? { ...a, volume_percent: volume } : a,
      ),
    }));
    debouncedInvoke(
      `appvol:${streamIndex}`,
      "set_app_volume",
      { streamIndex, volume },
      (e) => set({ error: String(e) }),
    );
  },

  fetchOutputs: async () => {
    const refreshVersion = profileRefreshVersion;
    try {
      const [outputDevices, channelOutputs, resolvedOutputs, channelFailover] = await Promise.all([
        invoke<OutputDevice[]>("get_output_devices"),
        invoke<Record<string, string | null>>("get_channel_outputs"),
        invoke<Record<string, string | null>>("get_resolved_outputs"),
        invoke<Record<string, boolean>>("get_channel_failover"),
      ]);
      if (refreshVersion !== profileRefreshVersion) return;
      const s = get();
      const patch: Partial<MixerStore> = {};
      if (!jsonEqual(s.outputDevices, outputDevices)) patch.outputDevices = outputDevices;
      if (!jsonEqual(s.channelOutputs, channelOutputs)) patch.channelOutputs = channelOutputs;
      if (!jsonEqual(s.resolvedOutputs, resolvedOutputs)) patch.resolvedOutputs = resolvedOutputs;
      if (!jsonEqual(s.channelFailover, channelFailover)) patch.channelFailover = channelFailover;
      if (Object.keys(patch).length) set(patch);
    } catch (e) {
      if (refreshVersion === profileRefreshVersion) set({ error: String(e) });
    }
  },

  setChannelOutput: async (sinkName, outputName) => {
    set((s) => ({
      channelOutputs: { ...s.channelOutputs, [sinkName]: outputName },
    }));
    try {
      await invoke("set_channel_output", {
        sinkName,
        outputName: outputName ?? "",
        expectedProfile: get().activeProfile,
      });
    } catch (e) {
      set({ error: String(e) });
      await get().fetchOutputs();
    }
  },

  setChannelFailover: async (sinkName, enabled) => {
    set((s) => ({
      channelFailover: { ...s.channelFailover, [sinkName]: enabled },
    }));
    try {
      await invoke("set_channel_failover", {
        sinkName,
        enabled,
        expectedProfile: get().activeProfile,
      });
    } catch (e) {
      set({ error: String(e) });
      await get().fetchOutputs();
    }
  },

  setAllOutputs: async (outputName) => {
    for (const channel of get().channels) {
      await get().setChannelOutput(channel.name, outputName);
    }
  },

  eqConfigs: {},

  fetchEq: async () => {
    const refreshVersion = profileRefreshVersion;
    try {
      const eqConfigs = await invoke<Record<string, EqConfig>>("get_channel_eq_configs");
      if (refreshVersion !== profileRefreshVersion) return;
      if (!jsonEqual(get().eqConfigs, eqConfigs)) set({ eqConfigs });
    } catch (e) {
      if (refreshVersion === profileRefreshVersion) set({ error: String(e) });
    }
  },

  setChannelEq: async (sinkName, config) => {
    set({ eqConfigs: { ...get().eqConfigs, [sinkName]: config } });
    // Debounced per channel: a band drag settles into one apply, and two
    // open EQ panels never clobber each other's pending call.
    debouncedInvoke(`eq:${sinkName}`, "set_channel_eq", {
      sinkName,
      config,
      expectedProfile: get().activeProfile,
    }, (e) => {
      set({ error: String(e) });
      void get().fetchEq();
    });
  },

  micConfigs: [],
  selectedMicNode: "sink_mic",
  multipleMics: false,
  micCreationOpen: false,

  fetchMic: async () => {
    const refreshVersion = profileRefreshVersion;
    try {
      const [micConfigs, inputDevices] = await Promise.all([
        invoke<MicConfig[]>("get_mic_configs"),
        invoke<OutputDevice[]>("get_input_devices"),
      ]);
      if (refreshVersion !== profileRefreshVersion) return;
      const micConfig = micConfigs.find((mic) => mic.node_name === "sink_mic") ?? micConfigs[0] ?? null;
      const selectedMicNode = micConfigs.some((mic) => mic.node_name === get().selectedMicNode)
        ? get().selectedMicNode
        : "sink_mic";
      set({ micConfig, micConfigs, inputDevices, selectedMicNode });
    } catch (e) {
      if (refreshVersion === profileRefreshVersion) set({ error: String(e) });
    }
  },

  fetchMicClients: async () => {
    const refreshVersion = profileRefreshVersion;
    try {
      const micClients = await invoke<MicClient[]>("get_mic_clients");
      if (refreshVersion !== profileRefreshVersion) return;
      if (!jsonEqual(get().micClients, micClients)) set({ micClients });
    } catch (e) {
      if (refreshVersion === profileRefreshVersion) set({ error: String(e) });
    }
  },

  setMicConfig: async (patch) => {
    await get().setMicChannelConfig("sink_mic", patch);
  },

  setMicChannelConfig: async (nodeName, patch) => {
    const current = get().micConfigs.find((mic) => mic.node_name === nodeName)
      ?? (nodeName === "sink_mic" ? get().micConfig : null);
    if (!current) return;
    const config = { ...current, ...patch };
    const micConfigs = get().micConfigs.map((mic) => mic.node_name === nodeName ? config : mic);
    set({ micConfigs, ...(nodeName === "sink_mic" ? { micConfig: config } : {}) });
    // Debounced: slider drags and rename typing settle into one apply.
    debouncedInvoke(`micConfig:${nodeName}`, "set_mic_config", {
      config,
      expectedProfile: get().activeProfile,
    }, (e) => {
      set({ error: String(e) });
      void get().fetchMic();
    });
  },

  addMicChannel: async (label, inputDevice, copyFrom) => {
    try {
      const config = await invoke<MicConfig>("add_mic_channel", {
        label,
        inputDevice,
        copyFrom,
        expectedProfile: get().activeProfile,
      });
      set({ micConfigs: [...get().micConfigs, config], selectedMicNode: config.node_name });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  removeMicChannel: async (nodeName) => {
    try {
      await invoke("remove_mic_channel", { nodeName, expectedProfile: get().activeProfile });
      set({
        micConfigs: get().micConfigs.filter((mic) => mic.node_name !== nodeName),
        selectedMicNode: get().selectedMicNode === nodeName ? "sink_mic" : get().selectedMicNode,
      });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  moveMicChannel: (from, to) => {
    if (from === "sink_mic" || from === to) return;
    const configs = [...get().micConfigs];
    const fromIndex = configs.findIndex((mic) => mic.node_name === from);
    if (fromIndex < 1) return;
    const [moving] = configs.splice(fromIndex, 1);
    const targetIndex = to === "sink_mic"
      ? 1
      : configs.findIndex((mic) => mic.node_name === to);
    configs.splice(targetIndex < 1 ? configs.length : targetIndex, 0, moving);
    set({ micConfigs: configs });
  },

  commitMicChannelOrder: async () => {
    try {
      await invoke("reorder_mic_channels", {
        order: get().micConfigs.slice(1).map((mic) => mic.node_name),
        expectedProfile: get().activeProfile,
      });
    } catch (e) {
      set({ error: String(e) });
      await get().fetchMic();
    }
  },

  selectMic: (nodeName) => set({ selectedMicNode: nodeName }),

  setMultipleMics: async (enabled) => {
    const previous = {
      multipleMics: get().multipleMics,
      selectedMicNode: get().selectedMicNode,
      micCreationOpen: get().micCreationOpen,
    };
    set({
      multipleMics: enabled,
      ...(!enabled ? { selectedMicNode: "sink_mic", micCreationOpen: false } : {}),
    });
    try {
      await invoke("set_multiple_mics", { enabled });
    } catch (e) {
      set({ ...previous, error: String(e) });
    }
  },

  setMicCreationOpen: (open) => set({ micCreationOpen: open }),

  fetchProfiles: async () => {
    const refreshVersion = profileRefreshVersion;
    try {
      const profiles = await invoke<ProfileInfo[]>("list_profiles");
      if (refreshVersion !== profileRefreshVersion) return;
      set({ profiles });
    } catch (e) {
      if (refreshVersion === profileRefreshVersion) set({ error: String(e) });
    }
  },

  setProfileTrigger: async (name, device) => {
    try {
      await invoke("set_profile_trigger", { name, device: device ?? "" });
      await get().fetchProfiles();
    } catch (e) {
      set({ error: String(e) });
    }
  },

  onProfileChanged: async (_name) => {
    // The backend has already switched (tray/application automation). Never
    // let an edit queued for the previous profile land in the new one.
    cancelPendingInvokes();
    const version = ++profileRefreshVersion;
    try {
      const snapshot = await readProfileSnapshot(get().selectedMicNode);
      if (version !== profileRefreshVersion) return;
      set(snapshot);
    } catch (e) {
      if (version === profileRefreshVersion) set({ error: String(e) });
    }
  },

  createBlankProfile: async (name, micEnabled) => {
    try {
      await invoke("create_blank_profile", { name, micEnabled });
    } catch (e) {
      set({ error: String(e) });
      return false;
    }
    await get().fetchProfiles();
    // The profile now exists even if switching to it fails. Report the
    // synchronization error, but let lifecycle UI close instead of offering
    // a retry that can only fail with "already exists".
    await get().loadProfile(name);
    return true;
  },

  copyProfile: async (sourceName, name) => {
    try {
      await invoke("copy_profile", { sourceName, name });
    } catch (e) {
      set({ error: String(e) });
      return false;
    }
    await get().fetchProfiles();
    await get().loadProfile(name);
    return true;
  },

  renameProfile: async (name, newName) => {
    try {
      await invoke("rename_profile", { name, newName });
    } catch (e) {
      set({ error: String(e) });
      return false;
    }
    if (get().activeProfile === name) set({ activeProfile: newName });
    await get().fetchProfiles();
    return true;
  },

  fetchSeenApps: async () => {
    const refreshVersion = profileRefreshVersion;
    try {
      const seenApps = await invoke<SeenApp[]>("get_seen_apps");
      if (refreshVersion !== profileRefreshVersion) return;
      if (!jsonEqual(get().seenApps, seenApps)) set({ seenApps });
    } catch (e) {
      if (refreshVersion === profileRefreshVersion) set({ error: String(e) });
    }
  },

  setAppIgnored: async (app, ignored) => {
    try {
      await invoke("set_app_ignored", {
        matchProp: app.match_prop,
        matchValue: app.match_value,
        ignored,
      });
      await Promise.all([get().fetchSeenApps(), get().fetchAppStreams()]);
    } catch (e) {
      set({ error: String(e) });
    }
  },

  setAppGroupIgnored: async (identities, ignored) => {
    try {
      await invoke("set_app_group_ignored", { identities, ignored });
      await Promise.all([get().fetchSeenApps(), get().fetchAppStreams()]);
    } catch (e) {
      set({ error: String(e) });
    }
  },

  forgetApp: async (app) => {
    try {
      await invoke("forget_app", {
        matchProp: app.match_prop,
        matchValue: app.match_value,
        expectedProfile: get().activeProfile,
      });
      await get().fetchSeenApps();
    } catch (e) {
      set({ error: String(e) });
    }
  },

  forgetAppGroup: async (identities) => {
    try {
      await invoke("forget_app_group", {
        identities,
        expectedProfile: get().activeProfile,
      });
      await get().fetchSeenApps();
    } catch (e) {
      set({ error: String(e) });
    }
  },

  setAppAssignment: async (app, sinkName) => {
    try {
      await invoke("set_app_assignment", {
        matchProp: app.match_prop,
        matchValue: app.match_value,
        sinkName: sinkName ?? "",
        expectedProfile: get().activeProfile,
      });
      await get().fetchSeenApps();
    } catch (e) {
      set({ error: String(e) });
    }
  },

  setAppGroupAssignment: async (identities, sinkName) => {
    try {
      await invoke("set_app_group_assignment", {
        identities,
        sinkName: sinkName ?? "",
        expectedProfile: get().activeProfile,
      });
      await get().fetchSeenApps();
    } catch (e) {
      set({ error: String(e) });
    }
  },

  loadProfile: async (name) => {
    ++profileRefreshVersion; // invalidate pre-mutation polls and snapshots
    try {
      // Manual switches preserve the final value of an in-progress drag in
      // the profile being left, then move to the requested profile.
      await flushPendingInvokes();
      await invoke("load_profile", { name });
      // Loading succeeded. Invalidate refreshes that began before or during
      // the switch, then read the final profile snapshot.
      const version = ++profileRefreshVersion;
      const snapshot = await readProfileSnapshot(get().selectedMicNode);
      if (version === profileRefreshVersion) set(snapshot);
      return true;
    } catch (e) {
      set({ error: String(e) });
      return false;
    }
  },

  deleteProfile: async (name) => {
    try {
      await invoke("delete_profile", { name });
    } catch (e) {
      set({ error: String(e) });
      return false;
    }
    try {
      const active = await invoke<string | null>("get_active_profile");
      if (active && active !== get().activeProfile) await get().onProfileChanged(active);
      else await get().fetchProfiles();
    } catch (e) {
      set({ error: String(e) });
    }
    return true;
  },

  addChannel: async (label, icon, spatial = false) => {
    try {
      await invoke("add_channel", { label, icon, spatial, expectedProfile: get().activeProfile });
      // Buses too: the master (and auto-include mixes) absorb the channel.
      await Promise.all([get().fetchChannels(), get().fetchOutputs(), get().fetchBuses()]);
    } catch (e) {
      set({ error: String(e) });
    }
  },

  buses: [],

  fetchBuses: async () => {
    const refreshVersion = profileRefreshVersion;
    try {
      const buses = await invoke<BusDef[]>("list_buses");
      if (refreshVersion !== profileRefreshVersion) return;
      set({ buses });
    } catch (e) {
      if (refreshVersion === profileRefreshVersion) set({ error: String(e) });
    }
  },

  addBus: async (label) => {
    try {
      await invoke("add_bus", { label, expectedProfile: get().activeProfile });
      await get().fetchBuses();
    } catch (e) {
      set({ error: String(e) });
    }
  },

  renameBus: async (name, label) => {
    set((s) => ({
      buses: s.buses.map((b) => (b.name === name ? { ...b, label } : b)),
    }));
    try {
      await invoke("rename_bus", { name, label, expectedProfile: get().activeProfile });
    } catch (e) {
      set({ error: String(e) });
      await get().fetchBuses();
    }
  },

  removeBus: async (name) => {
    try {
      await invoke("remove_bus", { name, expectedProfile: get().activeProfile });
      await get().fetchBuses();
    } catch (e) {
      set({ error: String(e) });
    }
  },

  setBusMembers: async (name, channels) => {
    // `channels` is the carried set; auto-include mixes store the
    // complement (mirrors the backend's conversion).
    const all = get().channels.map((c) => c.name);
    set((s) => ({
      buses: s.buses.map((b) =>
        b.name === name
          ? { ...b, channels: b.exclude ? all.filter((c) => !channels.includes(c)) : channels }
          : b,
      ),
    }));
    try {
      await invoke("set_bus_members", {
        name,
        channels,
        expectedProfile: get().activeProfile,
      });
      // The backend converts against its own channel set - sync up so the
      // stored complement can't drift if channels changed mid-flight.
      await get().fetchBuses();
    } catch (e) {
      set({ error: String(e) });
      await get().fetchBuses();
    }
  },

  setBusExclude: async (name, exclude) => {
    const all = get().channels.map((c) => c.name);
    set((s) => ({
      buses: s.buses.map((b) => {
        if (b.name !== name || b.exclude === exclude) return b;
        // Preserve the carried set; only the stored representation flips.
        const carried = b.exclude
          ? all.filter((c) => !b.channels.includes(c))
          : b.channels;
        return {
          ...b,
          exclude,
          channels: exclude ? all.filter((c) => !carried.includes(c)) : carried,
        };
      }),
    }));
    try {
      await invoke("set_bus_exclude", { name, exclude, expectedProfile: get().activeProfile });
    } catch (e) {
      set({ error: String(e) });
      await get().fetchBuses();
    }
  },

  setBusVolume: async (name, volume) => {
    set((s) => ({
      buses: s.buses.map((b) => (b.name === name ? { ...b, volume_percent: volume } : b)),
    }));
    debouncedInvoke(`busvol:${name}`, "set_bus_volume", {
      name,
      volume,
      expectedProfile: get().activeProfile,
    }, (e) => {
      set({ error: String(e) });
      void get().fetchBuses();
    });
  },

  setBusMute: async (name, muted) => {
    set((s) => ({
      buses: s.buses.map((b) => (b.name === name ? { ...b, muted } : b)),
    }));
    try {
      await invoke("set_bus_mute", { name, muted, expectedProfile: get().activeProfile });
    } catch (e) {
      set({ error: String(e) });
      await get().fetchBuses();
    }
  },

  monitors: {},

  toggleMonitor: async (name) => {
    const enabled = !get().monitors[name];
    set((s) => ({ monitors: { ...s.monitors, [name]: enabled } }));
    try {
      await invoke("set_monitor", { sinkName: name, enabled });
    } catch (e) {
      set({ error: String(e) });
      set((s) => ({ monitors: { ...s.monitors, [name]: !enabled } }));
    }
  },

  setChannelIcon: async (sinkName, icon) => {
    set((s) => ({
      channels: s.channels.map((c) => (c.name === sinkName ? { ...c, icon } : c)),
    }));
    try {
      await invoke("set_channel_icon", { sinkName, icon, expectedProfile: get().activeProfile });
    } catch (e) {
      set({ error: String(e) });
      await get().fetchChannels();
    }
  },

  renameChannel: async (sinkName, label) => {
    set((s) => ({
      channels: s.channels.map((c) => (c.name === sinkName ? { ...c, label } : c)),
    }));
    try {
      await invoke("rename_channel", { sinkName, label, expectedProfile: get().activeProfile });
    } catch (e) {
      set({ error: String(e) });
      await get().fetchChannels();
    }
  },

  // Visual-only move while dragging; commitChannelOrder persists on drop.
  moveChannel: (from, to) => {
    set((s) => {
      const arr = [...s.channels];
      const fi = arr.findIndex((c) => c.name === from);
      const ti = arr.findIndex((c) => c.name === to);
      if (fi < 0 || ti < 0 || fi === ti) return {};
      const [moved] = arr.splice(fi, 1);
      arr.splice(ti, 0, moved);
      return { channels: arr };
    });
  },

  commitChannelOrder: async () => {
    const order = get().channels.map((c) => c.name);
    try {
      await invoke("reorder_channels", { order, expectedProfile: get().activeProfile });
    } catch (e) {
      set({ error: String(e) });
      await get().fetchChannels();
    }
  },

  removeChannel: async (sinkName) => {
    try {
      await invoke("remove_channel", { sinkName, expectedProfile: get().activeProfile });
      await Promise.all([
        get().fetchChannels(),
        get().fetchAppStreams(),
        get().fetchOutputs(),
        get().fetchEq(), // the channel's EQ entry is gone too
        get().fetchBuses(), // memberships dropped the channel
      ]);
    } catch (e) {
      set({ error: String(e) });
    }
  },

  renameApp: async (stream, alias) => {
    const trimmed = alias.trim();
    set((s) => ({
      appStreams: s.appStreams.map((a) =>
        a.match_prop === stream.match_prop && a.match_value === stream.match_value
          ? { ...a, alias: trimmed === "" ? null : trimmed }
          : a,
      ),
    }));
    try {
      await invoke("rename_app", {
        matchProp: stream.match_prop,
        matchValue: stream.match_value,
        alias: trimmed,
      });
    } catch (e) {
      set({ error: String(e) });
      await get().fetchAppStreams();
    }
  },
}));
