import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { clearPublishedLevels, publishLevels, sleepMeter, subscribeLevel, wakeMeter } from "./liveMeters";

describe("live meter delivery", () => {
  it("publishes only to subscribers for the matching source", () => {
    const game = vi.fn();
    const chat = vi.fn();
    const stopGame = subscribeLevel("sink_game", game);
    const stopChat = subscribeLevel("sink_chat", chat);

    publishLevels({ sink_game: [0.4, 0.3] });
    expect(game).toHaveBeenCalledWith([0.4, 0.3]);
    expect(chat).not.toHaveBeenCalled();

    stopGame();
    stopChat();
  });

  it("does not wake a source again for an unchanged level", () => {
    const listener = vi.fn();
    const stop = subscribeLevel("unchanged_source", listener);
    publishLevels({ unchanged_source: [0, 0] });
    publishLevels({ unchanged_source: [0, 0] });
    publishLevels({ unchanged_source: [0.2, 0] });
    expect(listener).toHaveBeenCalledTimes(2);
    stop();
  });

  it("forgets cached peaks while meters are off", () => {
    publishLevels({ cached_source: [0.8, 0.7] });
    clearPublishedLevels();
    const listener = vi.fn();
    const stop = subscribeLevel("cached_source", listener);
    expect(listener).not.toHaveBeenCalled();
    stop();
  });
});

describe("shared meter animation scheduler", () => {
  let nextFrame = 1;
  let frames = new Map<number, FrameRequestCallback>();

  beforeEach(() => {
    nextFrame = 1;
    frames = new Map();
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      const id = nextFrame++;
      frames.set(id, callback);
      return id;
    });
    vi.stubGlobal("cancelAnimationFrame", (id: number) => frames.delete(id));
  });

  afterEach(() => vi.unstubAllGlobals());

  it("runs multiple meters from one browser animation frame", () => {
    const game = vi.fn(() => false);
    const chat = vi.fn(() => false);
    wakeMeter(game, 1000 / 60);
    wakeMeter(chat, 1000 / 60);
    expect(frames.size).toBe(1);

    const [id, callback] = frames.entries().next().value as [number, FrameRequestCallback];
    frames.delete(id);
    const now = performance.now() + 1;
    callback(now);

    expect(game).toHaveBeenCalledWith(now);
    expect(chat).toHaveBeenCalledWith(now);
    expect(frames.size).toBe(0);
    sleepMeter(game);
    sleepMeter(chat);
  });

  it("waits for the configured interval instead of polling every display frame", () => {
    const setTimeoutSpy = vi.spyOn(window, "setTimeout");
    const meter = vi.fn(() => true);
    wakeMeter(meter, 100);
    const [id, callback] = frames.entries().next().value as [number, FrameRequestCallback];
    frames.delete(id);
    callback(performance.now() + 1);

    expect(meter).toHaveBeenCalledTimes(1);
    expect(frames.size).toBe(0);
    expect(setTimeoutSpy).toHaveBeenCalledOnce();

    sleepMeter(meter);
    setTimeoutSpy.mockRestore();
  });

  it("cancels a pending timer when its only meter sleeps", () => {
    const clearTimeoutSpy = vi.spyOn(window, "clearTimeout");
    const meter = vi.fn(() => true);
    wakeMeter(meter, 100);
    const [id, callback] = frames.entries().next().value as [number, FrameRequestCallback];
    frames.delete(id);
    callback(performance.now() + 1);

    sleepMeter(meter);
    expect(clearTimeoutSpy).toHaveBeenCalledOnce();
    expect(frames.size).toBe(0);
    clearTimeoutSpy.mockRestore();
  });
});
