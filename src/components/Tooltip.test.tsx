import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Tooltip, TOOLTIP_DELAY_MS } from "./Tooltip";

describe("Tooltip delegation", () => {
  let host: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    vi.useFakeTimers();
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    act(() => root.render(<Tooltip />));
  });

  afterEach(() => {
    act(() => root.unmount());
    host.remove();
    vi.useRealTimers();
  });

  it("restores a parent title when leaving through a nested child", () => {
    const parent = document.createElement("button");
    const child = document.createElement("span");
    parent.title = "Parent help";
    parent.append(child);
    document.body.append(parent);

    parent.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    expect(parent.title).toBe("");
    act(() => vi.advanceTimersByTime(TOOLTIP_DELAY_MS));
    expect(document.querySelector(".app-tooltip")?.textContent).toContain("Parent help");
    act(() => child.dispatchEvent(new MouseEvent("mouseout", {
      bubbles: true,
      relatedTarget: document.body,
    })));
    expect(parent.title).toBe("Parent help");
    expect(document.querySelector(".app-tooltip")).toBeNull();
    parent.remove();
  });

  it("suppresses titled ancestors and tracks a nested live title", async () => {
    const parent = document.createElement("div");
    const child = document.createElement("button");
    parent.title = "Parent";
    child.title = "Child";
    parent.append(child);
    document.body.append(parent);

    parent.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    child.dispatchEvent(new MouseEvent("mouseover", { bubbles: true, relatedTarget: parent }));
    expect(parent.title).toBe("");
    expect(child.title).toBe("");
    act(() => vi.advanceTimersByTime(TOOLTIP_DELAY_MS));
    expect(document.querySelector(".app-tooltip")?.textContent).toContain("Child");
    await act(async () => {
      child.title = "Updated child";
      await Promise.resolve();
    });
    expect(child.title).toBe("");
    expect(document.querySelector(".app-tooltip")?.textContent).toContain("Updated child");
    act(() => child.dispatchEvent(new MouseEvent("mouseout", {
      bubbles: true,
      relatedTarget: document.body,
    })));
    expect(parent.title).toBe("Parent");
    expect(child.title).toBe("Updated child");
    parent.remove();
  });

  it("does not resurrect a title intentionally deleted while hovered", async () => {
    const parent = document.createElement("div");
    const child = document.createElement("button");
    parent.title = "Parent fallback";
    child.title = "Temporary child";
    parent.append(child);
    document.body.append(parent);

    child.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    await act(async () => {
      child.removeAttribute("title");
      await Promise.resolve();
    });
    act(() => vi.advanceTimersByTime(TOOLTIP_DELAY_MS));
    expect(document.querySelector(".app-tooltip")?.textContent).toContain("Parent fallback");
    act(() => child.dispatchEvent(new MouseEvent("mouseout", {
      bubbles: true,
      relatedTarget: document.body,
    })));
    expect(child.hasAttribute("title")).toBe(false);
    expect(parent.title).toBe("Parent fallback");
    parent.remove();
  });

  it("shows detailed help immediately for keyboard focus and tap", () => {
    const help = document.createElement("button");
    help.dataset.tooltipTitle = "Start at login";
    help.dataset.tooltipText = "Start Sonux with your desktop session.";
    document.body.append(help);

    act(() => help.focus());
    expect(document.querySelector(".app-tooltip")?.textContent).toContain("Start Sonux with your desktop session.");

    act(() => {
      help.dispatchEvent(new Event("pointerdown", { bubbles: true }));
      help.click();
    });
    expect(document.querySelector(".app-tooltip")?.textContent).toContain("Start Sonux with your desktop session.");
    help.remove();
  });
});
