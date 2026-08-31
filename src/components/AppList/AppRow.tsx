import { useState } from "react";
import { useMixerStore } from "../../store/mixer";
import type { AppStream } from "../../types";
import { IconButton } from "../IconButton";
import { AppIcon } from "./AppIcon";
import { ChannelSelect } from "./ChannelSelect";
import { HSlider } from "./HSlider";
import { useI18n } from "../../i18n";
import { applicationGroupKey, groupSeenApps } from "../../lib/appGroups";

interface AppRowProps {
  stream: AppStream;
}

export function AppRow({ stream }: Readonly<AppRowProps>) {
  const { t } = useI18n();
  const appStreams = useMixerStore((s) => s.appStreams);
  const seenApps = useMixerStore((s) => s.seenApps);
  const routeAppGroup = useMixerStore((s) => s.routeAppGroup);
  const setAppVolume = useMixerStore((s) => s.setAppVolume);
  const renameApp = useMixerStore((s) => s.renameApp);
  const setAppGroupIgnored = useMixerStore((s) => s.setAppGroupIgnored);

  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");

  const displayName = stream.alias ?? stream.app_name;
  const groupKey = applicationGroupKey(stream);
  const liveGroup = appStreams.filter((candidate) => applicationGroupKey(candidate) === groupKey);
  const historyGroup = groupSeenApps(seenApps).find((candidate) => candidate.group_key === groupKey);
  const identities = [...(historyGroup?.identities ?? [])];
  for (const candidate of liveGroup) {
    if (!identities.some((identity) => (
      identity.match_prop === candidate.match_prop && identity.match_value === candidate.match_value
    ))) identities.push({ match_prop: candidate.match_prop, match_value: candidate.match_value });
  }
  const streamIndices = liveGroup.map((candidate) => candidate.index);

  const startEdit = () => {
    setDraft(displayName);
    setEditing(true);
  };
  const commit = () => {
    setEditing(false);
    const next = draft.trim();
    // Re-entering the discovered name clears the alias.
    void renameApp(stream, next === stream.app_name ? "" : next);
  };

  return (
    <div className="row">
      <div className="ricon">
        <AppIcon iconPath={stream.icon_path} />
      </div>
      <div className="rmain">
        {editing ? (
          <input
            className="menu-input"
            style={{ width: "100%", maxWidth: 260 }}
            value={draft}
            autoFocus
            maxLength={64}
            onChange={(e) => setDraft(e.target.value)}
            onBlur={commit}
            onKeyDown={(e) => {
              if (e.key === "Enter") commit();
              if (e.key === "Escape") setEditing(false);
            }}
          />
        ) : (
          <div className="rtitle" title={stream.app_name}>
            <span
              className={"eq" + (stream.active ? " on" : "")}
              title={t(stream.active ? "applications.playing" : "applications.silent")}
              aria-hidden="true"
            >
              <i />
              <i />
              <i />
            </span>
            <span className="rname">{displayName}</span>
            {stream.alias && (
              <span className="tag" title={t("applications.discoveredAs", { name: stream.app_name })}>
                {stream.app_name}
              </span>
            )}
            <IconButton
              reveal
              size={14}
              icon="edit"
              title={t("common.action.rename")}
              label={t("applications.rename", { name: displayName })}
              onClick={startEdit}
            />
            <IconButton
              reveal
              size={14}
              icon="visibility_off"
              title={t("applications.ignoreHint")}
              label={t("applications.ignore", { name: displayName })}
              onClick={() => void setAppGroupIgnored(identities, true)}
            />
          </div>
        )}
        <div className="rsub">{t("applications.streamNumber", { number: stream.index })}</div>
      </div>
      <div className="rtrail">
        <HSlider
          value={stream.volume_percent}
          max={100}
          ariaLabel={t("applications.volume", { name: displayName })}
          onChange={(v) => void setAppVolume(stream.index, v)}
        />
        <ChannelSelect
          value={stream.assigned_sink}
          onChange={(sinkName) => void routeAppGroup(streamIndices, identities, stream.desktop_id, sinkName)}
        />
      </div>
    </div>
  );
}
