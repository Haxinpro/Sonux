import { useState } from "react";
import { useMixerStore } from "../../store/mixer";
import type { VirtualSink } from "../../types";
import { defaultEqConfig, MAX_VOLUME } from "../../types";
import { channelAccentClass, channelIcon, Ms, ICON_CHOICES } from "../Icons";
import { ConfirmModal } from "../ConfirmModal";
import { Popover } from "../Popover";
import { volToDb } from "../../lib/audio";
import { AppIcon } from "../AppList/AppIcon";
import { ChannelApps } from "./ChannelApps";
import { Fader } from "./Fader";
import { OutputSelect } from "./OutputSelect";
import { VuMeter } from "./VuMeter";
import { EqPresetMenu } from "../Eq/EqPresetMenu";

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

interface ChannelStripProps {
  channel: VirtualSink;
  appCount: number;
  /** Drag-reorder wiring (owned by MixerBoard). */
  dragging: boolean;
  onGripDragStart: (e: React.DragEvent) => void;
  onGripDragEnd: () => void;
  onStripDragOver: (e: React.DragEvent) => void;
  onOpenSettings: () => void;
}

export function ChannelStrip({
  channel,
  appCount,
  dragging,
  onGripDragStart,
  onGripDragEnd,
  onStripDragOver,
  onOpenSettings,
}: Readonly<ChannelStripProps>) {
  const setChannelVolume = useMixerStore((s) => s.setChannelVolume);
  const toggleMute = useMixerStore((s) => s.toggleMute);
  const output = useMixerStore((s) => s.channelOutputs[channel.name] ?? null);
  const resolvedOutput = useMixerStore((s) => s.resolvedOutputs[channel.name] ?? null);
  const failover = useMixerStore((s) => s.channelFailover[channel.name] ?? true);
  const setChannelOutput = useMixerStore((s) => s.setChannelOutput);
  const setChannelFailover = useMixerStore((s) => s.setChannelFailover);
  const renameChannel = useMixerStore((s) => s.renameChannel);
  const removeChannel = useMixerStore((s) => s.removeChannel);
  const setChannelIcon = useMixerStore((s) => s.setChannelIcon);
  const channelCount = useMixerStore((s) => s.channels.length);
  const monitoring = useMixerStore((s) => s.monitors[channel.name] ?? false);
  const toggleMonitor = useMixerStore((s) => s.toggleMonitor);
  const appStreams = useMixerStore((s) => s.appStreams);
  const routeApp = useMixerStore((s) => s.routeApp);
  const setAppAssignment = useMixerStore((s) => s.setAppAssignment);
  const eqConfig = useMixerStore((s) => s.eqConfigs[channel.name] ?? null) ?? defaultEqConfig();
  const setChannelEq = useMixerStore((s) => s.setChannelEq);
  const eqEnabled = eqConfig.enabled;
  const accentClass = ` ${channelAccentClass(channel)}`;

  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [confirmingDelete, setConfirmingDelete] = useState(false);
  const [pickingIcon, setPickingIcon] = useState(false);
  const [managingApps, setManagingApps] = useState(false);
  const [appDragOver, setAppDragOver] = useState(false);

  const commitRename = () => {
    setEditing(false);
    const label = draft.trim();
    if (label && label !== channel.label) {
      void renameChannel(channel.name, label);
    }
  };

  const appsByKey = new Map<string, DraggableApp>();
  for (const app of appStreams) {
    const key = `${app.match_prop}\0${app.match_value}`;
    if (app.assigned_sink !== channel.name) continue;
    const existing = appsByKey.get(key);
    if (existing) {
      existing.streamIndexes.push(app.index);
      existing.active ||= app.active;
    } else {
      appsByKey.set(key, {
        key,
        name: app.alias ?? app.app_name,
        iconPath: app.icon_path,
        active: app.active,
        streamIndexes: [app.index],
        matchProp: app.match_prop,
        matchValue: app.match_value,
      });
    }
  }
  const apps = Array.from(appsByKey.values());
  apps.sort((a, b) => Number(b.active) - Number(a.active) || a.name.localeCompare(b.name));

  const hasAppDrag = (event: React.DragEvent) =>
    Array.from(event.dataTransfer.types).includes(APP_DRAG_TYPE);

  const dropApp = (event: React.DragEvent) => {
    const raw = event.dataTransfer.getData(APP_DRAG_TYPE);
    if (!raw) return;
    event.preventDefault();
    setAppDragOver(false);
    try {
      const app = JSON.parse(raw) as DraggableApp;
      if (app.streamIndexes.length > 0) {
        for (const streamIndex of app.streamIndexes) void routeApp(streamIndex, channel.name);
        void setAppAssignment(
          { match_prop: app.matchProp, match_value: app.matchValue },
          channel.name,
        );
      } else {
        void setAppAssignment(
          { match_prop: app.matchProp, match_value: app.matchValue },
          channel.name,
        );
      }
    } catch {
      useMixerStore.setState({ error: "The dragged application could not be routed." });
    }
  };

  return (
    <div
      className={
        "strip channel-strip" +
        accentClass +
        (channel.muted ? " muted" : "") +
        (dragging ? " dragging" : "") +
        (appDragOver ? " app-drop-target" : "")
      }
      onDragOver={(event) => {
        if (hasAppDrag(event)) {
          event.preventDefault();
          event.dataTransfer.dropEffect = "move";
          setAppDragOver(true);
        } else {
          onStripDragOver(event);
        }
      }}
      onDragLeave={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setAppDragOver(false);
      }}
      onDrop={(event) => {
        if (hasAppDrag(event)) dropApp(event);
        else event.preventDefault();
      }}
    >
      {channelCount > 1 && (
        <span
          className="strip-grip"
          draggable
          title="Drag to reorder"
          onDragStart={onGripDragStart}
          onDragEnd={onGripDragEnd}
        >
          <Ms name="drag_indicator" />
        </span>
      )}
      {channelCount > 1 && (
        <button
          type="button"
          className="strip-x"
          aria-label={`Delete channel ${channel.label}`}
          title="Delete channel"
          onClick={() => setConfirmingDelete(true)}
        >
          <Ms name="close" />
        </button>
      )}

      <div className="strip-head">
        <div className="strip-title-row">
          <div style={{ position: "relative" }}>
            <button
              type="button"
              className="strip-icon strip-icon-btn"
              title="Change icon"
              aria-label={`Change icon for ${channel.label}`}
              onClick={() => setPickingIcon(true)}
            >
              <Ms name={channelIcon(channel)} />
            </button>
            <Popover
              open={pickingIcon}
              onClose={() => setPickingIcon(false)}
              side="bottom"
              align="center"
              style={{ minWidth: 196 }}
            >
              <div className="icon-grid">
                {ICON_CHOICES.map((icon) => (
                  <button
                    type="button"
                    key={icon}
                    className={"icon-cell" + (channelIcon(channel) === icon ? " sel" : "")}
                    onClick={() => {
                      setPickingIcon(false);
                      void setChannelIcon(channel.name, icon);
                    }}
                  >
                    <Ms name={icon} />
                  </button>
                ))}
              </div>
            </Popover>
          </div>
          {editing ? (
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
              title="Double-click to rename"
              onDoubleClick={() => {
                setDraft(channel.label);
                setEditing(true);
              }}
            >
              {channel.label}
            </div>
          )}
        </div>
        <div style={{ position: "relative" }}>
          <button
            type="button"
            className="strip-meta strip-meta-btn"
            title="Choose which apps play through this channel"
            onClick={() => setManagingApps(true)}
          >
            {appCount} {appCount === 1 ? "app" : "apps"}
            <Ms name="expand_more" style={{ fontSize: 13 }} />
          </button>
          <ChannelApps
            channel={channel}
            open={managingApps}
            onClose={() => setManagingApps(false)}
          />
        </div>
      </div>

      <div className="strip-preset-slot">
        <EqPresetMenu
          compact
          sinkName={channel.name}
          config={eqConfig}
          onApply={(config) => void setChannelEq(channel.name, config)}
          onError={(error) => useMixerStore.setState({ error })}
        />
      </div>

      <div className="strip-body">
        <Fader
          value={channel.volume_percent}
          max={MAX_VOLUME}
          ariaLabel={`${channel.label} volume`}
          onChange={(v) => void setChannelVolume(channel.name, v)}
        />
        <VuMeter source={channel.name} enabled={!channel.muted} />
      </div>

      <div className="strip-readout">
        {channel.volume_percent}
        <span style={{ fontSize: 11 }}>%</span>{" "}
        <span className="db">{volToDb(channel.volume_percent)}</span>
      </div>

      <div className="strip-btns">
        <button
          type="button"
          className={"sbtn" + (channel.muted ? " on-mute" : "")}
          onClick={() => void toggleMute(channel.name, !channel.muted)}
          aria-pressed={channel.muted}
          title={channel.muted ? "Unmute" : "Mute"}
        >
          <Ms name={channel.muted ? "volume_off" : "volume_up"} style={{ fontSize: 16 }} />
        </button>
        <button
          type="button"
          className={"sbtn" + (monitoring ? " on-mon" : "")}
          onClick={() => void toggleMonitor(channel.name)}
          aria-pressed={monitoring}
          title="Monitor - listen to this channel on the default output"
        >
          <Ms name="headphones" style={{ fontSize: 16 }} />
        </button>
        <button
          type="button"
          className={"sbtn" + (eqEnabled ? " on-eq" : "")}
          onClick={onOpenSettings}
          aria-pressed={eqEnabled}
          title={`Open ${channel.label} settings`}
        >
          <Ms name="tune" style={{ fontSize: 16 }} />
        </button>
      </div>

      <div className="strip-apps" aria-label={`Applications routed to ${channel.label}`}>
        <div className="strip-apps-label">Apps</div>
        {apps.length === 0 ? (
          <div className="strip-apps-empty">Drop apps here</div>
        ) : (
          apps.map((app) => (
            <div
              className={"strip-app-chip" + (app.active ? " active" : "")}
              key={app.key}
              draggable
              title={`Drag ${app.name} to another channel`}
              onDragStart={(event) => {
                event.stopPropagation();
                event.dataTransfer.effectAllowed = "move";
                event.dataTransfer.setData(APP_DRAG_TYPE, JSON.stringify(app));
              }}
              onDragEnd={() => setAppDragOver(false)}
            >
              <span className="strip-app-icon">
                <AppIcon iconPath={app.iconPath} />
              </span>
              <span className="strip-app-name">{app.name}</span>
              {app.active && <span className="strip-app-live" title="Running" />}
            </div>
          ))
        )}
      </div>

      <OutputSelect
        compact
        value={output}
        resolved={resolvedOutput}
        failover={failover}
        onFailoverChange={(enabled) => void setChannelFailover(channel.name, enabled)}
        onChange={(o) => void setChannelOutput(channel.name, o)}
      />

      <ConfirmModal
        open={confirmingDelete}
        onClose={() => setConfirmingDelete(false)}
        title={`Delete "${channel.label}"?`}
        confirmLabel="Delete channel"
        onConfirm={() => void removeChannel(channel.name)}
      >
        Apps routed to this channel return to the default output. Its saved
        routing is removed.
      </ConfirmModal>
    </div>
  );
}
