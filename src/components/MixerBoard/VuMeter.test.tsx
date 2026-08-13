import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useMixerStore } from "../../store/mixer";
import { publishLevels } from "../../lib/liveMeters";
import { VuMeter } from "./VuMeter";

describe("VuMeter direct scheduling", () => {
  let host: HTMLDivElement;
  let root: Root;
  let nextFrame = 1;
  let frames = new Map<number, FrameRequestCallback>();

  beforeEach(() => {
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    frames = new Map();
    nextFrame = 1;
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      const id = nextFrame++;
      frames.set(id, callback);
      return id;
    });
    vi.stubGlobal("cancelAnimationFrame", (id: number) => frames.delete(id));
    useMixerStore.setState({ meterMode: "fps_60" });
  });

  afterEach(() => {
    act(() => root.unmount());
    host.remove();
    vi.unstubAllGlobals();
  });

  const render = () => act(() => root.render(<VuMeter source="sink_game" enabled />));
  const paintNextFrame = (now: number) => act(() => {
    const [id, callback] = frames.entries().next().value as [number, FrameRequestCallback];
    frames.delete(id);
    callback(now);
  });

  it("paints a new target directly without a React state update", () => {
    render();
    act(() => publishLevels({ sink_game: [0.04, 0.03] }));
    expect(frames.size).toBe(1);
    paintNextFrame(performance.now() + 16);
    expect(host.querySelector<HTMLElement>(".meter-fill")?.style.clipPath).not.toBe("inset(100% 0px 0px 0px)");

  });
});
