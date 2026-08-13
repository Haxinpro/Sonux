import { useEffect, useState } from "react";
import { useMixerStore } from "../../store/mixer";
import { MAX_MIC_GAIN, MIC_LEVEL_KEY, MIC_DSP_DEFAULTS } from "../../types";
import { DspSlider } from "./DspSlider";
import { HSlider } from "../AppList/HSlider";
import { Ms } from "../Icons";
import { MenuItem } from "../MenuItem";
import { Popover } from "../Popover";
import { Toggle } from "../Toggle";
import { ProcessingInfo } from "../ProcessingInfo";
import { MicPresetMenu } from "./MicPresetMenu";
import { AudioTestControls } from "../AudioTestControls";
import { MicEqEditor } from "./MicEqEditor";
import { Modal } from "../Modal";
import { ConfirmModal } from "../ConfirmModal";

export function MicScreen() {
  const micConfigs = useMixerStore((s) => s.micConfigs);
  const selectedMicNode = useMixerStore((s) => s.selectedMicNode);
  const multipleMics = useMixerStore((s) => s.multipleMics);
  const addingMic = useMixerStore((s) => s.micCreationOpen);
  const setAddingMic = useMixerStore((s) => s.setMicCreationOpen);
  const selectMic = useMixerStore((s) => s.selectMic);
  const activeProfile = useMixerStore((s) => s.activeProfile);
  const inputDevices = useMixerStore((s) => s.inputDevices);
  const setMicChannelConfig = useMixerStore((s) => s.setMicChannelConfig);
  const addMicChannel = useMixerStore((s) => s.addMicChannel);
  const removeMicChannel = useMixerStore((s) => s.removeMicChannel);
  const micConfig = micConfigs.find((mic) => mic.node_name === selectedMicNode)
    ?? micConfigs.find((mic) => mic.node_name === MIC_LEVEL_KEY)
    ?? null;
  const micNode = micConfig?.node_name ?? MIC_LEVEL_KEY;
  const updateMic = (patch: Partial<NonNullable<typeof micConfig>>) => setMicChannelConfig(micNode, patch);
  const listening = useMixerStore((s) => s.monitors[micNode] ?? false);
  const toggleMonitor = useMixerStore((s) => s.toggleMonitor);
  const [deviceOpen, setDeviceOpen] = useState(false);
  const [channelOpen, setChannelOpen] = useState(false);
  const [newDeviceOpen, setNewDeviceOpen] = useState(false);
  const [copyOpen, setCopyOpen] = useState(false);
  const [deletingMic, setDeletingMic] = useState(false);
  const [newMicName, setNewMicName] = useState("");
  const [newMicDevice, setNewMicDevice] = useState<string | null>(null);
  const [copyFrom, setCopyFrom] = useState<string | null>(null);
  useEffect(() => () => setAddingMic(false), [setAddingMic]);
  if (!micConfig) {
    return (
      <div className="content">
        <div className="empty-hint" style={{ margin: "auto" }}>
          Loading mic configuration…
        </div>
      </div>
    );
  }

  const currentDevice = inputDevices.find((d) => d.name === micConfig.input_device);
  const deviceLabel =
    micConfig.input_device === null
      ? "System default"
      : (currentDevice?.description ?? micConfig.input_device);

  return (
    <div className="content channel-page mic-content strip-accent-mic">
      <div className="screen-head channel-screen-head mic-screen-head">
        <div className="channel-heading-icon">
          <Ms name="mic" />
        </div>
        <h1>Microphone</h1>
        {multipleMics && (
          <div className="mic-channel-picker">
            <div style={{ position: "relative" }}>
              <button type="button" className="select mic-channel-select" onClick={() => setChannelOpen((open) => !open)}>
                <span>{micConfig.output_label}</span>
                <Ms name="expand_more" />
              </button>
              <Popover open={channelOpen} onClose={() => setChannelOpen(false)} side="bottom" align="start">
                {micConfigs.map((mic, index) => (
                  <MenuItem
                    key={mic.node_name}
                    selected={mic.node_name === micNode}
                    showCheck
                    onClick={() => {
                      selectMic(mic.node_name);
                      setChannelOpen(false);
                    }}
                  >
                    {mic.output_label}{index === 0 ? " · Primary" : ""}
                  </MenuItem>
                ))}
              </Popover>
            </div>
            <button type="button" className="select mic-channel-action" onClick={() => setAddingMic(true)} title="Add microphone channel">
              <Ms name="add" />
            </button>
            {micNode !== MIC_LEVEL_KEY && (
              <button type="button" className="select mic-channel-action" onClick={() => setDeletingMic(true)} title="Delete microphone channel">
                <Ms name="delete" />
              </button>
            )}
          </div>
        )}
        <div className="screen-head-actions">
          <div
            className="mic-head-enable"
            title={`Enable the processed microphone for profile “${activeProfile ?? "Current profile"}”`}
          >
            <span>Processed mic</span>
            <Toggle
              on={micConfig.enabled}
              onClick={() => {
                if (micConfig.enabled && listening) void toggleMonitor(micNode);
                setDeviceOpen(false);
                void updateMic({ enabled: !micConfig.enabled });
              }}
            />
          </div>
          <div className={`mic-live-actions${micConfig.enabled ? "" : " disabled"}`}>
            <button
              type="button"
              className={"select channel-head-control" + (micConfig.muted ? " channel-action-danger" : "")}
              disabled={!micConfig.enabled}
              onClick={() => void updateMic({ muted: !micConfig.muted })}
            >
              <Ms name={micConfig.muted ? "mic_off" : "mic"} />
              {micConfig.muted ? "Muted" : "Mute"}
            </button>
            <button
              type="button"
              className={"select channel-head-control" + (listening ? " on-mon" : "")}
              disabled={!micConfig.enabled}
              aria-pressed={listening}
              title="Listen to yourself - hear the processed mic to tune the chain (wear headphones)"
              onClick={() => void toggleMonitor(micNode)}
            >
              <Ms name="headphones" />
              {listening ? "Listening" : "Listen"}
            </button>
            <AudioTestControls kind="mic" nodeName={micNode} />
          </div>
        </div>
      </div>
      <div className="screen-scroll channel-scroll">
        <div className={micConfig.enabled ? undefined : "mic-disabled"}>
            <div className="section-label">Input</div>
            <div className="card channel-controls-card mic-controls-card">
              <div className="channel-control-block channel-preset-control">
                <div>
                  <div className="rtitle">Preset</div>
                </div>
                <MicPresetMenu
                  config={micConfig}
                  onApply={(patch) => void updateMic(patch)}
                />
              </div>
              <div className="channel-control-block mic-summary-gain-control">
                <div>
                  <div className="rtitle">Gain</div>
                </div>
                <div className="mic-summary-gain">
                  <HSlider
                    value={micConfig.gain_percent}
                    max={MAX_MIC_GAIN}
                    ariaLabel={`${micConfig.output_label} gain`}
                    onChange={(v) => void updateMic({ gain_percent: v })}
                  />
                </div>
              </div>
              <div className="channel-control-block mic-input-control">
                <div>
                  <div className="rtitle">Device</div>
                </div>
                <div className="mic-input-picker">
                  <button type="button" className="select mic-device-select" onClick={() => setDeviceOpen((o) => !o)}>
                    <Ms name="settings_voice" />
                    <span className="mic-device-name">{deviceLabel}</span>
                    <Ms name="expand_more" />
                  </button>
                  <Popover
                    open={deviceOpen}
                    onClose={() => setDeviceOpen(false)}
                    side="bottom"
                    align="start"
                  >
                    <MenuItem
                      icon="mic"
                      selected={micConfig.input_device === null}
                      onClick={() => {
                        void updateMic({ input_device: null });
                        setDeviceOpen(false);
                      }}
                    >
                      System default
                    </MenuItem>
                    {inputDevices.map((d) => (
                      <MenuItem
                        key={d.name}
                        icon="mic"
                        selected={d.name === micConfig.input_device}
                        onClick={() => {
                          void updateMic({ input_device: d.name });
                          setDeviceOpen(false);
                        }}
                      >
                        {d.description}
                      </MenuItem>
                    ))}
                  </Popover>
                </div>
              </div>
              <div className="channel-control-block mic-name-control">
                <div>
                  <div className="rtitle">Name</div>
                </div>
                <input
                  className="menu-input"
                  value={micConfig.output_label}
                  maxLength={32}
                  title="How other apps list your processed mic"
                  onChange={(e) => void updateMic({ output_label: e.target.value })}
                />
              </div>
            </div>

            <div className="card channel-eq-card mic-eq-card">
              <MicEqEditor
                config={micConfig}
                onApply={(patch) => void updateMic(patch)}
              />
            </div>

            <div className="section-label">Processing</div>
            <div className="mic-processing-grid">
              <div className="card mic-processing-card">
                <div className="processing-card-head">
                  <div className="processing-toggle-title">
                    <Toggle
                      on={micConfig.gate_enabled}
                      onClick={() => void updateMic({ gate_enabled: !micConfig.gate_enabled })}
                    />
                    <div className="rtitle">Noise gate</div>
                  </div>
                  <div className="processing-card-head-actions">
                    <ProcessingInfo
                      label="Noise gate"
                      text={'Cuts the microphone noise floor between words.\n\nRaise the threshold to reject more background sound.'}
                    />
                  </div>
                </div>
                <DspSlider
                  label="Threshold"
                  min={-80}
                  max={-10}
                  step={1}
                  unit=" dB"
                  value={micConfig.gate_threshold_db}
                  defaultValue={MIC_DSP_DEFAULTS.gate_threshold_db}
                  disabled={!micConfig.gate_enabled}
                  onChange={(v) => void updateMic({ gate_threshold_db: v })}
                />
              </div>

              <div className="card mic-processing-card">
                <div className="processing-card-head">
                  <div className="processing-toggle-title">
                    <Toggle
                      on={micConfig.comp_enabled}
                      onClick={() => void updateMic({ comp_enabled: !micConfig.comp_enabled })}
                    />
                    <div className="rtitle">Compressor</div>
                  </div>
                  <div className="processing-card-head-actions">
                    <ProcessingInfo
                      label="Compressor"
                      text={'Evens out loud peaks and quiet speech.\n\nThreshold chooses when compression starts. Ratio controls its strength.'}
                    />
                  </div>
                </div>
                <DspSlider
                  label="Threshold"
                  min={-60}
                  max={0}
                  step={1}
                  unit=" dB"
                  value={micConfig.comp_threshold_db}
                  defaultValue={MIC_DSP_DEFAULTS.comp_threshold_db}
                  disabled={!micConfig.comp_enabled}
                  onChange={(v) => void updateMic({ comp_threshold_db: v })}
                />
                <DspSlider
                  label="Ratio"
                  min={1}
                  max={10}
                  step={0.5}
                  unit=":1"
                  value={micConfig.comp_ratio}
                  defaultValue={MIC_DSP_DEFAULTS.comp_ratio}
                  disabled={!micConfig.comp_enabled}
                  onChange={(v) => void updateMic({ comp_ratio: v })}
                />
              </div>

              <div className="card mic-processing-card">
                <div className="processing-card-head">
                  <div className="processing-toggle-title">
                    <Toggle
                      on={micConfig.limiter_enabled}
                      onClick={() => void updateMic({ limiter_enabled: !micConfig.limiter_enabled })}
                    />
                    <div className="rtitle">Volume limiter</div>
                  </div>
                  <div className="processing-card-head-actions">
                    <ProcessingInfo
                      label="Volume limiter"
                      text="Applies a hard final ceiling so the processed microphone cannot clip downstream."
                    />
                  </div>
                </div>
                <DspSlider
                  label="Ceiling"
                  min={-12}
                  max={0}
                  step={0.5}
                  unit=" dB"
                  value={micConfig.limiter_ceiling_db}
                  defaultValue={MIC_DSP_DEFAULTS.limiter_ceiling_db}
                  disabled={!micConfig.limiter_enabled}
                  onChange={(v) => void updateMic({ limiter_ceiling_db: v })}
                />
              </div>
            </div>

        </div>
      </div>

      <Modal
        open={addingMic}
        onClose={() => {
          setAddingMic(false);
          setNewDeviceOpen(false);
          setCopyOpen(false);
        }}
        title="New microphone channel"
      >
        <p className="modal-text">
          Create another independently processed virtual microphone. One channel is recommended unless your workflow specifically needs separate inputs.
        </p>
        <label className="modal-label" htmlFor="new-mic-name">Virtual microphone name</label>
        <input
          id="new-mic-name"
          className="menu-input"
          value={newMicName}
          maxLength={32}
          autoFocus
          placeholder="e.g. Harmonics"
          onChange={(event) => setNewMicName(event.target.value)}
        />
        <label className="modal-label" htmlFor="new-mic-device">Input device</label>
        <div className="mic-create-picker">
          <button type="button" id="new-mic-device" className="select mic-create-select" onClick={() => setNewDeviceOpen((open) => !open)}>
            <span>{newMicDevice === null ? "System default" : inputDevices.find((device) => device.name === newMicDevice)?.description ?? newMicDevice}</span>
            <Ms name="expand_more" />
          </button>
          <Popover open={newDeviceOpen} onClose={() => setNewDeviceOpen(false)} side="bottom" align="start">
            <MenuItem selected={newMicDevice === null} showCheck onClick={() => { setNewMicDevice(null); setNewDeviceOpen(false); }}>System default</MenuItem>
            {inputDevices.map((device) => (
              <MenuItem key={device.name} selected={newMicDevice === device.name} showCheck onClick={() => { setNewMicDevice(device.name); setNewDeviceOpen(false); }}>
                {device.description}
              </MenuItem>
            ))}
          </Popover>
        </div>
        <label className="modal-label" htmlFor="new-mic-copy">Processing</label>
        <div className="mic-create-picker">
          <button type="button" id="new-mic-copy" className="select mic-create-select" onClick={() => setCopyOpen((open) => !open)}>
            <span>{copyFrom === null ? "Fresh setup" : `Copy ${micConfigs.find((mic) => mic.node_name === copyFrom)?.output_label ?? "microphone"}`}</span>
            <Ms name="expand_more" />
          </button>
          <Popover open={copyOpen} onClose={() => setCopyOpen(false)} side="bottom" align="start">
            <MenuItem selected={copyFrom === null} showCheck onClick={() => { setCopyFrom(null); setCopyOpen(false); }}>Fresh setup</MenuItem>
            {micConfigs.map((mic) => (
              <MenuItem key={mic.node_name} selected={copyFrom === mic.node_name} showCheck onClick={() => { setCopyFrom(mic.node_name); setCopyOpen(false); }}>
                Copy {mic.output_label}
              </MenuItem>
            ))}
          </Popover>
        </div>
        <div className="modal-btns">
          <button
            type="button"
            className="modal-btn primary"
            disabled={!newMicName.trim()}
            onClick={() => {
              void addMicChannel(newMicName, newMicDevice, copyFrom);
              setAddingMic(false);
              setNewDeviceOpen(false);
              setCopyOpen(false);
              setNewMicName("");
              setCopyFrom(null);
            }}
          >
            Create microphone
          </button>
          <button type="button" className="modal-btn" onClick={() => { setAddingMic(false); setNewDeviceOpen(false); setCopyOpen(false); }}>Cancel</button>
        </div>
      </Modal>

      <ConfirmModal
        open={deletingMic}
        onClose={() => setDeletingMic(false)}
        title={`Delete ${micConfig.output_label}?`}
        confirmLabel="Delete microphone"
        onConfirm={() => void removeMicChannel(micNode)}
      >
        This removes its virtual input and profile-specific processing. Applications using it will need another microphone selected. This cannot be undone.
      </ConfirmModal>
    </div>
  );
}
