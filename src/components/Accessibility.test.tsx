import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { HSlider } from "./AppList/HSlider";
import { DspSlider } from "./Mic/DspSlider";
import { Fader } from "./MixerBoard/Fader";
import { BalanceBar } from "./MixerBoard/BalanceBar";
import { Modal } from "./Modal";
import { useMixerStore } from "../store/mixer";

describe("custom slider accessibility", () => {
  let host: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });

  afterEach(() => {
    act(() => root.unmount());
    host.remove();
  });

  it("exposes slider values and supports arrows, pages, home and end", () => {
    const vertical = vi.fn();
    const horizontal = vi.fn();
    const dsp = vi.fn();
    const bubbled = vi.fn();
    act(() => root.render(<div onKeyDown={bubbled}>
      <Fader value={50} max={150} ariaLabel="Game volume" onChange={vertical} />
      <HSlider value={25} max={100} ariaLabel="Firefox volume" valueLabel="quarter" onChange={horizontal} />
      <DspSlider label="Gain" min={-12} max={12} step={0.5} value={1}
        defaultValue={0} unit=" dB" onChange={dsp} />
    </div>));

    const sliders = host.querySelectorAll<HTMLElement>('[role="slider"]');
    expect(sliders).toHaveLength(3);
    expect(sliders[0].getAttribute("aria-orientation")).toBe("vertical");
    expect(sliders[0].getAttribute("aria-label")).toBe("Game volume");
    expect(sliders[1].getAttribute("aria-label")).toBe("Firefox volume");
    expect(sliders[1].getAttribute("aria-valuetext")).toBe("quarter");
    expect(sliders[2].getAttribute("aria-label")).toBe("Gain");

    sliders[0].dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowUp", bubbles: true }));
    sliders[1].dispatchEvent(new KeyboardEvent("keydown", { key: "End", bubbles: true }));
    sliders[2].dispatchEvent(new KeyboardEvent("keydown", { key: "PageDown", bubbles: true }));
    expect(vertical).toHaveBeenCalledWith(51);
    expect(horizontal).toHaveBeenCalledWith(100);
    expect(dsp).toHaveBeenCalledWith(-4);
    expect(bubbled).not.toHaveBeenCalled();
  });

  it("exposes the two-channel balance as a named bipolar slider", () => {
    useMixerStore.setState({
      showBalance: true,
      balanceA: "sink_game",
      balanceB: "sink_chat",
      channels: [
        { name: "sink_game", label: "Game", icon: null, volume_percent: 100, muted: false, stream_mix: true },
        { name: "sink_chat", label: "Chat", icon: null, volume_percent: 60, muted: false, stream_mix: true },
      ],
    });
    act(() => root.render(<BalanceBar />));

    const slider = host.querySelector<HTMLElement>('.bal-track[role="slider"]')!;
    expect(slider.getAttribute("aria-label")).toContain("Game and Chat");
    expect(slider.getAttribute("aria-valuemin")).toBe("-100");
    expect(slider.getAttribute("aria-valuemax")).toBe("100");
    expect(slider.getAttribute("aria-valuetext")).toBe("Game 100%, Chat 60%");

    act(() => useMixerStore.setState({
      channels: [
        { name: "sink_game", label: "Game", icon: null, volume_percent: 150, muted: false, stream_mix: true },
        { name: "sink_chat", label: "Chat", icon: null, volume_percent: 0, muted: false, stream_mix: true },
      ],
    }));
    expect(slider.getAttribute("aria-valuenow")).toBe("-100");
    expect(slider.querySelector<HTMLElement>(".bal-cap")?.style.left).toBe("0%");
  });

  it("announces numeric tuning values and the current endpoint", () => {
    const render = (value: number) => act(() => root.render(
      <DspSlider label="Performance" endLabel="Immersion" inlineEndLabel
        min={0} max={100} step={1} value={value} defaultValue={50} unit="%" onChange={() => {}} />,
    ));
    render(50);
    const slider = host.querySelector<HTMLElement>('[role="slider"]')!;
    expect(slider.getAttribute("aria-valuetext")).toBe("50% between Performance and Immersion");
    render(100);
    expect(slider.getAttribute("aria-valuetext")).toBe("Immersion (100%)");
  });
});

