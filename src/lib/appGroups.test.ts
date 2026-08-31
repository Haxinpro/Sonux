import { describe, expect, it } from "vitest";
import type { SeenApp } from "../types";
import { applicationGroupKey, groupRouteState, groupSeenApps } from "./appGroups";

function seen(overrides: Partial<SeenApp>): SeenApp {
  return {
    match_prop: "application.name",
    match_value: "helper.exe",
    display_name: "Steam",
    icon_name: null,
    icon_path: null,
    desktop_id: "steam",
    last_seen: 1,
    ignored: false,
    assigned_sink: null,
    alias: null,
    ...overrides,
  };
}

describe("canonical application groups", () => {
  it("groups distinct helper identities by desktop id", () => {
    const groups = groupSeenApps([
      seen({ match_value: "upc.exe", last_seen: 10 }),
      seen({ match_value: "dxdiag.exe", last_seen: 20 }),
    ]);
    expect(groups).toHaveLength(1);
    expect(groups[0].display_name).toBe("Steam");
    expect(groups[0].identities).toEqual([
      { match_prop: "application.name", match_value: "upc.exe" },
      { match_prop: "application.name", match_value: "dxdiag.exe" },
    ]);
    expect(groups[0].last_seen).toBe(20);
  });

  it("never groups unresolved apps merely because their names match", () => {
    const groups = groupSeenApps([
      seen({ desktop_id: null, match_value: "helper-a" }),
      seen({ desktop_id: null, match_value: "helper-b" }),
    ]);
    expect(groups).toHaveLength(2);
  });

  it("does not pretend mixed assignments are one route", () => {
    const [group] = groupSeenApps([
      seen({ match_value: "helper-a", assigned_sink: "sink_game" }),
      seen({ match_value: "helper-b", assigned_sink: "sink_media" }),
    ]);
    expect(group.assignment_mixed).toBe(true);
    expect(group.assigned_sink).toBeNull();
    expect(group.assigned_sinks).toEqual(["sink_game", "sink_media"]);
  });

  it("does not apply one helper identity's alias to the whole application", () => {
    const [group] = groupSeenApps([
      seen({ match_value: "launcher", alias: "My game" }),
      seen({ match_value: "renderer", alias: null }),
    ]);
    expect(group.alias).toBeNull();
  });

  it("uses the canonical id for live and historical group membership", () => {
    expect(applicationGroupKey(seen({ match_value: "one" }))).toBe("desktop:steam");
    expect(applicationGroupKey(seen({ match_value: "two", desktop_id: "STEAM" }))).toBe("desktop:steam");
  });

  it("keeps simultaneous same-identity streams on different channels mixed", () => {
    expect(groupRouteState(["sink_game", "sink_media"], "sink_game")).toEqual({
      checked: false,
      mixed: true,
    });
  });
});
