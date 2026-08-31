import type { EqBand, EqBandKind } from "../../types";
import { EQ_FREQ_MAX_HZ, EQ_FREQ_MIN_HZ, EQ_GAIN_RANGE_DB } from "../../types";
import { Ms } from "../Icons";
import { useI18n, type TranslationKey } from "../../i18n";

const KINDS: { value: EqBandKind; label: TranslationKey }[] = [
  { value: "peaking", label: "equalizer.peak" },
  { value: "low_shelf", label: "equalizer.lowShelf" },
  { value: "high_shelf", label: "equalizer.highShelf" },
  { value: "low_pass", label: "equalizer.lowPass" },
  { value: "high_pass", label: "equalizer.highPass" },
];

const isShelf = (kind: EqBandKind) => kind === "low_shelf" || kind === "high_shelf";
const isPass = (kind: EqBandKind) => kind === "low_pass" || kind === "high_pass";

interface EqBandRowProps {
  /** Zero-based position, used for the row's accessible name. */
  index: number;
  band: EqBand;
  /** Dot color on the curve - repeated here so row and dot read as one. */
  color: string;
  selected: boolean;
  /** False for the sole remaining band; the EQ always keeps at least one. */
  canRemove: boolean;
  onSelect: () => void;
  onChange: (patch: Partial<EqBand>) => void;
  onRemove: () => void;
}

/** Numeric editor for one band - the keyboard-accessible twin of the
 *  curve dot (drag isn't reachable for everyone). */
export function EqBandRow({ index, band, color, selected, canRemove, onSelect, onChange, onRemove }: Readonly<EqBandRowProps>) {
  const { t } = useI18n();
  const clampNum = (v: string, lo: number, hi: number, fallback: number) => {
    const n = Number(v);
    return Number.isFinite(n) ? Math.max(lo, Math.min(hi, n)) : fallback;
  };

  const widthLabel = t(isShelf(band.kind) ? "equalizer.slope" : "equalizer.q");

  return (
    <div
      className={"eqm-band" + (selected ? " sel" : "")}
      role="group"
      aria-label={t("equalizer.band", { number: index + 1 })}
      onPointerDown={onSelect}
    >
      <span className="eqm-chip" style={{ background: color }} aria-hidden="true" />
      <select
        className="eqm-kind"
        value={band.kind}
        aria-label={t("equalizer.bandType", { number: index + 1 })}
        onChange={(e) => onChange({ kind: e.target.value as EqBandKind })}
      >
        {KINDS.map((k) => (
          <option key={k.value} value={k.value}>
            {t(k.label)}
          </option>
        ))}
      </select>
      <label className="eqm-field">
        <span>Hz</span>
        <input
          type="number"
          min={EQ_FREQ_MIN_HZ}
          max={EQ_FREQ_MAX_HZ}
          step={1}
          value={Math.round(band.freq_hz)}
          aria-label={t("equalizer.frequencyLabel", { number: index + 1 })}
          onChange={(e) =>
            onChange({
              freq_hz: clampNum(e.target.value, EQ_FREQ_MIN_HZ, EQ_FREQ_MAX_HZ, band.freq_hz),
            })
          }
        />
      </label>
      <label className="eqm-field">
        <span>dB</span>
        <input
          type="number"
          min={-EQ_GAIN_RANGE_DB}
          max={EQ_GAIN_RANGE_DB}
          step={0.5}
          value={band.gain_db}
          disabled={isPass(band.kind)}
          title={isPass(band.kind) ? t("equalizer.passNoGain") : undefined}
          aria-label={t("equalizer.gainLabel", { number: index + 1 })}
          onChange={(e) =>
            onChange({
              gain_db: clampNum(e.target.value, -EQ_GAIN_RANGE_DB, EQ_GAIN_RANGE_DB, band.gain_db),
            })
          }
        />
      </label>
      <label className="eqm-field">
        {/* One schema field, two meanings (see EqBand.q). */}
        <span>{widthLabel}</span>
        <input
          type="number"
          min={0.1}
          max={10}
          step={0.1}
          value={band.q}
          aria-label={t("equalizer.widthLabel", { number: index + 1, width: widthLabel })}
          onChange={(e) => onChange({ q: clampNum(e.target.value, 0.1, 10, band.q) })}
        />
      </label>
      <button
        type="button"
        className="eqm-remove"
        disabled={!canRemove}
        title={t(canRemove ? "equalizer.removeBandHint" : "equalizer.keepOneBand")}
        aria-label={t("equalizer.removeBand", { number: index + 1 })}
        onClick={onRemove}
      >
        <Ms name="close" style={{ fontSize: 14 }} />
      </button>
    </div>
  );
}
