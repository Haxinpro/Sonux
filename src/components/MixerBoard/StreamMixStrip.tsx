import { useState } from "react";
import { useMixerStore } from "../../store/mixer";
import type { AppIdentity, BusDef } from "../../types";
import { busMembers, MASTER_BUS, MAX_VOLUME } from "../../types";
import { volToDb } from "../../lib/audio";
import { Ms } from "../Icons";
import { ConfirmModal } from "../ConfirmModal";
import { MenuCheckItem } from "../MenuItem";
import { Popover } from "../Popover";
import { Fader } from "./Fader";
import { VuMeter } from "./VuMeter";
import { ProfileMenu } from "../TitleBar/ProfileMenu";
import { AppIcon } from "../AppList/AppIcon";
import { useI18n } from "../../i18n";
import { applicationGroupKey, groupSeenApps } from "../../lib/appGroups";

const APP_DRAG_TYPE = "application/x-sonux-routed-app";

interface DraggableApp {
  key: string;
  name: string;
  iconPath: string | null;
  active: boolean;
  streamIndexes: number[];
  identities: AppIdentity[];
  desktopId: string | null;
  hasUnrouted: boolean;
}

/**
 * A mix (record bus): aggregates the chosen channels into a capturable
 * source. The label is exactly the device name recorders display - rename
 * it and OBS sees the new name. Volume/mute shape what recorders hear,
 * not what you hear.
 */
