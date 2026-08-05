import { invoke } from "@tauri-apps/api/core";
import { useMixerStore } from "../../store/mixer";
import type { EqConfig, VirtualSink } from "../../types";
import { defaultEqConfig } from "../../types";
import { DspSlider } from "../Mic/DspSlider";
import { Toggle, ToggleRow } from "../Toggle";
import { Ms } from "../Icons";
import { ProcessingInfo } from "../ProcessingInfo";

function ModeButton({
  mode,
  current,
  icon,
  title,
  onSelect,
}: Readonly<{
  mode: EqConfig["playback_mode"];
  current: EqConfig["playback_mode"];
  icon: string;
  title: string;
  onSelect: (mode: EqConfig["playback_mode"]) => void;
}>) {
  return (
    <button
      type="button"
      className={"channel-mode-button" + (mode === current ? " active" : "")}
      aria-pressed={mode === current}
      onClick={() => onSelect(mode)}
    >
      <Ms name={icon} />
      {title}
    </button>
  );
}

export function ChannelProcessing({ channel }: Readonly<{ channel: VirtualSink }>) {
  const config =
    useMixerStore((s) => s.eqConfigs[channel.name] ?? null) ?? defaultEqConfig();
  const setChannelEq = useMixerStore((s) => s.setChannelEq);
  const isVoice = channel.name === "sink_chat";
  const supportsSpatial = channel.name === "sink_game" || channel.name === "sink_media";
  const apply = (patch: Partial<EqConfig>) =>
    void setChannelEq(channel.name, { ...config, ...patch });

  return (
    <div className={"channel-processing-grid" + (supportsSpatial ? " has-spatial" : "")}>
      {supportsSpatial ? (
        <div className="card channel-processing-card spatial-processing-card">
          <div className="spatial-head">
            <div className="processing-toggle-title">
              <Toggle
                on={config.spatial_enabled}
                onClick={() => apply({ spatial_enabled: !config.spatial_enabled })}
              />
              <div className="rtitle">Spatial audio</div>
            </div>
            <div className="processing-card-head-actions">
              <div className="channel-mode-switch" role="group" aria-label="Output mode">
                <ModeButton
                  mode="headphones"
                  current={config.playback_mode}
                  icon="headphones"
                  title="Headphones"
                  onSelect={(playback_mode) => apply({ playback_mode })}
                />
                <ModeButton
                  mode="speakers"
                  current={config.playback_mode}
                  icon="speaker"
                  title="Speakers"
                  onSelect={(playback_mode) => apply({ playback_mode })}
                />
              </div>
              <ProcessingInfo
                label="Spatial audio"
                text={'Headphones use the Aalto University near-field SOFA HRTF to render 7.1 audio.\n\nPerformance emphasizes directional clarity. Immersion adds diffuse room energy.\n\nDistance moves from Close at 0, through neutral at 50, to Far at 100. Its level ranges from +2.5 dB to -2.5 dB.\n\nSelect surround output in the game and disable the game\'s own HRTF or DTS processing. LFE retains a direct bass path.\n\nSpeakers use a 7.1-to-stereo fold-down instead.'}
              />
            </div>
          </div>

          <div
            className={
              "spatial-body" +
              (!config.spatial_enabled ? " inactive" : "")
            }
          >
            <div className="spatial-stage" aria-label="7.1 speaker test">
              <div className="spatial-listener">
                <Ms name={config.playback_mode === "headphones" ? "headphones" : "speaker"} />
              </div>
              {[
                ["FL", "Front left", "fl"],
                ["FC", "Front centre", "fc"],
                ["FR", "Front right", "fr"],
                ["SL", "Side left", "sl"],
                ["SR", "Side right", "sr"],
                ["RL", "Rear left", "rl"],
                ["LFE", "Subwoofer", "lfe"],
                ["RR", "Rear right", "rr"],
              ].map(([id, title, position]) => (
                <button
                  key={id}
                  type="button"
                  className={`spatial-speaker ${position}`}
                  title={`Test ${title}`}
                  aria-label={`Test ${title}`}
                  disabled={!config.spatial_enabled}
                  onClick={() => void invoke("test_spatial_channel", { sinkName: channel.name, channel: id })}
                >
                  <Ms name={id === "LFE" ? "surround_sound" : "volume_up"} />
                  <span>{id}</span>
                </button>
              ))}
            </div>

            <div className="spatial-tuning">
              <div className="rtitle spatial-tuning-title">Tuning</div>
              <DspSlider
                label="Performance"
                endLabel="Immersion"
                min={0}
                max={100}
                step={1}
                value={Math.round(config.spatial_tuning * 100)}
                defaultValue={50}
                unit=""
                inlineEndLabel
                disabled={!config.spatial_enabled || config.playback_mode !== "headphones"}
                onChange={(value) => apply({ spatial_tuning: value / 100 })}
              />
              <DspSlider
                label="Distance"
                min={0}
                max={100}
                step={1}
                value={Math.round(config.spatial_distance * 100)}
                defaultValue={50}
                unit=""
                disabled={!config.spatial_enabled || config.playback_mode !== "headphones"}
                onChange={(value) => apply({ spatial_distance: value / 100 })}
              />
            </div>
          </div>
        </div>
      ) : (
      <div className="card channel-processing-card channel-output-card">
        <div className="processing-card-head">
          <div className="rtitle">Output mode</div>
          <div className="processing-card-head-actions">
            <div className="channel-mode-switch" role="group" aria-label="Output mode">
              <ModeButton
                mode="headphones"
                current={config.playback_mode}
                icon="headphones"
                title="Headphones"
                onSelect={(playback_mode) => apply({ playback_mode })}
              />
              <ModeButton
                mode="speakers"
                current={config.playback_mode}
                icon="speaker"
                title="Speakers"
                onSelect={(playback_mode) => apply({ playback_mode })}
              />
            </div>
            <ProcessingInfo
              label="Output mode"
              text={'Headphones add gentle low-frequency crossfeed for hard-panned stereo audio.\n\nSpeakers keep direct stereo.\n\nThis stereo option is not HRTF surround.'}
            />
          </div>
        </div>
      </div>
      )}

      <div className="card channel-processing-card">
        <div className="processing-card-head">
          {isVoice ? (
            <div className="rtitle">Voice dynamics</div>
          ) : (
            <div className="processing-toggle-title">
              <Toggle
                on={config.comp_enabled}
                onClick={() => apply({ comp_enabled: !config.comp_enabled })}
              />
              <div className="rtitle">Smart volume & boost</div>
            </div>
          )}
          <div className="processing-card-head-actions">
            <ProcessingInfo
              label={isVoice ? "Voice dynamics" : "Smart volume and boost"}
              text={isVoice
                ? "Noise gate silences signals below its threshold.\n\nCompressor reduces loud peaks while keeping speech present.\n\nVolume boost adjusts the final channel level."
                : "Smart volume balances quiet and loud audio automatically.\n\nVolume boost adjusts the final channel level."}
            />
          </div>
        </div>

        {isVoice && (
          <>
            <ToggleRow
              icon="noise_control_off"
              title="Noise gate"
              on={config.gate_enabled}
              onToggle={() => apply({ gate_enabled: !config.gate_enabled })}
            />
            <DspSlider
              label="Threshold"
              min={-80}
              max={-10}
              step={1}
              unit=" dB"
              value={config.gate_threshold_db}
              defaultValue={-48}
              disabled={!config.gate_enabled}
              onChange={(gate_threshold_db) => apply({ gate_threshold_db })}
            />
          </>
        )}

        {isVoice && (
          <ToggleRow
            icon="compress"
            title="Compressor"
            on={config.comp_enabled}
            onToggle={() => apply({ comp_enabled: !config.comp_enabled })}
          />
        )}
        {isVoice ? (
          <>
            <DspSlider
              label="Threshold"
              min={-60}
              max={0}
              step={1}
              unit=" dB"
              value={config.comp_threshold_db}
              defaultValue={-18}
              disabled={!config.comp_enabled}
              onChange={(comp_threshold_db) => apply({ comp_threshold_db })}
            />
            <DspSlider
              label="Strength"
              min={1}
              max={10}
              step={0.5}
              unit=":1"
              value={config.comp_ratio}
              defaultValue={3}
              disabled={!config.comp_enabled}
              onChange={(comp_ratio) => apply({ comp_ratio })}
            />
          </>
        ) : (
          <DspSlider
            label="Level"
            min={1}
            max={10}
            step={0.5}
            unit=""
            value={config.comp_ratio}
            defaultValue={3}
            disabled={!config.comp_enabled}
            onChange={(comp_ratio) => apply({ comp_ratio })}
          />
        )}

        <DspSlider
          label="Volume boost"
          min={-12}
          max={12}
          step={0.5}
          unit=" dB"
          value={config.boost_db}
          defaultValue={0}
          onChange={(boost_db) => apply({ boost_db })}
        />
      </div>

      <div className="card channel-processing-card channel-limiter-card">
        <div className="processing-card-head">
          <div className="processing-toggle-title">
            <Toggle
              on={config.limiter_enabled}
              onClick={() => apply({ limiter_enabled: !config.limiter_enabled })}
            />
            <div className="rtitle">Volume limiter</div>
          </div>
          <div className="processing-card-head-actions">
            <ProcessingInfo
              label="Volume limiter"
              text="Prevents volume boost and dynamics processing from exceeding the selected ceiling and clipping."
            />
          </div>
        </div>
        <DspSlider
          label="Ceiling"
          min={-12}
          max={0}
          step={0.5}
          unit=" dB"
          value={config.limiter_ceiling_db}
          defaultValue={-1}
          disabled={!config.limiter_enabled}
          onChange={(limiter_ceiling_db) => apply({ limiter_ceiling_db })}
        />
      </div>
    </div>
  );
}
