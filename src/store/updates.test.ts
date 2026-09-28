import { afterEach, describe, expect, it, vi } from "vitest";
import {
  AUTOMATIC_CHECK_INTERVAL_MS,
  compareVersions,
  fetchLatestStableRelease,
  isAutomaticCheckDue,
  useUpdates,
} from "./updates";

function githubRelease(tagName: string, options: { draft?: boolean; prerelease?: boolean } = {}) {
  return {
    ok: true,
    status: 200,
    json: async () => ({
      tag_name: tagName,
      draft: options.draft ?? false,
      prerelease: options.prerelease ?? false,
    }),
  } as Response;
}

describe("update checker", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    useUpdates.setState({
      status: "idle",
      automaticChecks: false,
      latestVersion: null,
      releaseUrl: null,
      lastCheckKind: null,
      dismissed: false,
    });
    localStorage.clear();
  });

  it("reads and validates GitHub's latest stable release", async () => {
    const fetcher = vi.fn().mockResolvedValue(githubRelease("v1.3.0"));
    await expect(fetchLatestStableRelease(fetcher)).resolves.toEqual({
      version: "1.3.0",
      releaseUrl: "https://github.com/Haxinpro/Sonux/releases/tag/v1.3.0",
    });
    expect(fetcher).toHaveBeenCalledWith(
      "https://api.github.com/repos/Haxinpro/Sonux/releases/latest",
      expect.objectContaining({ cache: "no-store" }),
    );
  });

  it("rejects prereleases and unsupported release tags", async () => {
    await expect(fetchLatestStableRelease(vi.fn().mockResolvedValue(githubRelease("v1.3.0-beta.1"))))
      .rejects.toThrow("unsupported release tag");
    await expect(fetchLatestStableRelease(vi.fn().mockResolvedValue(githubRelease("v1.3.0", { prerelease: true }))))
      .rejects.toThrow("invalid stable release");
  });

  it("reports an available release using the real response", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(githubRelease("v1.3.0")));
    const check = useUpdates.getState().check("1.2.1", "manual");

    expect(useUpdates.getState().status).toBe("checking");
    await check;
    expect(useUpdates.getState()).toMatchObject({
      status: "available",
      latestVersion: "1.3.0",
      releaseUrl: "https://github.com/Haxinpro/Sonux/releases/tag/v1.3.0",
      lastCheckKind: "manual",
    });
  });

  it("reports current when the installed version is equal or newer", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(githubRelease("v1.2.0")));
    await useUpdates.getState().check("1.2.1", "manual");

    expect(useUpdates.getState().status).toBe("current");
    expect(useUpdates.getState().latestVersion).toBe("1.2.0");
  });

  it("provides a recoverable error state when GitHub cannot be reached", async () => {
    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new TypeError("offline")));
    await useUpdates.getState().check("1.2.1", "manual");

    expect(useUpdates.getState()).toMatchObject({
      status: "error",
      latestVersion: null,
      releaseUrl: null,
      dismissed: true,
    });
  });

  it("dismisses the optional banner without clearing the release result", () => {
    useUpdates.setState({ status: "available", latestVersion: "1.3.0" });
    useUpdates.getState().dismiss();

    expect(useUpdates.getState().dismissed).toBe(true);
    expect(useUpdates.getState().latestVersion).toBe("1.3.0");
  });

  it("allows automatic checks once every 24 hours", async () => {
    const now = new Date("2026-09-29T12:00:00Z").getTime();
    vi.setSystemTime(now);
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(githubRelease("v1.3.0")));

    expect(isAutomaticCheckDue(now)).toBe(true);
    await useUpdates.getState().check("1.2.1", "automatic");
    expect(isAutomaticCheckDue(now + AUTOMATIC_CHECK_INTERVAL_MS - 1)).toBe(false);
    expect(isAutomaticCheckDue(now + AUTOMATIC_CHECK_INTERVAL_MS)).toBe(true);
    expect(isAutomaticCheckDue(now - 1)).toBe(true);
    vi.useRealTimers();
  });

  it("compares dotted stable versions", () => {
    expect(compareVersions("1.3.0", "1.2.1")).toBe(1);
    expect(compareVersions("1.3.0", "1.3.0")).toBe(0);
    expect(compareVersions("1.2.1", "1.3.0")).toBe(-1);
  });
});
