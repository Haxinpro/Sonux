import { create } from "zustand";

export type UpdateStatus = "idle" | "checking" | "available" | "current" | "error";
export type UpdateCheckKind = "manual" | "automatic";

export interface ReleaseInfo {
  version: string;
  releaseUrl: string;
}

const GITHUB_LATEST_RELEASE_ENDPOINT = "https://api.github.com/repos/Haxinpro/Sonux/releases/latest";
const GITHUB_RELEASE_BASE_URL = "https://github.com/Haxinpro/Sonux/releases/tag/";
const AUTOMATIC_CHECKS_KEY = "sonux-automatic-update-checks";
const LAST_AUTOMATIC_CHECK_KEY = "sonux-last-automatic-update-check";
export const AUTOMATIC_CHECK_INTERVAL_MS = 24 * 60 * 60 * 1000;
export const UPDATE_CHECK_TIMEOUT_MS = 10_000;

export function isAutomaticCheckDue(now = Date.now()): boolean {
  const stored = Number.parseInt(localStorage.getItem(LAST_AUTOMATIC_CHECK_KEY) ?? "", 10);
  return !Number.isFinite(stored) || stored > now || now - stored >= AUTOMATIC_CHECK_INTERVAL_MS;
}

export function compareVersions(left: string, right: string): number {
  const leftParts = left.replace(/^v/, "").split(".").map((part) => Number.parseInt(part, 10) || 0);
  const rightParts = right.replace(/^v/, "").split(".").map((part) => Number.parseInt(part, 10) || 0);
  const length = Math.max(leftParts.length, rightParts.length);
  for (let index = 0; index < length; index += 1) {
    const difference = (leftParts[index] ?? 0) - (rightParts[index] ?? 0);
    if (difference !== 0) return Math.sign(difference);
  }
  return 0;
}

export async function fetchLatestStableRelease(fetcher: typeof fetch = fetch): Promise<ReleaseInfo> {
  const controller = new AbortController();
  const timeout = window.setTimeout(() => controller.abort(), UPDATE_CHECK_TIMEOUT_MS);
  try {
    const response = await fetcher(GITHUB_LATEST_RELEASE_ENDPOINT, {
      headers: {
        Accept: "application/vnd.github+json",
        "X-GitHub-Api-Version": "2022-11-28",
      },
      cache: "no-store",
      signal: controller.signal,
    });
    if (!response.ok) throw new Error(`GitHub release request failed with status ${response.status}`);

    const release = await response.json() as { tag_name?: unknown; draft?: unknown; prerelease?: unknown };
    if (release.draft === true || release.prerelease === true || typeof release.tag_name !== "string") {
      throw new Error("GitHub returned an invalid stable release");
    }

    const tag = release.tag_name.trim();
    const match = /^v?(\d+\.\d+\.\d+)$/.exec(tag);
    if (!match) throw new Error("GitHub returned an unsupported release tag");

    return {
      version: match[1],
      releaseUrl: `${GITHUB_RELEASE_BASE_URL}${encodeURIComponent(tag)}`,
    };
  } finally {
    window.clearTimeout(timeout);
  }
}

interface UpdateState {
  status: UpdateStatus;
  automaticChecks: boolean;
  latestVersion: string | null;
  releaseUrl: string | null;
  lastCheckKind: UpdateCheckKind | null;
  dismissed: boolean;
  check: (installedVersion: string, kind?: UpdateCheckKind) => Promise<void>;
  setAutomaticChecks: (enabled: boolean) => void;
  dismiss: () => void;
}

let pendingCheck: Promise<void> | null = null;

export const useUpdates = create<UpdateState>((set) => ({
  status: "idle",
  automaticChecks: localStorage.getItem(AUTOMATIC_CHECKS_KEY) === "true",
  latestVersion: null,
  releaseUrl: null,
  lastCheckKind: null,
  dismissed: false,
  check: (installedVersion, kind = "manual") => {
    if (pendingCheck) return pendingCheck;
    if (kind === "automatic") {
      localStorage.setItem(LAST_AUTOMATIC_CHECK_KEY, String(Date.now()));
    }
    set({ status: "checking", lastCheckKind: kind });
    pendingCheck = (async () => {
      try {
        const release = await fetchLatestStableRelease();
        const status = compareVersions(release.version, installedVersion) > 0 ? "available" : "current";
        set({
          status,
          latestVersion: release.version,
          releaseUrl: release.releaseUrl,
          dismissed: status !== "available",
        });
      } catch {
        set({ status: "error", latestVersion: null, releaseUrl: null, dismissed: true });
      } finally {
        pendingCheck = null;
      }
    })();
    return pendingCheck;
  },
  setAutomaticChecks: (enabled) => {
    localStorage.setItem(AUTOMATIC_CHECKS_KEY, String(enabled));
    set({ automaticChecks: enabled, dismissed: enabled ? false : true });
  },
  dismiss: () => set({ dismissed: true }),
}));
