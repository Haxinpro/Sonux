import { useState } from "react";
import { useMixerStore } from "../../store/mixer";
import type { BusDef } from "../../types";
import { busMembers, MASTER_BUS, MAX_VOLUME } from "../../types";
import { perceptual, volToDb } from "../../lib/audio";
import { Ms } from "../Icons";
import { ConfirmModal } from "../ConfirmModal";
import { MenuCheckItem } from "../MenuItem";
import { Popover } from "../Popover";
import { Fader } from "./Fader";
import { VuMeter } from "./VuMeter";
import { ProfileMenu } from "../TitleBar/ProfileMenu";
import { AppIcon } from "../AppList/AppIcon";

const APP_DRAG_TYPE = "application/x-sonux-routed-app";

interface DraggableApp {
  key: string;
  name: string;
  iconPath: string | null;
  active: boolean;
  streamIndexes: number[];
  matchProp: string;
  matchValue: string;
}

/**
 * A mix (record bus): aggregates the chosen channels into a capturable
 * source. The label is exactly the device name recorders display - rename
 * it and OBS sees the new name. Volume/mute shape what recorders hear,
 * not what you hear.
 */
/** Compact "what this mix carries" label for the membership button. */
function memberLabel(exclude: boolean, carried: number, all: number): string {
  if (!exclude) return `${carried} ${carried === 1 ? "channel" : "channels"}`;
  if (carried === all) return "all channels";
  return `all but ${all - carried}`;
}

