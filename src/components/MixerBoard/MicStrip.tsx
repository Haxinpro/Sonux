import { useState } from "react";
import { useMixerStore } from "../../store/mixer";
import { MAX_MIC_GAIN, type MicConfig } from "../../types";
import { Ms } from "../Icons";
import { Fader } from "./Fader";
import { VuMeter } from "./VuMeter";
import { MicPresetMenu } from "../Mic/MicPresetMenu";
import { AppIcon } from "../AppList/AppIcon";
import { InputSelect } from "./InputSelect";
import { ConfirmModal } from "../ConfirmModal";

interface MicStripProps {
  config: MicConfig;
  dragging: boolean;
  onGripDragStart: (event: React.DragEvent) => void;
  onGripDragEnd: () => void;
  onStripDragOver: (event: React.DragEvent) => void;
  onOpenSettings: () => void;
}

/** Mic channel strip (Phase 3): fader = chain gain, meters = processed signal. */
export function MicStrip({
  config: micConfig,
  dragging,
  onGripDragStart,
  onGripDragEnd,
  onStripDragOver,
  onOpenSettings,
}: Readonly<MicStripProps>) {
  const setMicChannelConfig = useMixerStore((s) => s.setMicChannelConfig);
  const setMicConfig = (patch: Partial<MicConfig>) => setMicChannelConfig(micConfig.node_name, patch);
  const monitoring = useMixerStore((s) => s.monitors[micConfig.node_name] ?? false);
  const toggleMonitor = useMixerStore((s) => s.toggleMonitor);
  const removeMicChannel = useMixerStore((s) => s.removeMicChannel);
  const allMicClients = useMixerStore((s) => s.micClients);
  const micClients = allMicClients.filter((client) => client.mic_node === micConfig.node_name);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [confirmingDelete, setConfirmingDelete] = useState(false);
  const secondary = micConfig.node_name !== "sink_mic";

  const commitRename = () => {
    setEditing(false);
    const label = draft.trim();
    if (label && label !== micConfig.output_label)
      void setMicConfig({ output_label: label });
  };

  return (
      <div
        className={"strip input-strip strip-accent-mic" + (micConfig.muted ? " muted" : "") + (!micConfig.enabled ? " mic-strip-disabled" : "") + (dragging ? " dragging" : "")}
        onDragOver={onStripDragOver}
      >
      {secondary && (
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
      {secondary && (
        <button
          type="button"
          className="strip-x"
          aria-label={`Delete microphone ${micConfig.output_label}`}
          title="Delete microphone"
          onClick={() => setConfirmingDelete(true)}
        >
          <Ms name="close" />
        </button>
      )}
      <div className="strip-head">
        <div className="strip-title-row">
          <div className="strip-icon strip-icon-mic">
            <Ms name={micConfig.enabled ? "mic" : "mic_off"} />
          </div>
          {editing ? (
            <input
              className="menu-input strip-name-input"
              value={draft}
              autoFocus
              maxLength={32}
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
              title="Double-click to rename - other apps see this name"
              onDoubleClick={() => {
                if (!micConfig.enabled) return;
                setDraft(micConfig.output_label);
                setEditing(true);
              }}
            >
              {micConfig.output_label}
            </div>
          )}
        </div>
        <div className="strip-meta">{micConfig.enabled ? "capture" : "disabled"}</div>
      </div>

      <div className="strip-preset-slot">
        <MicPresetMenu
          compact
          config={micConfig}
          onApply={(patch) => void setMicConfig(patch)}
        />
      </div>

      <div className="strip-body">
        <Fader
          value={micConfig.gain_percent}
          max={MAX_MIC_GAIN}
          ariaLabel={`${micConfig.output_label} gain`}
          onChange={(v) => void setMicConfig({ gain_percent: v })}
        />
        <VuMeter
          source={micConfig.node_name}
          enabled={micConfig.enabled && !micConfig.muted}
          mono
        />
      </div>

      <div className="strip-readout">
        {micConfig.gain_percent}
        <span style={{ fontSize: 11 }}>%</span>{" "}
        <span className="db">gain</span>
      </div>

      <div className="strip-btns">
        <button
          type="button"
          className={"sbtn" + (micConfig.muted ? " on-mute" : "")}
          onClick={() => void setMicConfig({ muted: !micConfig.muted })}
          aria-pressed={micConfig.muted}
          title={micConfig.muted ? "Unmute mic" : "Mute mic"}
        >
          <Ms name={micConfig.muted ? "mic_off" : "mic"} style={{ fontSize: 16 }} />
        </button>
        <button
          type="button"
          className={"sbtn" + (monitoring ? " on-mon" : "")}
          onClick={() => void toggleMonitor(micConfig.node_name)}
          aria-pressed={monitoring}
          title="Sidetone - hear your processed mic on the default output"
        >
          <Ms name="headphones" style={{ fontSize: 16 }} />
        </button>
      </div>

      {!micConfig.enabled ? (
        <button type="button" className="strip-apps mic-disabled-settings" onClick={onOpenSettings}>
          <Ms name="settings_voice" />
          <strong>Open mic settings</strong>
          <small>Enable this microphone for the active profile</small>
        </button>
      ) : (
        <div className="strip-apps strip-apps-passive" aria-label="Applications using the microphone">
          <div className="strip-apps-label">Apps</div>
          {micClients.length === 0 ? (
            <div className="strip-apps-empty">No apps are using this mic</div>
          ) : (
          micClients.map((client) => (
            <div
              className={"strip-app-chip" + (client.active ? " active" : "")}
              key={`${client.mic_node}\0${client.match_prop}\0${client.match_value}`}
              title={`${client.app_name} is recording from this processed mic`}
            >
              <span className="strip-app-icon">
                <AppIcon iconPath={client.icon_path} />
              </span>
              <span className="strip-app-name">{client.app_name}</span>
              {client.active && <span className="strip-app-live" title="Recording" />}
            </div>
          ))) }
        </div>
      )}

      <InputSelect
        value={micConfig.input_device}
        onChange={(inputDevice) => void setMicConfig({ input_device: inputDevice })}
      />
      <ConfirmModal
        open={confirmingDelete}
        onClose={() => setConfirmingDelete(false)}
        title={`Delete "${micConfig.output_label}"?`}
        confirmLabel="Delete microphone"
        onConfirm={() => void removeMicChannel(micConfig.node_name)}
      >
        This removes its virtual input and profile-specific processing. Applications using it will need another microphone selected.
      </ConfirmModal>
      </div>
  );
}
