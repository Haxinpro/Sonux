import { invoke } from "@tauri-apps/api/core";
import { useMixerStore } from "../../store/mixer";
import type { EqConfig, VirtualSink } from "../../types";
import { defaultEqConfig } from "../../types";
import { DspSlider } from "../Mic/DspSlider";
import { Toggle, ToggleRow } from "../Toggle";
import { Ms } from "../Icons";
import { ProcessingInfo } from "../ProcessingInfo";
import { useI18n, type TranslationKey } from "../../i18n";

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
  const { t } = useI18n();
  const config =
    useMixerStore((s) => s.eqConfigs[channel.name] ?? null) ?? defaultEqConfig();
  const setChannelEq = useMixerStore((s) => s.setChannelEq);
  const isVoice = channel.name === "sink_chat";
  const supportsSpatial = channel.name === "sink_game"
    || channel.name === "sink_media"
    || channel.name.startsWith("sink_spatial_");
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
              <div className="rtitle">{t("processing.spatial.title")}</div>
            </div>
            <div className="processing-card-head-actions">
              <div className="channel-mode-switch" role="group" aria-label={t("processing.outputMode")}>
                <ModeButton
                  mode="headphones"
                  current={config.playback_mode}
                  icon="headphones"
                  title={t("processing.headphones")}
                  onSelect={(playback_mode) => apply({ playback_mode })}
                />
                <ModeButton
                  mode="speakers"
                  current={config.playback_mode}
                  icon="speaker"
                  title={t("processing.speakers")}
                  onSelect={(playback_mode) => apply({ playback_mode })}
                />
              </div>
              <ProcessingInfo
                label={t("processing.spatial.title")}
                text={t("processing.spatial.info")}
              />
            </div>
          </div>

          <div
            className={
              "spatial-body" +
              (!config.spatial_enabled ? " inactive" : "")
            }
          >
            <div className="spatial-stage" aria-label={t("processing.spatial.stageLabel")}>
              <div className="spatial-listener">
                <Ms name={config.playback_mode === "headphones" ? "headphones" : "speaker"} />
              </div>
              {[
                ["FL", "processing.spatial.frontLeft", "fl"],
                ["FC", "processing.spatial.frontCentre", "fc"],
                ["FR", "processing.spatial.frontRight", "fr"],
                ["SL", "processing.spatial.sideLeft", "sl"],
                ["SR", "processing.spatial.sideRight", "sr"],
                ["RL", "processing.spatial.rearLeft", "rl"],
                ["LFE", "processing.spatial.subwoofer", "lfe"],
                ["RR", "processing.spatial.rearRight", "rr"],
              ].map(([id, title, position]) => (
                <button
                  key={id}
                  type="button"
                  className={`spatial-speaker ${position}`}
                  title={t("processing.spatial.test", { speaker: t(title as TranslationKey) })}
                  aria-label={t("processing.spatial.test", { speaker: t(title as TranslationKey) })}
                  disabled={!config.spatial_enabled}
                  onClick={() => void invoke("test_spatial_channel", { sinkName: channel.name, channel: id })}
                >
                  <Ms name={id === "LFE" ? "surround_sound" : "volume_up"} />
                  <span>{id}</span>
                </button>
              ))}
            </div>

            <div className="spatial-tuning">
              <div className="rtitle spatial-tuning-title">{t("processing.spatial.tuning")}</div>
              <DspSlider
                label={t("processing.spatial.performance")}
                endLabel={t("processing.spatial.immersion")}
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
                label={t("processing.spatial.distance")}
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
          <div className="rtitle">{t("processing.outputMode")}</div>
          <div className="processing-card-head-actions">
            <div className="channel-mode-switch" role="group" aria-label={t("processing.outputMode")}>
              <ModeButton
                mode="headphones"
                current={config.playback_mode}
                icon="headphones"
                title={t("processing.headphones")}
                onSelect={(playback_mode) => apply({ playback_mode })}
              />
              <ModeButton
                mode="speakers"
                current={config.playback_mode}
                icon="speaker"
                title={t("processing.speakers")}
                onSelect={(playback_mode) => apply({ playback_mode })}
              />
            </div>
            <ProcessingInfo
              label={t("processing.outputMode")}
              text={t("processing.outputInfo")}
            />
          </div>
        </div>
      </div>
      )}

      <div className="card channel-processing-card">
        <div className="processing-card-head">
          {isVoice ? (
            <div className="rtitle">{t("processing.voice.title")}</div>
          ) : (
            <div className="processing-toggle-title">
              <Toggle
                on={config.comp_enabled}
                onClick={() => apply({ comp_enabled: !config.comp_enabled })}
              />
              <div className="rtitle">{t("processing.smartVolume.title")}</div>
            </div>
          )}
          <div className="processing-card-head-actions">
            <ProcessingInfo
              label={t(isVoice ? "processing.voice.title" : "processing.smartVolume.infoLabel")}
              text={t(isVoice ? "processing.voice.info" : "processing.smartVolume.info")}
            />
          </div>
        </div>

        {isVoice && (
          <>
            <ToggleRow
              icon="noise_control_off"
              title={t("processing.noiseGate")}
              on={config.gate_enabled}
              onToggle={() => apply({ gate_enabled: !config.gate_enabled })}
            />
            <DspSlider
              label={t("processing.threshold")}
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
            title={t("processing.compressor")}
            on={config.comp_enabled}
            onToggle={() => apply({ comp_enabled: !config.comp_enabled })}
          />
        )}
        {isVoice ? (
          <>
            <DspSlider
              label={t("processing.threshold")}
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
              label={t("processing.strength")}
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
            label={t("processing.level")}
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
          label={t("processing.volumeBoost")}
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
            <div className="rtitle">{t("processing.limiter")}</div>
          </div>
          <div className="processing-card-head-actions">
            <ProcessingInfo
              label={t("processing.limiter")}
              text={t("processing.limiter.info")}
            />
          </div>
        </div>
        <DspSlider
          label={t("processing.ceiling")}
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