export function BusStrip({ bus }: Readonly<{ bus: BusDef }>) {
  const channels = useMixerStore((s) => s.channels);
  const setBusMembers = useMixerStore((s) => s.setBusMembers);
  const setBusExclude = useMixerStore((s) => s.setBusExclude);
  const renameBus = useMixerStore((s) => s.renameBus);
  const removeBus = useMixerStore((s) => s.removeBus);
  const level = useMixerStore((s) => s.levels[bus.name]);
  const monitoring = useMixerStore((s) => s.monitors[bus.name] ?? false);
  const toggleMonitor = useMixerStore((s) => s.toggleMonitor);
  const setBusVolume = useMixerStore((s) => s.setBusVolume);
  const setBusMute = useMixerStore((s) => s.setBusMute);
  const appStreams = useMixerStore((s) => s.appStreams);
  const routeApp = useMixerStore((s) => s.routeApp);
  const setAppAssignment = useMixerStore((s) => s.setAppAssignment);

  const [managing, setManaging] = useState(false);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [confirmingDelete, setConfirmingDelete] = useState(false);
  const [appDragOver, setAppDragOver] = useState(false);

  // The master mix always exists and carries every channel.
  const isMaster = bus.name === MASTER_BUS;
  const displayLabel = isMaster ? "Master" : bus.label;

  // Volume/mute live on the persisted bus, so they survive remounts, profile
  // switches, and restarts (the backend re-applies them to the fresh node).
  const volume = bus.volume_percent;
  const muted = bus.muted;

  const amplitude = Math.max(level?.[0] ?? 0, level?.[1] ?? 0);

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
  const assignedIdentities = new Set(
    appStreams
      .filter((stream) => stream.assigned_sink !== null)
      .map((stream) => `${stream.match_prop}\0${stream.match_value}`),
  );
  const unroutedByKey = new Map<string, DraggableApp>();
  for (const stream of appStreams) {
    const key = `${stream.match_prop}\0${stream.match_value}`;
    if (stream.assigned_sink !== null || assignedIdentities.has(key)) continue;
    const existing = unroutedByKey.get(key);
    if (existing) {
      existing.streamIndexes.push(stream.index);
      existing.active ||= stream.active;
    } else {
      unroutedByKey.set(key, {
        key,
        name: stream.alias ?? stream.app_name,
        iconPath: stream.icon_path,
        active: stream.active,
        streamIndexes: [stream.index],
        matchProp: stream.match_prop,
        matchValue: stream.match_value,
      });
    }
  }
  const unroutedApps = Array.from(unroutedByKey.values()).sort((left, right) =>
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
      for (const streamIndex of app.streamIndexes) void routeApp(streamIndex, "");
      void setAppAssignment(
        { match_prop: app.matchProp, match_value: app.matchValue },
        null,
      );
    } catch {
      useMixerStore.setState({ error: "The dragged application could not be unrouted." });
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
          aria-label={`Delete mix ${bus.label}`}
          title="Delete mix"
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
              title='Double-click to rename - recorders see this name'
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
          <div className="strip-meta" title="The master mix always carries every channel">
            all channels
          </div>
        ) : (
          <div style={{ position: "relative" }}>
            <button
              type="button"
              className="strip-meta strip-meta-btn"
              title="Choose which channels this mix carries"
              onClick={() => setManaging(true)}
            >
              {memberLabel(bus.exclude, carried.length, allNames.length)}
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
                title="New channels join this mix automatically - keep the ones you don't want unchecked"
                onClick={() => void setBusExclude(bus.name, !bus.exclude)}
              >
                <span className="menu-item-label">Auto-include new channels</span>
              </MenuCheckItem>
            </Popover>
          </div>
        )}
      </div>

      <div className="strip-preset-slot">
        {isMaster ? (
          <ProfileMenu compact />
        ) : (
          <div className="strip-preset-static">
            <Ms name="podcasts" />
            <span>Mix routing</span>
          </div>
        )}
      </div>

      <div className="strip-body">
        <Fader value={volume} max={MAX_VOLUME} onChange={applyVolume} />
        <VuMeter target={muted ? 0 : perceptual(amplitude)} />
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
          title={muted ? "Unmute this mix" : "Mute this mix (recorders hear silence)"}
        >
          <Ms name={muted ? "volume_off" : "volume_up"} style={{ fontSize: 16 }} />
        </button>
        <button
          type="button"
          className={"sbtn" + (monitoring ? " on-mon" : "")}
          onClick={() => void toggleMonitor(bus.name)}
          aria-pressed={monitoring}
          title="Monitor - hear what this mix carries on the default output"
        >
          <Ms name="headphones" style={{ fontSize: 16 }} />
        </button>
      </div>

      <div
        className={"strip-apps" + (isMaster ? "" : " strip-apps-passive")}
        aria-label={isMaster ? "Applications waiting to be routed" : `Channels carried by ${bus.label}`}
      >
        <div className="strip-apps-label">{isMaster ? "Apps to be routed" : "Channels"}</div>
        {isMaster ? (
          unroutedApps.length === 0 ? (
            <div className="strip-apps-empty">All active apps are routed</div>
          ) : (
            unroutedApps.map((app) => (
              <div
                className={"strip-app-chip" + (app.active ? " active" : "")}
                key={app.key}
                draggable
                title={`Drag ${app.name} to a channel`}
                onDragStart={(event) => {
                  event.dataTransfer.effectAllowed = "move";
                  event.dataTransfer.setData(APP_DRAG_TYPE, JSON.stringify(app));
                }}
              >
                <span className="strip-app-icon"><AppIcon iconPath={app.iconPath} /></span>
                <span className="strip-app-name">{app.name}</span>
                {app.active && <span className="strip-app-live" title="Running" />}
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
        title={`Select "${bus.label}" as an audio source in OBS or any recorder`}
      >
        {isMaster && (
          <>
            <Ms name="podcasts" />
            Recording source
          </>
        )}
      </div>

      <ConfirmModal
        open={confirmingDelete}
        onClose={() => setConfirmingDelete(false)}
        title={`Delete mix "${bus.label}"?`}
        confirmLabel="Delete mix"
        onConfirm={() => void removeBus(bus.name)}
      >
        Recorders capturing "{bus.label}" will go silent. Channels are unaffected.
      </ConfirmModal>
    </div>
  );
}
