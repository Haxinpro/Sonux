import { useState } from "react";
import { useMixerStore } from "../../store/mixer";
import type { AppStream } from "../../types";
import { Ms } from "../Icons";
import { AppRow } from "./AppRow";
import { InactiveRow } from "./InactiveRow";
import { useI18n } from "../../i18n";
import { HelpInfo } from "../HelpInfo";
import { applicationGroupKey, groupSeenApps } from "../../lib/appGroups";

/** Apps screen: live apps grouped by channel, previously-seen apps below
 * (pre-routable while closed), ignored apps tucked away at the bottom. */
export function AppList() {
  const { t } = useI18n();
  const appStreams = useMixerStore((s) => s.appStreams);
  const channels = useMixerStore((s) => s.channels);
  const seenApps = useMixerStore((s) => s.seenApps);
  const [showIgnored, setShowIgnored] = useState(false);

  const byName = (a: AppStream, b: AppStream) =>
    (a.alias ?? a.app_name).localeCompare(b.alias ?? b.app_name);

  const groups = [
    ...channels.map((c) => ({
      key: c.name,
      label: c.label,
      streams: appStreams.filter((s) => s.assigned_sink === c.name).sort(byName),
    })),
    {
      key: "unrouted",
      label: t("applications.unrouted"),
      streams: appStreams.filter((s) => !s.assigned_sink).sort(byName),
    },
  ].filter((g) => g.streams.length > 0);

  const liveGroups = new Set(appStreams.map(applicationGroupKey));
  const seenGroups = groupSeenApps(seenApps);
  const inactive = seenGroups
    .filter((app) => !app.ignored && !liveGroups.has(app.group_key))
    .sort((a, b) => b.last_seen - a.last_seen);
  const ignored = seenGroups.filter((app) => app.ignored);

  return (
    <div className="content">
      <div className="screen-head">
        <h1>{t("applications.title")}</h1>
        <HelpInfo label={t("applications.title")} text={t("applications.description")} className="screen-head-help" />
        <div className="screen-head-actions">
          <span className="tag">
            <Ms name="graphic_eq" />
            {t(appStreams.length === 1 ? "applications.streamOne" : "applications.streamMany", { count: appStreams.length })}
          </span>
        </div>
      </div>
      <div className="screen-scroll">
        {appStreams.length === 0 ? (
          <div className="empty-hint">
            {t("applications.empty.title")}
            <br />
            {t("applications.empty.body")}
          </div>
        ) : (
          groups.map((group) => (
            <div className="app-group" key={group.key}>
              <div className="section-label">
                {group.label} · {group.streams.length}
              </div>
              <div className="card">
                {group.streams.map((stream) => (
                  <AppRow key={stream.index} stream={stream} />
                ))}
              </div>
            </div>
          ))
        )}

        {inactive.length > 0 && (
          <div className="app-group">
            <div className="section-label">{t("applications.notRunning", { count: inactive.length })}</div>
            <div className="card card-inactive">
              {inactive.map((app) => (
                <InactiveRow key={app.group_key} app={app} />
              ))}
            </div>
          </div>
        )}

        {ignored.length > 0 && (
          <>
            <button type="button" className="ignored-toggle" onClick={() => setShowIgnored((v) => !v)}>
              <Ms name={showIgnored ? "expand_less" : "expand_more"} />
              {t(ignored.length === 1 ? "applications.ignoredOne" : "applications.ignoredMany", { count: ignored.length })}
            </button>
            {showIgnored && (
              <div className="card card-inactive">
                {ignored.map((app) => (
                  <InactiveRow key={app.group_key} app={app} ignored />
                ))}
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );
}
