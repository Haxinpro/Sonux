import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import type { EqBand, MicConfig } from "../../types";
import { DEFAULT_EQ_BANDS, MIC_DSP_DEFAULTS } from "../../types";
import { useMixerStore } from "../../store/mixer";
import { Ms } from "../Icons";
import { Popover } from "../Popover";
import { useI18n } from "../../i18n";

interface MicPreset {
  schema: number;
  name: string;
  eq_enabled: boolean;
  eq_preamp_db: number;
  eq_bands: EqBand[];
  gate_enabled: boolean;
  gate_threshold_db: number;
  comp_enabled: boolean;
  comp_threshold_db: number;
  comp_ratio: number;
  limiter_enabled: boolean;
  limiter_ceiling_db: number;
}

const BALANCED: MicPreset = {
  schema: 1,
  name: "Balanced",
  eq_enabled: false,
  eq_preamp_db: 0,
  eq_bands: DEFAULT_EQ_BANDS.map((band) => ({ ...band })),
  gate_enabled: true,
  gate_threshold_db: MIC_DSP_DEFAULTS.gate_threshold_db,
  comp_enabled: true,
  comp_threshold_db: MIC_DSP_DEFAULTS.comp_threshold_db,
  comp_ratio: MIC_DSP_DEFAULTS.comp_ratio,
  limiter_enabled: true,
  limiter_ceiling_db: MIC_DSP_DEFAULTS.limiter_ceiling_db,
};

function matches(config: MicConfig, preset: MicPreset): boolean {
  return (
    config.eq_enabled === preset.eq_enabled &&
    config.eq_preamp_db === preset.eq_preamp_db &&
    JSON.stringify(config.eq_bands) === JSON.stringify(preset.eq_bands) &&
    config.gate_enabled === preset.gate_enabled &&
    config.gate_threshold_db === preset.gate_threshold_db &&
    config.comp_enabled === preset.comp_enabled &&
    config.comp_threshold_db === preset.comp_threshold_db &&
    config.comp_ratio === preset.comp_ratio &&
    config.limiter_enabled === preset.limiter_enabled &&
    config.limiter_ceiling_db === preset.limiter_ceiling_db
  );
}

export function MicPresetMenu({
  config,
  onApply,
  compact = false,
}: Readonly<{
  config: MicConfig;
  onApply: (patch: Partial<MicConfig>) => void;
  compact?: boolean;
}>) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const [presets, setPresets] = useState<MicPreset[]>([]);
  const [saveName, setSaveName] = useState("");
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);

  const refresh = () => {
    invoke<MicPreset[]>("list_mic_presets")
      .then(setPresets)
      .catch((error) => setMixerError(String(error)));
  };

  useEffect(() => {
    refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const all = [BALANCED, ...presets];
  const active = all.find((preset) => matches(config, preset));
  const apply = (preset: MicPreset) => {
    onApply({
      eq_enabled: preset.eq_enabled,
      eq_preamp_db: preset.eq_preamp_db,
      eq_bands: preset.eq_bands.map((band) => ({ ...band })),
      gate_enabled: preset.gate_enabled,
      gate_threshold_db: preset.gate_threshold_db,
      comp_enabled: preset.comp_enabled,
      comp_threshold_db: preset.comp_threshold_db,
      comp_ratio: preset.comp_ratio,
      limiter_enabled: preset.limiter_enabled,
      limiter_ceiling_db: preset.limiter_ceiling_db,
    });
    setOpen(false);
  };
  const save = async () => {
    const name = saveName.trim();
    if (!name) return;
    try {
      await invoke("save_mic_preset", { name, config });
      setSaveName("");
      refresh();
    } catch (error) {
      setMixerError(String(error));
    }
  };
  const remove = async (name: string) => {
    try {
      await invoke("delete_mic_preset", { name });
      setConfirmDelete(null);
      refresh();
    } catch (error) {
      setMixerError(String(error));
    }
  };

  return (
    <div className="strip-preset-anchor">
      <button
        type="button"
        className={compact ? "strip-preset-button" : "select"}
        onClick={() => setOpen((value) => !value)}
        title={t("microphone.presets.hint")}
      >
        <Ms name="graphic_eq" />
        <span>{active === BALANCED ? t("microphone.presets.balanced") : active?.name ?? t("microphone.presets.custom")}</span>
        <Ms name="expand_more" />
      </button>
      <Popover
        open={open}
        onClose={() => {
          setOpen(false);
          setConfirmDelete(null);
        }}
        side="bottom"
        align={compact ? "center" : "end"}
        style={{ minWidth: 250 }}
      >
        <div className="eqm-preset-head">{t("microphone.presets.title")}</div>
        {all.map((preset, index) => (
          <div key={`${index === 0 ? "bundled" : "user"}:${preset.name}`} className="eqm-preset-row">
            <button
              type="button"
              className={"menu-item eqm-preset-apply" + (preset === active ? " sel" : "")}
              onClick={() => apply(preset)}
            >
              <Ms name="graphic_eq" />
              <span className="eqm-preset-name">{index === 0 ? t("microphone.presets.balanced") : preset.name}</span>
            </button>
            {index > 0 && (
              confirmDelete === preset.name ? (
                <span className="eqm-preset-confirm">
                  <button type="button" className="eqm-remove danger" title={t("microphone.presets.deleteHint")} onClick={() => void remove(preset.name)}>
                    <Ms name="check" />
                  </button>
                  <button type="button" className="eqm-remove" title={t("microphone.presets.keep")} onClick={() => setConfirmDelete(null)}>
                    <Ms name="close" />
                  </button>
                </span>
              ) : (
                <button type="button" className="eqm-remove" title={t("microphone.presets.delete")} onClick={() => setConfirmDelete(preset.name)}>
                  <Ms name="close" />
                </button>
              )
            )}
          </div>
        ))}
        <div className="menu-sep" />
        <div className="eqm-save-label">{t("microphone.presets.save")}</div>
        <div className="eqm-save-row">
          <input
            className="menu-input"
            placeholder={t("microphone.presets.namePlaceholder")}
            value={saveName}
            maxLength={64}
            onChange={(event) => setSaveName(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") void save();
            }}
          />
          <button type="button" className="select" disabled={!saveName.trim()} onClick={() => void save()}>
            <Ms name="save" />
          </button>
        </div>
      </Popover>
    </div>
  );
}

function setMixerError(message: string) {
  useMixerStore.setState({ error: message });
}
