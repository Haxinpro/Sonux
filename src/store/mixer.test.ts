import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// The store talks to the Rust backend through Tauri IPC; mock the boundary.
const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invoke(...args),
}));

import { useMixerStore } from "./mixer";
import type { VirtualSink } from "../types";
import { defaultEqConfig } from "../types";

const channel = (name: string, volume = 100): VirtualSink => ({
  name,
  label: name.replace("sink_", ""),
  icon: null,
  volume_percent: volume,
  muted: false,
  stream_mix: true,
});

const initialState = useMixerStore.getState();

beforeEach(() => {
  vi.useFakeTimers();
  invoke.mockReset();
  invoke.mockResolvedValue(undefined);
  useMixerStore.setState(initialState, true);
});

afterEach(() => {
  vi.runOnlyPendingTimers();
  vi.useRealTimers();
});

describe("setChannelVolume", () => {
  it("updates the UI immediately and debounces the backend call", async () => {
    useMixerStore.setState({ channels: [channel("sink_game")] });
    const store = useMixerStore.getState();

    await store.setChannelVolume("sink_game", 40);
    await store.setChannelVolume("sink_game", 55);

    // Optimistic: the strip moved on the second call already…
    expect(useMixerStore.getState().channels[0].volume_percent).toBe(55);
    // …but the backend hasn't been hit yet (drag in progress).
    expect(invoke).not.toHaveBeenCalled();

    vi.advanceTimersByTime(100);
    // Only the final value of the drag reaches the backend.
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("set_channel_volume", {
      sinkName: "sink_game",
      volume: 55,
      expectedProfile: null,
    });
  });

  it("keeps per-channel debounce keys separate", async () => {
    useMixerStore.setState({ channels: [channel("sink_game"), channel("sink_chat")] });
    const store = useMixerStore.getState();

    await store.setChannelVolume("sink_game", 10);
    await store.setChannelVolume("sink_chat", 20);
    vi.advanceTimersByTime(100);

    expect(invoke).toHaveBeenCalledTimes(2);
  });

  it("flushes the old profile's pending edit before a manual profile switch", async () => {
    useMixerStore.setState({
      channels: [channel("sink_game")],
      activeProfile: "Old",
    });
    await useMixerStore.getState().setChannelVolume("sink_game", 35);

    await useMixerStore.getState().loadProfile("New");

    expect(invoke.mock.calls[0]).toEqual([
      "set_channel_volume",
      { sinkName: "sink_game", volume: 35, expectedProfile: "Old" },
    ]);
    expect(invoke.mock.calls[1]).toEqual(["load_profile", { name: "New" }]);
  });
});

