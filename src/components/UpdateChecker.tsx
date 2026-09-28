import { useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n";
import { useUpdates } from "../store/updates";
import { Ms } from "./Icons";

function GitHubMark() {
  return (
    <svg className="github-mark" viewBox="0 0 24 24" aria-hidden="true">
      <path fill="currentColor" d="M12 .7a11.5 11.5 0 0 0-3.6 22.4c.6.1.8-.3.8-.6v-2.2c-3.3.7-4-1.4-4-1.4-.5-1.4-1.3-1.8-1.3-1.8-1.1-.7.1-.7.1-.7 1.2.1 1.8 1.2 1.8 1.2 1.1 1.8 2.8 1.3 3.5 1 .1-.8.4-1.3.8-1.6-2.6-.3-5.4-1.3-5.4-5.7 0-1.3.5-2.3 1.2-3.1-.1-.3-.5-1.5.1-3.1 0 0 1-.3 3.1 1.2a10.8 10.8 0 0 1 5.7 0c2.2-1.5 3.1-1.2 3.1-1.2.6 1.6.2 2.8.1 3.1.8.8 1.2 1.8 1.2 3.1 0 4.4-2.7 5.4-5.4 5.7.4.4.8 1.1.8 2.2v3.2c0 .3.2.7.8.6A11.5 11.5 0 0 0 12 .7Z" />
    </svg>
  );
}

async function copyReleaseUrl(releaseUrl: string): Promise<void> {
  if (navigator.clipboard?.writeText) {
    await navigator.clipboard.writeText(releaseUrl);
    return;
  }
  const input = document.createElement("textarea");
  input.value = releaseUrl;
  input.style.position = "fixed";
  input.style.opacity = "0";
  document.body.append(input);
  input.select();
  const copied = document.execCommand("copy");
  input.remove();
  if (!copied) throw new Error("Clipboard copy was rejected");
}

export function CopyReleaseLinkButton({ releaseUrl, className = "" }: Readonly<{ releaseUrl: string; className?: string }>) {
  const { t } = useI18n();
  const [copyState, setCopyState] = useState<"idle" | "copied" | "error">("idle");
  const resetTimer = useRef<number | null>(null);

  useEffect(() => () => {
    if (resetTimer.current !== null) window.clearTimeout(resetTimer.current);
  }, []);

  const copy = async () => {
    try {
      await copyReleaseUrl(releaseUrl);
      setCopyState("copied");
    } catch {
      setCopyState("error");
    }
    if (resetTimer.current !== null) window.clearTimeout(resetTimer.current);
    resetTimer.current = window.setTimeout(() => setCopyState("idle"), 2200);
  };

  const buttonText = t(copyState === "copied" ? "updates.link.copied" : copyState === "error" ? "updates.link.copyError" : "updates.link.copy");

  return (
    <>
      <button
        type="button"
        className={className}
        onClick={() => void copy()}
        aria-label={t("updates.link.copyAria", { url: releaseUrl })}
        title={releaseUrl}
      >
        <GitHubMark />
        <span aria-hidden="true">{buttonText}</span>
      </button>
      <span className="sr-only" role="status" aria-live="polite">
        {copyState === "idle" ? "" : buttonText}
      </span>
    </>
  );
}

export function UpdateBanner() {
  const { t } = useI18n();
  const dismissed = useUpdates((state) => state.dismissed);
  const status = useUpdates((state) => state.status);
  const automaticChecks = useUpdates((state) => state.automaticChecks);
  const latestVersion = useUpdates((state) => state.latestVersion);
  const releaseUrl = useUpdates((state) => state.releaseUrl);
  const lastCheckKind = useUpdates((state) => state.lastCheckKind);
  const dismiss = useUpdates((state) => state.dismiss);

  if (status !== "available" || !automaticChecks || lastCheckKind !== "automatic" || dismissed || !latestVersion || !releaseUrl) return null;

  return (
    <div className="update-checker-banner" role="status">
      <Ms name="new_releases" />
      <div className="update-checker-message">
        <strong>{t("updates.available.title")}</strong>
        <span>{t("updates.available.version", { version: latestVersion })}</span>
      </div>
      <CopyReleaseLinkButton releaseUrl={releaseUrl} className="update-checker-action" />
      <button
        type="button"
        className="update-checker-dismiss"
        aria-label={t("updates.dismiss")}
        title={t("updates.dismiss")}
        onClick={dismiss}
      >
        <Ms name="close" />
      </button>
    </div>
  );
}
