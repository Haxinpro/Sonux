import { describe, expect, it } from "vitest";
import { meterFrameInterval, meterNeedsFrame, nextMeterFrameAt } from "./meter";

describe("meterFrameInterval", () => {
  it("offers progressively cheaper visual modes", () => {
    expect(meterFrameInterval("monitor")).toBe(0);
    expect(meterFrameInterval("fps_144")).toBeCloseTo(1000 / 144);
    expect(meterFrameInterval("fps_120")).toBeCloseTo(1000 / 120);
    expect(meterFrameInterval("fps_100")).toBe(10);
    expect(meterFrameInterval("fps_60")).toBeCloseTo(1000 / 60);
    expect(meterFrameInterval("off")).toBe(Number.POSITIVE_INFINITY);
  });
});

describe("nextMeterFrameAt", () => {
  it("follows the monitor when uncapped", () => {
    expect(nextMeterFrameAt(100, 106.94, 0)).toBe(106.94);
  });

  it("preserves a 120 FPS deadline across 144 Hz display frames", () => {
    const interval = 1000 / 120;
    expect(nextMeterFrameAt(100, 106.94, interval)).toBeCloseTo(108.33, 2);
    expect(nextMeterFrameAt(108.33, 120.83, interval)).toBeCloseTo(124.997, 2);
  });

  it("skips missed deadlines without creating a catch-up backlog", () => {
    expect(nextMeterFrameAt(100, 151, 10)).toBe(160);
  });
});

describe("meterNeedsFrame", () => {
  it("sleeps at silence and wakes for signal or visible decay", () => {
    expect(meterNeedsFrame("fps_60", 0, 0, 0, false)).toBe(false);
    expect(meterNeedsFrame("fps_60", 0.2, 0, 0, false)).toBe(true);
    expect(meterNeedsFrame("fps_60", 0, 0.1, 0, false)).toBe(true);
    expect(meterNeedsFrame("fps_60", 0, 0, 0.4, false)).toBe(true);
    expect(meterNeedsFrame("fps_60", 0, 0, 0, true)).toBe(true);
    expect(meterNeedsFrame("fps_60", 0.4, 0.4, 0.4, false)).toBe(false);
    expect(meterNeedsFrame("off", 1, 1, 1, true)).toBe(false);
  });
});
