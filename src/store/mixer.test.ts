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
