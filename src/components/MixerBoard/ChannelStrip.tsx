import { useState } from "react";
import { useMixerStore } from "../../store/mixer";
import type { AppIdentity, VirtualSink } from "../../types";
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
  onChannel: boolean;
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
  const { t } = useI18n();
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
  const seenApps = useMixerStore((s) => s.seenApps);
  const routeAppGroup = useMixerStore((s) => s.routeAppGroup);
  const setAppGroupAssignment = useMixerStore((s) => s.setAppGroupAssignment);
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
    const key = applicationGroupKey(app);
    const existing = appsByKey.get(key);
    if (existing) {
      existing.streamIndexes.push(app.index);
      existing.active ||= app.active;
      existing.desktopId ??= app.desktop_id;
      existing.onChannel ||= app.assigned_sink === channel.name;
      if (!existing.identities.some((identity) => (
        identity.match_prop === app.match_prop && identity.match_value === app.match_value
      ))) existing.identities.push({ match_prop: app.match_prop, match_value: app.match_value });
    } else {
      appsByKey.set(key, {
        key,
        name: app.alias ?? app.app_name,
        iconPath: app.icon_path,
        active: app.active,
        streamIndexes: [app.index],
        identities: [{ match_prop: app.match_prop, match_value: app.match_value }],
        desktopId: app.desktop_id,
        onChannel: app.assigned_sink === channel.name,
      });
    }
  }
  for (const history of groupSeenApps(seenApps)) {
    const existing = appsByKey.get(history.group_key);
    if (!existing) continue;
    for (const identity of history.identities) {
      if (!existing.identities.some((candidate) => (
        candidate.match_prop === identity.match_prop && candidate.match_value === identity.match_value
      ))) existing.identities.push(identity);
    }
    existing.name = history.alias ?? existing.name;
    existing.iconPath ??= history.icon_path;
  }
  const apps = Array.from(appsByKey.values()).filter((app) => app.onChannel);
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
        void routeAppGroup(app.streamIndexes, app.identities, app.desktopId, channel.name);
      } else {
        void setAppGroupAssignment(app.identities, channel.name);
      }
    } catch {
      useMixerStore.setState({ error: t("mixer.routeError") });
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
          title={t("mixer.dragReorder")}
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
          aria-label={t("mixer.channel.deleteNamed", { channel: channel.label })}
          title={t("mixer.channel.delete")}
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
              title={t("mixer.channel.changeIcon")}
              aria-label={t("mixer.channel.changeIconNamed", { channel: channel.label })}
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
              title={t("mixer.renameHint")}
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
            title={t("channel.appsHint")}
            onClick={() => setManagingApps(true)}
          >
            {t(appCount === 1 ? "channel.appsOne" : "channel.appsMany", { count: appCount })}
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
          ariaLabel={t("channel.volumeLabel", { channel: channel.label })}
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
          title={t(channel.muted ? "mixer.unmute" : "channel.mute")}
        >
          <Ms name={channel.muted ? "volume_off" : "volume_up"} style={{ fontSize: 16 }} />
        </button>
        <button
          type="button"
          className={"sbtn" + (monitoring ? " on-mon" : "")}
          onClick={() => void toggleMonitor(channel.name)}
          aria-pressed={monitoring}
          title={t("channel.listenHint")}
        >
          <Ms name="headphones" style={{ fontSize: 16 }} />
        </button>
        <button
          type="button"
          className={"sbtn" + (eqEnabled ? " on-eq" : "")}
          onClick={onOpenSettings}
          aria-pressed={eqEnabled}
          title={t("mixer.channel.settings", { channel: channel.label })}
        >
          <Ms name="tune" style={{ fontSize: 16 }} />
        </button>
      </div>

      <div className="strip-apps" aria-label={t("mixer.channel.routedApps", { channel: channel.label })}>
        <div className="strip-apps-label">{t("onboarding.flow.apps")}</div>
        {apps.length === 0 ? (
          <div className="strip-apps-empty">{t("mixer.channel.dropApps")}</div>
        ) : (
          apps.map((app) => (
            <div
              className={"strip-app-chip" + (app.active ? " active" : "")}
              key={app.key}
              draggable
              title={t("mixer.channel.dragApp", { application: app.name })}
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
              {app.active && <span className="strip-app-live" title={t("mixer.running")} />}
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
        title={t("mixer.channel.deleteTitle", { channel: channel.label })}
        confirmLabel={t("mixer.channel.delete")}
        onConfirm={() => void removeChannel(channel.name)}
      >
        {t("mixer.channel.deleteBody")}
      </ConfirmModal>
    </div>
  );
}
