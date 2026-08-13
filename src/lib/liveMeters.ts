import { nextMeterFrameAt } from "./meter";

export type Levels = Record<string, [number, number]>;

type LevelListener = (level: [number, number]) => void;
type MeterFrame = (now: number) => boolean;
interface ActiveFrame {
  interval: number;
  nextAt: number;
}

const listeners = new Map<string, Set<LevelListener>>();
const latestLevels = new Map<string, [number, number]>();
const activeFrames = new Map<MeterFrame, ActiveFrame>();
let animationFrame = 0;
let wakeTimer = 0;
const FRAME_LEAD_MS = 4;

/** Deliver native peaks directly to meter refs, outside React state. */
export function publishLevels(levels: Levels): void {
  for (const [name, level] of Object.entries(levels)) {
    const previous = latestLevels.get(name);
    if (previous && Math.abs(previous[0] - level[0]) < 0.0001 && Math.abs(previous[1] - level[1]) < 0.0001) {
      continue;
    }
    latestLevels.set(name, level);
    for (const listener of listeners.get(name) ?? []) listener(level);
  }
}

/** Forget cached peaks when metering is disabled, avoiding a stale flash on resume. */
export function clearPublishedLevels(): void {
  latestLevels.clear();
}

export function subscribeLevel(name: string, listener: LevelListener): () => void {
  const named = listeners.get(name) ?? new Set<LevelListener>();
  named.add(listener);
  listeners.set(name, named);
  const latest = latestLevels.get(name);
  if (latest) listener(latest);
  return () => {
    named.delete(listener);
    if (named.size === 0) listeners.delete(name);
  };
}

function scheduleNextFrame(): void {
  if (activeFrames.size === 0 || animationFrame !== 0 || wakeTimer !== 0) return;
  const now = performance.now();
  const nextAt = Math.min(...[...activeFrames.values()].map((entry) => entry.nextAt));
  const delay = nextAt - now;
  if (delay > FRAME_LEAD_MS) {
    wakeTimer = window.setTimeout(() => {
      wakeTimer = 0;
      animationFrame = requestAnimationFrame(runMeters);
    }, delay - FRAME_LEAD_MS);
  } else {
    animationFrame = requestAnimationFrame(runMeters);
  }
}

function cancelScheduledFrame(): void {
  if (animationFrame !== 0) cancelAnimationFrame(animationFrame);
  if (wakeTimer !== 0) window.clearTimeout(wakeTimer);
  animationFrame = 0;
  wakeTimer = 0;
}

function runMeters(now: number): void {
  animationFrame = 0;
  for (const [frame, entry] of [...activeFrames]) {
    if (now + 0.5 < entry.nextAt) continue;
    if (!frame(now)) {
      activeFrames.delete(frame);
    } else {
      // Advance from the previous deadline rather than from the actual frame.
      // This avoids a 120 FPS cap collapsing to 72 FPS on a 144 Hz display.
      entry.nextAt = nextMeterFrameAt(entry.nextAt, now, entry.interval);
    }
  }
  scheduleNextFrame();
}

/** Wake one meter on the single animation frame shared by the whole mixer. */
export function wakeMeter(frame: MeterFrame, interval: number): void {
  if (activeFrames.has(frame)) return;
  activeFrames.set(frame, { interval, nextAt: performance.now() });
  // A newly active meter may be due before the timer for existing meters.
  cancelScheduledFrame();
  scheduleNextFrame();
}

export function sleepMeter(frame: MeterFrame): void {
  activeFrames.delete(frame);
  cancelScheduledFrame();
  scheduleNextFrame();
}