describe("profile operations", () => {
  const snapshot = (profile: string) => ({
    activeProfile: profile,
    channels: [channel(`sink_${profile.toLowerCase()}`)],
    appStreams: [], outputDevices: [], channelOutputs: {}, resolvedOutputs: {},
    channelFailover: {}, eqConfigs: {}, micConfigs: [], inputDevices: [],
    micClients: [], seenApps: [],
    profiles: [{ name: profile, trigger_device: null, protected: false }],
    buses: [],
  });

  it("commits only the latest rapid external profile refresh", async () => {
    let resolveOld!: (value: unknown) => void;
    invoke
      .mockImplementationOnce(() => new Promise((resolve) => { resolveOld = resolve; }))
      .mockResolvedValueOnce(snapshot("New"));

    const oldRefresh = useMixerStore.getState().onProfileChanged("Old");
    const newRefresh = useMixerStore.getState().onProfileChanged("New");
    await newRefresh;
    resolveOld(snapshot("Old"));
    await oldRefresh;

    expect(useMixerStore.getState().activeProfile).toBe("New");
    expect(useMixerStore.getState().channels[0].name).toBe("sink_new");
    expect(useMixerStore.getState().profiles[0].name).toBe("New");
  });

  it("keeps a backend-final manual load over an event started during its mutation", async () => {
    let resolvePoll!: (value: unknown) => void;
    let resolveLoad!: (value: unknown) => void;
    let resolveEvent!: (value: unknown) => void;
    let snapshotCalls = 0;
    invoke.mockImplementation((command: string) => {
      if (command === "get_virtual_devices") {
        return new Promise((resolve) => { resolvePoll = resolve; });
      }
      if (command === "load_profile") {
        return new Promise((resolve) => { resolveLoad = resolve; });
      }
      snapshotCalls += 1;
      if (snapshotCalls === 1) {
        return new Promise((resolve) => { resolveEvent = resolve; });
      }
      return Promise.resolve(snapshot("Manual"));
    });

    const oldPoll = useMixerStore.getState().fetchChannels();
    const manual = useMixerStore.getState().loadProfile("Manual");
    for (let index = 0; index < 4 && !resolveLoad; index += 1) {
      await Promise.resolve();
    }
    const event = useMixerStore.getState().onProfileChanged("Event");
    await Promise.resolve();
    resolveLoad(undefined);
    await manual;
    resolvePoll([channel("sink_poll")]);
    resolveEvent(snapshot("Event"));
    await Promise.all([oldPoll, event]);

    expect(useMixerStore.getState().activeProfile).toBe("Manual");
    expect(useMixerStore.getState().channels[0].name).toBe("sink_manual");
  });

  it("reports a failed load without changing the active profile", async () => {
    useMixerStore.setState({ activeProfile: "Old" });
    invoke.mockRejectedValueOnce("malformed profile New");

    const succeeded = await useMixerStore.getState().loadProfile("New");

    expect(succeeded).toBe(false);
    expect(useMixerStore.getState().activeProfile).toBe("Old");
    expect(useMixerStore.getState().error).toContain("malformed profile");
  });

  it("returns failure when rename is rejected", async () => {
    invoke.mockRejectedValueOnce("profile already exists");

    const succeeded = await useMixerStore.getState().renameProfile("Old", "New");

    expect(succeeded).toBe(false);
    expect(useMixerStore.getState().error).toContain("already exists");
  });

  it("reports creation committed when automatic activation fails", async () => {
    invoke
      .mockResolvedValueOnce(undefined)
      .mockResolvedValueOnce([])
      .mockRejectedValueOnce("could not activate new profile");

    const succeeded = await useMixerStore.getState().createBlankProfile("New", true);

    expect(succeeded).toBe(true);
    expect(useMixerStore.getState().error).toContain("could not activate");
  });

  it("reports copy committed when automatic activation fails", async () => {
    invoke
      .mockResolvedValueOnce(undefined)
      .mockResolvedValueOnce([])
      .mockRejectedValueOnce("could not activate copied profile");

    const succeeded = await useMixerStore.getState().copyProfile("Old", "Copy");

    expect(succeeded).toBe(true);
    expect(useMixerStore.getState().error).toContain("could not activate");
  });

  it("reports rename committed when profile refresh fails", async () => {
    invoke
      .mockResolvedValueOnce(undefined)
      .mockRejectedValueOnce("could not refresh profiles");

    const succeeded = await useMixerStore.getState().renameProfile("Old", "New");

    expect(succeeded).toBe(true);
    expect(useMixerStore.getState().error).toContain("could not refresh");
  });

  it("reports deletion committed when active-profile refresh fails", async () => {
    invoke
      .mockResolvedValueOnce(undefined)
      .mockRejectedValueOnce("could not refresh active profile");

    const succeeded = await useMixerStore.getState().deleteProfile("Old");

    expect(succeeded).toBe(true);
    expect(useMixerStore.getState().error).toContain("could not refresh");
  });
});

describe("balance preferences", () => {
  it("rolls back optimistic channel picks when persistence fails", async () => {
    useMixerStore.setState({ balanceA: "sink_game", balanceB: "sink_chat" });
    invoke.mockRejectedValueOnce("write failed");

    await useMixerStore.getState().setBalanceChannels("sink_media", "sink_chat");

    expect(useMixerStore.getState().balanceA).toBe("sink_game");
    expect(useMixerStore.getState().balanceB).toBe("sink_chat");
    expect(useMixerStore.getState().error).toContain("write failed");
  });

  it("rolls back optimistic visibility when persistence fails", async () => {
    useMixerStore.setState({ showBalance: true });
    invoke.mockRejectedValueOnce("write failed");

    await useMixerStore.getState().setBalanceVisible(false);

    expect(useMixerStore.getState().showBalance).toBe(true);
  });

  it("does not let an older identical failure undo a newer success", async () => {
    let rejectOlder!: (reason: unknown) => void;
    invoke
      .mockImplementationOnce(() => new Promise((_, reject) => { rejectOlder = reject; }))
      .mockResolvedValueOnce(undefined);
    useMixerStore.setState({ showBalance: true });

    const older = useMixerStore.getState().setBalanceVisible(false);
    const newer = useMixerStore.getState().setBalanceVisible(false);
    await Promise.resolve();
    expect(invoke).toHaveBeenCalledTimes(1);
    rejectOlder("older write failed");
    await Promise.all([older, newer]);

    expect(useMixerStore.getState().showBalance).toBe(false);
    expect(useMixerStore.getState().error).toBeNull();
  });

  it("serializes an exact visibility reversal in request order", async () => {
    let resolveFirst!: () => void;
    invoke
      .mockImplementationOnce(() => new Promise<void>((resolve) => { resolveFirst = resolve; }))
      .mockResolvedValueOnce(undefined);
    useMixerStore.setState({ showBalance: true });

    const hide = useMixerStore.getState().setBalanceVisible(false);
    const show = useMixerStore.getState().setBalanceVisible(true);
    await Promise.resolve();
    expect(invoke.mock.calls).toEqual([["set_balance_visible", { visible: false }]]);
    resolveFirst();
    await Promise.all([hide, show]);

    expect(invoke.mock.calls).toEqual([
      ["set_balance_visible", { visible: false }],
      ["set_balance_visible", { visible: true }],
    ]);
    expect(useMixerStore.getState().showBalance).toBe(true);
  });

  it("reconciles to the last confirmed value when every queued write fails", async () => {
    invoke.mockRejectedValue("write failed");
    useMixerStore.setState({ showBalance: true });

    const first = useMixerStore.getState().setBalanceVisible(false);
    const second = useMixerStore.getState().setBalanceVisible(true);
    await Promise.all([first, second]);

    expect(useMixerStore.getState().showBalance).toBe(true);
    expect(invoke.mock.calls.map(([command]) => command)).toEqual([
      "set_balance_visible",
      "set_balance_visible",
    ]);
  });

  it("restores confirmed channel picks when both serialized writes fail", async () => {
    invoke.mockRejectedValue("write failed");
    useMixerStore.setState({ balanceA: "sink_game", balanceB: "sink_chat" });

    const first = useMixerStore.getState().setBalanceChannels("sink_media", "sink_chat");
    const second = useMixerStore.getState().setBalanceChannels("sink_game", "sink_media");
    await Promise.all([first, second]);

    expect(useMixerStore.getState().balanceA).toBe("sink_game");
    expect(useMixerStore.getState().balanceB).toBe("sink_chat");
    expect(invoke.mock.calls).toEqual([
      ["set_balance_channels", { a: "sink_media", b: "sink_chat" }],
      ["set_balance_channels", { a: "sink_game", b: "sink_media" }],
    ]);
  });
});

