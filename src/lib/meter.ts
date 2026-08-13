import type { MeterMode } from "../types";

/** Minimum time between visual frames while a meter is moving. */
export function meterFrameInterval(mode: MeterMode): number {
  switch (mode) {
    case "monitor": return 0;
    case "fps_144": return 1000 / 144;
    case "fps_120": return 1000 / 120;
    case "fps_100": return 10;
    case "fps_60": return 1000 / 60;
    case "off": return Number.POSITIVE_INFINITY;
  }
}

/** Move a meter deadline forward without accumulating timer/rAF drift. */
export function nextMeterFrameAt(previous: number, now: number, interval: number): number {
  if (interval === 0) return now;
  const elapsedIntervals = Math.floor((now - previous) / interval) + 1;
  return previous + Math.max(1, elapsedIntervals) * interval;
}

/** Stop scheduling frames once the signal, smoothing, peak and clip latch rest. */
export function meterNeedsFrame(
  mode: MeterMode,
  target: number,
  smooth: number,
  peak: number,
  clipLatched: boolean,
): boolean {
  return mode !== "off" && (
    Math.abs(target - smooth) > 0.001 ||
    peak - smooth > 0.01 ||
    clipLatched
  );
}