describe("Modal focus management", () => {
  let host: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });

  afterEach(() => {
    act(() => root.unmount());
    host.remove();
  });

  it("enters, traps and restores focus for an aria-modal dialog", () => {
    const opener = document.createElement("button");
    document.body.append(opener);
    opener.focus();
    const close = vi.fn();
    const render = (open: boolean) => act(() => root.render(
      <Modal open={open} title="Edit channel" onClose={close}>
        <button type="button">First action</button>
        <input aria-label="Channel name" />
        <button type="button">Last action</button>
      </Modal>,
    ));

    render(true);
    const dialog = document.querySelector<HTMLElement>('[role="dialog"]')!;
    expect(dialog.getAttribute("aria-modal")).toBe("true");
    const buttons = dialog.querySelectorAll<HTMLButtonElement>("button");
    expect(document.activeElement).toBe(buttons[0]);
    const input = dialog.querySelector<HTMLInputElement>("input")!;
    input.focus();
    // A fresh inline callback must not rerun focus entry and steal focus.
    act(() => root.render(
      <Modal open title="Edit channel" onClose={() => close()}>
        <button type="button">First action</button>
        <input aria-label="Channel name" />
        <button type="button">Last action</button>
      </Modal>,
    ));
    expect(document.activeElement?.getAttribute("aria-label")).toBe("Channel name");
    opener.focus();
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Tab", bubbles: true }));
    expect(document.activeElement).toBe(buttons[0]);
    buttons[buttons.length - 1].focus();
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Tab", bubbles: true }));
    expect(document.activeElement).toBe(buttons[0]);

    render(false);
    expect(document.activeElement).toBe(opener);
    opener.remove();
  });

  it("preserves child autofocus and skips hidden controls in both trap directions", () => {
    act(() => root.render(
      <Modal open title="Autofocus" onClose={() => {}}>
        <div style={{ display: "none" }}><button type="button">Hidden action</button></div>
        <input autoFocus aria-label="Preferred field" />
        <button type="button">Visible last</button>
      </Modal>,
    ));
    const dialog = document.querySelector<HTMLElement>('[role="dialog"]')!;
    expect(document.activeElement?.getAttribute("aria-label")).toBe("Preferred field");
    const focusableButtons = [...dialog.querySelectorAll<HTMLButtonElement>("button")]
      .filter((button) => button.textContent !== "Hidden action");
    focusableButtons[0].focus();
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Tab", shiftKey: true }));
    expect(document.activeElement).toBe(focusableButtons[focusableButtons.length - 1]);
  });

  it("recovers containment when the focused child is removed on rerender", () => {
    const render = (editing: boolean) => act(() => root.render(
      <Modal open title="Dynamic" onClose={() => {}}>
        {editing ? <input autoFocus aria-label="Temporary editor" /> : null}
        <button type="button">Remaining action</button>
      </Modal>,
    ));
    render(true);
    expect(document.activeElement?.getAttribute("aria-label")).toBe("Temporary editor");
    render(false);
    const dialog = document.querySelector<HTMLElement>('[role="dialog"]')!;
    expect(dialog.contains(document.activeElement)).toBe(true);
    expect((document.activeElement as HTMLElement).getAttribute("title")).toBe("Close");
  });

  it("can require an explicit in-dialog action", () => {
    const close = vi.fn();
    act(() => root.render(
      <Modal open dismissible={false} title="Welcome" onClose={close}>
        <button type="button">Skip</button>
      </Modal>,
    ));
    const dialog = document.querySelector<HTMLElement>('[role="dialog"]')!;
    expect(dialog.querySelector('[title="Close"]')).toBeNull();
    act(() => window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" })));
    act(() => dialog.parentElement!.click());
    expect(close).not.toHaveBeenCalled();
    expect(document.activeElement?.textContent).toBe("Skip");
  });
});