describe("toggleMute", () => {
  it("binds an immediate edit to the profile visible when it was sent", async () => {
    useMixerStore.setState({
      channels: [channel("sink_game")],
      activeProfile: "Gaming",
    });

    await useMixerStore.getState().toggleMute("sink_game", true);

    expect(invoke).toHaveBeenCalledWith("toggle_channel_mute", {
      sinkName: "sink_game",
      muted: true,
      expectedProfile: "Gaming",
    });
  });
});

describe("fetchAppStreams", () => {
  it("does not clear an error raised by an unrelated operation", async () => {
    useMixerStore.setState({ error: "backup failed" });
    invoke.mockResolvedValueOnce([]);

    await useMixerStore.getState().fetchAppStreams();

    expect(useMixerStore.getState().error).toBe("backup failed");
  });
});

describe("toggleMonitor", () => {
  it("flips optimistically and calls the backend", async () => {
    const store = useMixerStore.getState();
    await store.toggleMonitor("sink_game");

    expect(useMixerStore.getState().monitors["sink_game"]).toBe(true);
    expect(invoke).toHaveBeenCalledWith("set_monitor", {
      sinkName: "sink_game",
      enabled: true,
    });

    await useMixerStore.getState().toggleMonitor("sink_game");
    expect(useMixerStore.getState().monitors["sink_game"]).toBe(false);
  });

  it("reverts the optimistic flip when the backend rejects", async () => {
    invoke.mockRejectedValueOnce("monitoring requires the native PipeWire backend");
    const store = useMixerStore.getState();

    await store.toggleMonitor("sink_game");

    const s = useMixerStore.getState();
    expect(s.monitors["sink_game"]).toBe(false);
    expect(s.error).toContain("native PipeWire");
  });
});

describe("setMeterMode", () => {
  it("persists the visual mode without touching audio controls", async () => {
    await useMixerStore.getState().setMeterMode("off");

    expect(useMixerStore.getState().meterMode).toBe("off");
    expect(invoke).toHaveBeenCalledWith("set_meter_mode", { mode: "off" });
  });
});

describe("setChannelEq", () => {
  it("applies optimistically and debounces per channel", async () => {
    const store = useMixerStore.getState();
    const config = {
      ...defaultEqConfig(),
      enabled: true,
    };

    await store.setChannelEq("sink_game", { ...config, preamp_db: -2 });
    await store.setChannelEq("sink_game", { ...config, preamp_db: -5 });
    await store.setChannelEq("sink_chat", config);

    // Optimistic: both channels reflect their latest config immediately…
    expect(useMixerStore.getState().eqConfigs["sink_game"].preamp_db).toBe(-5);
    expect(useMixerStore.getState().eqConfigs["sink_chat"].enabled).toBe(true);
    // …but nothing has hit the backend yet (drag in progress).
    expect(invoke).not.toHaveBeenCalled();

    vi.advanceTimersByTime(100);
    // One call per channel: sink_game's two edits collapsed into the last.
    expect(invoke).toHaveBeenCalledTimes(2);
    expect(invoke).toHaveBeenCalledWith("set_channel_eq", {
      sinkName: "sink_game",
      config: { ...config, preamp_db: -5 },
      expectedProfile: null,
    });
    expect(invoke).toHaveBeenCalledWith("set_channel_eq", {
      sinkName: "sink_chat",
      config,
      expectedProfile: null,
    });
  });
});
