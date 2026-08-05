import { useState } from "react";
import { useMixerStore } from "../../store/mixer";
import type { EqBand, EqConfig, VirtualSink } from "../../types";
import { defaultEqConfig, MAX_EQ_BANDS, TONE_GAIN_RANGE_DB } from "../../types";
import { Ms } from "../Icons";
import { Toggle } from "../Toggle";
import { ProcessingInfo } from "../ProcessingInfo";
import { DspSlider } from "../Mic/DspSlider";
import { EqBandRow } from "./EqBandRow";
import { bandColor, EqCurve } from "./EqCurve";

interface EqEditorProps {
  channel: VirtualSink;
}

/** Embedded per-channel parametric EQ editor: response curve, band list, preamp. */
export function EqEditor({ channel }: Readonly<EqEditorProps>) {
  const config = useMixerStore(
    (s) => s.eqConfigs[channel.name] ?? null,
  ) ?? defaultEqConfig();
  const setChannelEq = useMixerStore((s) => s.setChannelEq);
  const backendNative = useMixerStore((s) => s.backendNative);
  const [selected, setSelected] = useState(0);
  const [advancedOpen, setAdvancedOpen] = useState(false);

  const apply = (next: EqConfig) => void setChannelEq(channel.name, next);

  const patchBand = (index: number, patch: Partial<EqBand>) => {
    const bands = config.bands.map((b, i) => (i === index ? { ...b, ...patch } : b));
    apply({ ...config, bands });
  };

  const addBand = () => {
    if (config.bands.length >= MAX_EQ_BANDS) return;
    const bands = [...config.bands, { kind: "peaking" as const, freq_hz: 1000, gain_db: 0, q: 1 }];
    apply({ ...config, bands });
    setSelected(bands.length - 1);
  };

  const addBandAt = (freq_hz: number, gain_db: number) => {
    if (config.bands.length >= MAX_EQ_BANDS) return;
    const bands = [
      ...config.bands,
      { kind: "peaking" as const, freq_hz, gain_db, q: 1 },
    ];
    apply({ ...config, bands });
    setSelected(bands.length - 1);
  };

  const removeBand = (index: number) => {
    if (config.bands.length <= 1) return;
    const bands = config.bands.filter((_, i) => i !== index);
    apply({ ...config, bands });
    setSelected(Math.min(selected, bands.length - 1));
  };

  const reset = () => {
    // Back to the flat starting layout; keep the enable switch and the
    // channel's non-EQ playback processing intact.
    const flat = defaultEqConfig();
    apply({
      ...config,
      preamp_db: flat.preamp_db,
      bands: flat.bands,
      tone_bass_db: flat.tone_bass_db,
      tone_voice_db: flat.tone_voice_db,
      tone_treble_db: flat.tone_treble_db,
    });
    setSelected(0);
  };

  const tones: { field: "tone_bass_db" | "tone_voice_db" | "tone_treble_db"; label: string; detail: string }[] = [
    { field: "tone_bass_db", label: "Bass", detail: "Broad low-frequency tone" },
    { field: "tone_voice_db", label: "Voice", detail: "Broad vocal-presence tone" },
    { field: "tone_treble_db", label: "Treble", detail: "Broad high-frequency tone" },
  ];

  return (
    <div className="eqm-editor">
      {backendNative === false && (
        <p className="modal-text">
          Parametric EQ requires the native PipeWire engine, which isn't
          running on this system.
        </p>
      )}
      <div className="eqm-head">
        <div className="processing-toggle-title">
          <Toggle
            on={config.enabled}
            onClick={() => apply({ ...config, enabled: !config.enabled })}
          />
          <div className="rtitle">Equalizer</div>
        </div>
        <div className="eqm-head-actions">
          <button
            type="button"
            className="select eqm-iconbtn"
            onClick={reset}
            title="Reset the parametric EQ and tone controls"
            aria-label="Reset EQ"
          >
            <Ms name="restart_alt" style={{ fontSize: 16 }} />
          </button>
          <ProcessingInfo
            label="Equalizer controls"
            text={'Drag a point to move it. Scroll over a point to change its width.\n\nDouble-click empty graph space to add a band. Right-click a point for options.'}
          />
        </div>
      </div>

      <EqCurve
        config={config}
        selected={selected}
        onSelect={setSelected}
        onBandChange={patchBand}
        onAddBand={addBandAt}
        onRemoveBand={removeBand}
      />

      <div className="eqm-tone-controls" aria-label="Quick tone controls">
        {tones.map(({ field, label, detail }) => {
          return (
            <div
              key={field}
              title={`${detail}; this separate tone stage does not move the parametric EQ points`}
            >
              <DspSlider
                label={label}
                min={-TONE_GAIN_RANGE_DB}
                max={TONE_GAIN_RANGE_DB}
                step={0.5}
                unit=" dB"
                value={config[field]}
                defaultValue={0}
                onChange={(gain_db) => apply({ ...config, [field]: gain_db })}
              />
            </div>
          );
        })}
      </div>

      <DspSlider
        label="Preamp"
        min={-24}
        max={24}
        step={0.5}
        unit=" dB"
        value={config.preamp_db}
        defaultValue={0}
        onChange={(v) => apply({ ...config, preamp_db: v })}
      />

      <div className={"eqm-advanced" + (advancedOpen ? " open" : "")}>
        <button
          type="button"
          className="eqm-advanced-toggle"
          aria-expanded={advancedOpen}
          onClick={() => setAdvancedOpen((open) => !open)}
        >
          <span>
            <Ms name="tune" style={{ fontSize: 16 }} />
            Advanced band controls
          </span>
          <span className="eqm-advanced-count">
            {config.bands.length} bands
            <Ms name={advancedOpen ? "expand_less" : "expand_more"} style={{ fontSize: 17 }} />
          </span>
        </button>
        {advancedOpen && (
          <div className="eqm-bandlist">
            <div className="eqm-bands-viewport">
              <div className="eqm-bands">
                {config.bands.map((band, i) => (
                  <EqBandRow
                    key={i}
                    index={i}
                    band={band}
                    color={bandColor(i)}
                    selected={i === selected}
                    canRemove={config.bands.length > 1}
                    onSelect={() => setSelected(i)}
                    onChange={(patch) => patchBand(i, patch)}
                    onRemove={() => removeBand(i)}
                  />
                ))}
              </div>
            </div>
            {config.bands.length < MAX_EQ_BANDS && (
              <button type="button" className="eqm-add" onClick={addBand}>
                <Ms name="add" style={{ fontSize: 15 }} />
                Add band
              </button>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
