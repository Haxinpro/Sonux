import { useState } from "react";
import type { EqBand, EqConfig, MicConfig } from "../../types";
import { defaultEqConfig, MAX_EQ_BANDS } from "../../types";
import { EqBandRow } from "../Eq/EqBandRow";
import { bandColor, EqCurve } from "../Eq/EqCurve";
import { Ms } from "../Icons";
import { Toggle } from "../Toggle";
import { ProcessingInfo } from "../ProcessingInfo";
import { DspSlider } from "./DspSlider";
import { useI18n } from "../../i18n";

export function MicEqEditor({
  config,
  onApply,
}: Readonly<{
  config: MicConfig;
  onApply: (patch: Partial<MicConfig>) => void;
}>) {
  const { t } = useI18n();
  const [selected, setSelected] = useState(0);
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const eq: EqConfig = {
    ...defaultEqConfig(),
    enabled: config.eq_enabled,
    preamp_db: config.eq_preamp_db,
    bands: config.eq_bands,
  };

  const apply = (next: EqConfig) =>
    onApply({
      eq_enabled: next.enabled,
      eq_preamp_db: next.preamp_db,
      eq_bands: next.bands,
    });
  const patchBand = (index: number, patch: Partial<EqBand>) =>
    apply({
      ...eq,
      bands: eq.bands.map((band, i) => (i === index ? { ...band, ...patch } : band)),
    });
  const addBandAt = (freq_hz: number, gain_db: number) => {
    if (eq.bands.length >= MAX_EQ_BANDS) return;
    const bands = [...eq.bands, { kind: "peaking" as const, freq_hz, gain_db, q: 1 }];
    apply({ ...eq, bands });
    setSelected(bands.length - 1);
  };
  const addBand = () => addBandAt(1000, 0);
  const removeBand = (index: number) => {
    if (eq.bands.length <= 1) return;
    const bands = eq.bands.filter((_, i) => i !== index);
    apply({ ...eq, bands });
    setSelected(Math.min(selected, bands.length - 1));
  };
  const reset = () => {
    const flat = defaultEqConfig();
    apply({ ...eq, preamp_db: 0, bands: flat.bands });
    setSelected(0);
  };

  return (
    <div className="eqm-editor">
      <div className="eqm-head">
        <div className="processing-toggle-title">
          <Toggle on={eq.enabled} onClick={() => apply({ ...eq, enabled: !eq.enabled })} />
          <div className="rtitle">{t("equalizer.title")}</div>
        </div>
        <div className="eqm-head-actions">
          <button
            type="button"
            className="select eqm-iconbtn"
            onClick={reset}
            title={t("equalizer.resetMicrophoneHint")}
            aria-label={t("equalizer.resetMicrophone")}
          >
            <Ms name="restart_alt" style={{ fontSize: 16 }} />
          </button>
          <ProcessingInfo
            label={t("equalizer.infoLabel")}
            text={t("equalizer.info")}
          />
        </div>
      </div>
      <EqCurve
        config={eq}
        selected={selected}
        onSelect={setSelected}
        onBandChange={patchBand}
        onAddBand={addBandAt}
        onRemoveBand={removeBand}
      />
      <DspSlider
        label={t("equalizer.preamp")}
        min={-24}
        max={24}
        step={0.5}
        unit=" dB"
        value={eq.preamp_db}
        defaultValue={0}
        onChange={(value) => apply({ ...eq, preamp_db: value })}
      />
      <div className={"eqm-advanced" + (advancedOpen ? " open" : "")}>
        <button
          type="button"
          className="eqm-advanced-toggle"
          aria-expanded={advancedOpen}
          onClick={() => setAdvancedOpen((open) => !open)}
        >
          <span><Ms name="tune" style={{ fontSize: 16 }} />{t("equalizer.advanced")}</span>
          <span className="eqm-advanced-count">
            {t(eq.bands.length === 1 ? "equalizer.bandOne" : "equalizer.bandMany", { count: eq.bands.length })}
            <Ms name={advancedOpen ? "expand_less" : "expand_more"} style={{ fontSize: 17 }} />
          </span>
        </button>
        {advancedOpen && (
          <div className="eqm-bandlist">
            <div className="eqm-bands-viewport">
              <div className="eqm-bands">
                {eq.bands.map((band, index) => (
                  <EqBandRow
                    key={index}
                    index={index}
                    band={band}
                    color={bandColor(index)}
                    selected={index === selected}
                    canRemove={eq.bands.length > 1}
                    onSelect={() => setSelected(index)}
                    onChange={(patch) => patchBand(index, patch)}
                    onRemove={() => removeBand(index)}
                  />
                ))}
              </div>
            </div>
            {eq.bands.length < MAX_EQ_BANDS && (
              <button type="button" className="eqm-add" onClick={addBand}>
                <Ms name="add" style={{ fontSize: 15 }} />{t("equalizer.addBand")}
              </button>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
