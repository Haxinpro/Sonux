import { useState } from "react";
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

export function MicScreen() {
  const micConfig = useMixerStore((s) => s.micConfig);
  const inputDevices = useMixerStore((s) => s.inputDevices);
  const setMicConfig = useMixerStore((s) => s.setMicConfig);
  const listening = useMixerStore((s) => s.monitors[MIC_LEVEL_KEY] ?? false);
  const toggleMonitor = useMixerStore((s) => s.toggleMonitor);
  const [deviceOpen, setDeviceOpen] = useState(false);
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
        <div className="screen-head-actions">
          <button
            type="button"
            className={"select channel-head-control" + (micConfig.muted ? " channel-action-danger" : "")}
            onClick={() => void setMicConfig({ muted: !micConfig.muted })}
          >
            <Ms name={micConfig.muted ? "mic_off" : "mic"} />
            {micConfig.muted ? "Muted" : "Mute"}
          </button>
          <button
            type="button"
            className={"select channel-head-control" + (listening ? " on-mon" : "")}
            aria-pressed={listening}
            title="Listen to yourself - hear the processed mic to tune the chain (wear headphones)"
            onClick={() => void toggleMonitor(MIC_LEVEL_KEY)}
          >
            <Ms name="headphones" />
            {listening ? "Listening" : "Listen"}
          </button>
          <AudioTestControls kind="mic" />
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
                  onApply={(patch) => void setMicConfig(patch)}
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
                    onChange={(v) => void setMicConfig({ gain_percent: v })}
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
                        void setMicConfig({ input_device: null });
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
                          void setMicConfig({ input_device: d.name });
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
                  onChange={(e) => void setMicConfig({ output_label: e.target.value })}
                />
              </div>
            </div>

            <div className="card channel-eq-card mic-eq-card">
              <MicEqEditor
                config={micConfig}
                onApply={(patch) => void setMicConfig(patch)}
              />
            </div>

            <div className="section-label">Processing</div>
            <div className="mic-processing-grid">
              <div className="card mic-processing-card">
                <div className="processing-card-head">
                  <div className="processing-toggle-title">
                    <Toggle
                      on={micConfig.gate_enabled}
                      onClick={() => void setMicConfig({ gate_enabled: !micConfig.gate_enabled })}
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
                  onChange={(v) => void setMicConfig({ gate_threshold_db: v })}
                />
              </div>

              <div className="card mic-processing-card">
                <div className="processing-card-head">
                  <div className="processing-toggle-title">
                    <Toggle
                      on={micConfig.comp_enabled}
                      onClick={() => void setMicConfig({ comp_enabled: !micConfig.comp_enabled })}
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
                  onChange={(v) => void setMicConfig({ comp_threshold_db: v })}
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
                  onChange={(v) => void setMicConfig({ comp_ratio: v })}
                />
              </div>

              <div className="card mic-processing-card">
                <div className="processing-card-head">
                  <div className="processing-toggle-title">
                    <Toggle
                      on={micConfig.limiter_enabled}
                      onClick={() => void setMicConfig({ limiter_enabled: !micConfig.limiter_enabled })}
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
                  onChange={(v) => void setMicConfig({ limiter_ceiling_db: v })}
                />
              </div>
            </div>

        </div>
      </div>
    </div>
  );
}