/** Compact "what this mix carries" label for the membership button. */
export function BusStrip({
  bus,
  onManageProfiles,
}: Readonly<{
  bus: BusDef;
  onManageProfiles: () => void;
}>) {
  const { t } = useI18n();
  const channels = useMixerStore((s) => s.channels);
  const setBusMembers = useMixerStore((s) => s.setBusMembers);
  const setBusExclude = useMixerStore((s) => s.setBusExclude);
  const renameBus = useMixerStore((s) => s.renameBus);
  const removeBus = useMixerStore((s) => s.removeBus);
  const monitoring = useMixerStore((s) => s.monitors[bus.name] ?? false);
  const toggleMonitor = useMixerStore((s) => s.toggleMonitor);
  const setBusVolume = useMixerStore((s) => s.setBusVolume);
  const setBusMute = useMixerStore((s) => s.setBusMute);
  const appStreams = useMixerStore((s) => s.appStreams);
  const seenApps = useMixerStore((s) => s.seenApps);
  const routeAppGroup = useMixerStore((s) => s.routeAppGroup);
  const setAppGroupAssignment = useMixerStore((s) => s.setAppGroupAssignment);

  const [managing, setManaging] = useState(false);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [confirmingDelete, setConfirmingDelete] = useState(false);
  const [appDragOver, setAppDragOver] = useState(false);

  // The master mix always exists and carries every channel.
  const isMaster = bus.name === MASTER_BUS;
  const displayLabel = isMaster ? t("mixer.group.master") : bus.label;
  const memberLabel = () => {
    if (!bus.exclude) return t(carried.length === 1 ? "mixer.mix.channelOne" : "mixer.mix.channelMany", { count: carried.length });
    if (carried.length === allNames.length) return t("mixer.mix.allChannels");
    return t("mixer.mix.allBut", { count: allNames.length - carried.length });
  };

  // Volume/mute live on the persisted bus, so they survive remounts, profile
  // switches, and restarts (the backend re-applies them to the fresh node).
  const volume = bus.volume_percent;
  const muted = bus.muted;

  const applyVolume = (v: number) => void setBusVolume(bus.name, v);
  const toggleMute = () => void setBusMute(bus.name, !muted);
  const commitRename = () => {
    setEditing(false);
    const label = draft.trim();
    if (label && label !== bus.label) void renameBus(bus.name, label);
  };
  // What this mix actually carries (mode-aware).
  const allNames = channels.map((c) => c.name);
  const carried = busMembers(bus, allNames);

  // Sonar-style inbox: Master carries every channel in the signal graph,
  // while this panel shows only live playback apps that still need routing.
  const unroutedByKey = new Map<string, DraggableApp>();
  for (const stream of appStreams) {
    const key = applicationGroupKey(stream);
    const existing = unroutedByKey.get(key);
    if (existing) {
      existing.streamIndexes.push(stream.index);
      existing.active ||= stream.active;
      existing.desktopId ??= stream.desktop_id;
      existing.hasUnrouted ||= stream.assigned_sink === null;
      if (!existing.identities.some((identity) => (
        identity.match_prop === stream.match_prop && identity.match_value === stream.match_value
      ))) existing.identities.push({ match_prop: stream.match_prop, match_value: stream.match_value });
    } else {
      unroutedByKey.set(key, {
        key,
        name: stream.alias ?? stream.app_name,
        iconPath: stream.icon_path,
        active: stream.active,
        streamIndexes: [stream.index],
        identities: [{ match_prop: stream.match_prop, match_value: stream.match_value }],
        desktopId: stream.desktop_id,
        hasUnrouted: stream.assigned_sink === null,
      });
    }
  }
  for (const history of groupSeenApps(seenApps)) {
    const existing = unroutedByKey.get(history.group_key);
    if (!existing) continue;
    for (const identity of history.identities) {
      if (!existing.identities.some((candidate) => (
        candidate.match_prop === identity.match_prop && candidate.match_value === identity.match_value
      ))) existing.identities.push(identity);
    }
    existing.name = history.alias ?? existing.name;
    existing.iconPath ??= history.icon_path;
  }
  const unroutedApps = Array.from(unroutedByKey.values()).filter((app) => app.hasUnrouted).sort((left, right) =>
    left.name.localeCompare(right.name),
  );

  const hasAppDrag = (event: React.DragEvent) =>
    Array.from(event.dataTransfer.types).includes(APP_DRAG_TYPE);

  const unrouteApp = (event: React.DragEvent) => {
    const raw = event.dataTransfer.getData(APP_DRAG_TYPE);
    if (!raw) return;
    event.preventDefault();
    setAppDragOver(false);
    try {
      const app = JSON.parse(raw) as DraggableApp;
      if (app.streamIndexes.length > 0) {
        void routeAppGroup(app.streamIndexes, app.identities, app.desktopId, "");
      } else {
        void setAppGroupAssignment(app.identities, null);
      }
    } catch {
      useMixerStore.setState({ error: t("mixer.mix.unrouteError") });
    }
  };

  const toggleMember = (channelName: string) => {
    const next = carried.includes(channelName)
      ? carried.filter((c) => c !== channelName)
      : [...carried, channelName];
    void setBusMembers(bus.name, next);
  };

  return (
    <div
      className={
        "strip bus-strip" +
        (isMaster ? " master-strip" : "") +
        (muted ? " muted" : "") +
        (appDragOver ? " app-drop-target" : "")
      }
      onDragOver={(event) => {
        if (isMaster && hasAppDrag(event)) {
          event.preventDefault();
          event.dataTransfer.dropEffect = "move";
          setAppDragOver(true);
        }
      }}
      onDragLeave={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setAppDragOver(false);
      }}
      onDrop={(event) => {
        if (isMaster && hasAppDrag(event)) unrouteApp(event);
      }}
    >
      {!isMaster && (
        <button
          type="button"
          className="strip-x"
          aria-label={t("mixer.mix.deleteNamed", { mix: bus.label })}
          title={t("mixer.mix.delete")}
          onClick={() => setConfirmingDelete(true)}
        >
          <Ms name="close" />
        </button>
      )}

      <div className="strip-head">
        <div className="strip-title-row">
          <div className="strip-icon strip-icon-bus">
            <Ms name={isMaster ? "instant_mix" : "radio_button_checked"} />
          </div>
          {isMaster ? (
            <div className="strip-name">{displayLabel}</div>
          ) : editing ? (
            <input
              className="menu-input strip-name-input"
              value={draft}
              autoFocus
              maxLength={24}
              onChange={(e) => setDraft(e.target.value)}
              onBlur={commitRename}
              onKeyDown={(e) => {
                if (e.key === "Enter") commitRename();
                if (e.key === "Escape") setEditing(false);
              }}
            />
          ) : (
            <div
              className="strip-name strip-name-editable"
              title={t("mixer.mix.renameHint")}
              onDoubleClick={() => {
                setDraft(bus.label);
                setEditing(true);
              }}
            >
              {displayLabel}
            </div>
          )}
        </div>
        {isMaster ? (
          <div className="strip-meta" title={t("mixer.mix.masterHint")}>
            {t("mixer.mix.allChannels")}
          </div>
        ) : (
          <div style={{ position: "relative" }}>
            <button
              type="button"
              className="strip-meta strip-meta-btn"
              title={t("mixer.mix.chooseChannels")}
              onClick={() => setManaging(true)}
            >
              {memberLabel()}
              <Ms name="expand_more" style={{ fontSize: 13 }} />
            </button>
            <Popover
              open={managing}
              onClose={() => setManaging(false)}
              side="bottom"
              align="center"
              style={{ minWidth: 220 }}
            >
              {channels.map((c) => (
                <MenuCheckItem
                  key={c.name}
                  checked={carried.includes(c.name)}
                  onClick={() => toggleMember(c.name)}
                >
                  <span className="menu-item-label">{c.label}</span>
                </MenuCheckItem>
              ))}
              <div className="menu-div" />
              <MenuCheckItem
                checked={bus.exclude}
                title={t("mixer.mix.autoIncludeHint")}
                onClick={() => void setBusExclude(bus.name, !bus.exclude)}
              >
                <span className="menu-item-label">{t("mixer.mix.autoInclude")}</span>
              </MenuCheckItem>
            </Popover>
          </div>
        )}
      </div>

      <div className="strip-preset-slot">
        {isMaster ? (
          <ProfileMenu compact onManageProfiles={onManageProfiles} />
        ) : (
          <div className="strip-preset-static">
            <Ms name="podcasts" />
            <span>{t("mixer.mix.routing")}</span>
          </div>
        )}
      </div>

      <div className="strip-body">
        <Fader
          value={volume}
          max={MAX_VOLUME}
          ariaLabel={t("mixer.mix.volume", { mix: bus.label })}
          onChange={applyVolume}
        />
        <VuMeter source={bus.name} enabled={!muted} />
      </div>

      <div className="strip-readout">
        {volume}
        <span style={{ fontSize: 11 }}>%</span> <span className="db">{volToDb(volume)}</span>
      </div>

      <div className="strip-btns">
        <button
          type="button"
          className={"sbtn" + (muted ? " on-mute" : "")}
          onClick={toggleMute}
          aria-pressed={muted}
          title={t(muted ? "mixer.mix.unmute" : "mixer.mix.mute")}
        >
          <Ms name={muted ? "volume_off" : "volume_up"} style={{ fontSize: 16 }} />
        </button>
        <button
          type="button"
          className={"sbtn" + (monitoring ? " on-mon" : "")}
          onClick={() => void toggleMonitor(bus.name)}
          aria-pressed={monitoring}
          title={t("mixer.mix.monitor")}
        >
          <Ms name="headphones" style={{ fontSize: 16 }} />
        </button>
      </div>

      <div
        className={"strip-apps" + (isMaster ? "" : " strip-apps-passive")}
        aria-label={isMaster ? t("mixer.mix.waitingApps") : t("mixer.mix.carriedChannels", { mix: bus.label })}
      >
        <div className="strip-apps-label">{isMaster ? t("mixer.mix.appsToRoute") : t("mixer.group.channels")}</div>
        {isMaster ? (
          unroutedApps.length === 0 ? (
            <div className="strip-apps-empty">{t("mixer.mix.allRouted")}</div>
          ) : (
            unroutedApps.map((app) => (
              <div
                className={"strip-app-chip" + (app.active ? " active" : "")}
                key={app.key}
                draggable
                title={t("mixer.mix.dragApp", { application: app.name })}
                onDragStart={(event) => {
                  event.dataTransfer.effectAllowed = "move";
                  event.dataTransfer.setData(APP_DRAG_TYPE, JSON.stringify(app));
                }}
              >
                <span className="strip-app-icon"><AppIcon iconPath={app.iconPath} /></span>
                <span className="strip-app-name">{app.name}</span>
                {app.active && <span className="strip-app-live" title={t("mixer.running")} />}
              </div>
            ))
          )
        ) : (
          carried.map((name) => {
            const channel = channels.find((candidate) => candidate.name === name);
            return (
              <div className="strip-app-chip" key={name}>
                <span className="strip-app-icon"><Ms name="audio_file" /></span>
                <span className="strip-app-name">{channel?.label ?? name}</span>
              </div>
            );
          })
        )}
      </div>

      <div
        className="strip-route"
        title={t("mixer.mix.sourceHint", { mix: bus.label })}
      >
        {isMaster && (
          <>
            <Ms name="podcasts" />
            {t("mixer.mix.recordingSource")}
          </>
        )}
      </div>

      <ConfirmModal
        open={confirmingDelete}
        onClose={() => setConfirmingDelete(false)}
        title={t("mixer.mix.deleteTitle", { mix: bus.label })}
        confirmLabel={t("mixer.mix.delete")}
        onConfirm={() => void removeBus(bus.name)}
      >
        {t("mixer.mix.deleteBody", { mix: bus.label })}
      </ConfirmModal>
    </div>
  );
}
