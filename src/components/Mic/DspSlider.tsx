import { useCallback, useEffect, useRef } from "react";

interface DspSliderProps {
  label: string;
  /** Optional label shown at the opposite end of the track. */
  endLabel?: string;
  min: number;
  max: number;
  step: number;
  value: number;
  /** Marked with a tick on the track; double-click resets to it. */
  defaultValue: number;
  unit: string;
  /** Place the end label in the value column instead of above the track. */
  inlineEndLabel?: boolean;
  disabled?: boolean;
  onChange: (value: number) => void;
}

/** Horizontal parameter slider with a default-value tick. */
export function DspSlider({
  label,
  endLabel,
  min,
  max,
  step,
  value,
  defaultValue,
  unit,
  inlineEndLabel = false,
  disabled = false,
  onChange,
}: Readonly<DspSliderProps>) {
  const trackRef = useRef<HTMLDivElement>(null);
  const dragging = useRef(false);

  const setFromEvent = useCallback(
    (clientX: number) => {
      if (disabled) return;
      const el = trackRef.current;
      if (!el) return;
      const r = el.getBoundingClientRect();
      const pct = Math.max(0, Math.min(1, (clientX - r.left) / r.width));
      const raw = min + pct * (max - min);
      const snapped = Math.round(raw / step) * step;
      onChange(Math.max(min, Math.min(max, Number(snapped.toFixed(2)))));
    },
    [disabled, min, max, step, onChange],
  );

  // Attached once; reads the latest handler through a ref (see Fader).
  const setFromEventRef = useRef(setFromEvent);
  setFromEventRef.current = setFromEvent;

  useEffect(() => {
    const move = (e: PointerEvent) => {
      if (dragging.current) setFromEventRef.current(e.clientX);
    };
    const up = () => {
      dragging.current = false;
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
  }, []);

  const pct = ((value - min) / (max - min)) * 100;
  const defaultPct = ((defaultValue - min) / (max - min)) * 100;
  const hasEndLabels = endLabel !== undefined && !inlineEndLabel;

  return (
    <div className={"dsp-row" + (disabled ? " disabled" : "") + (hasEndLabels ? " end-labels" : "")}>
      {hasEndLabels ? (
        <div className="dsp-range-column">
          <div className="dsp-range-labels">
            <span>{label}</span>
            <span>{endLabel}</span>
          </div>
          <div
            className="hs-track"
            ref={trackRef}
            title={`Default: ${defaultValue}${unit} (double-click to reset)`}
            onPointerDown={(e) => {
              if (disabled) return;
              dragging.current = true;
              setFromEvent(e.clientX);
            }}
            onDoubleClick={() => {
              if (!disabled) onChange(defaultValue);
            }}
          >
            <div className="dsp-default-tick" style={{ left: defaultPct + "%" }} />
            <div className="hs-fill" style={{ width: pct + "%" }} />
            <div className="hs-cap" style={{ left: pct + "%" }} />
          </div>
        </div>
      ) : (
        <>
          <span className="dsp-label">{label}</span>
          <div
            className="hs-track"
            ref={trackRef}
            title={`Default: ${defaultValue}${unit} (double-click to reset)`}
            onPointerDown={(e) => {
              if (disabled) return;
              dragging.current = true;
              setFromEvent(e.clientX);
            }}
            onDoubleClick={() => {
              if (!disabled) onChange(defaultValue);
            }}
          >
            <div className="dsp-default-tick" style={{ left: defaultPct + "%" }} />
            <div className="hs-fill" style={{ width: pct + "%" }} />
            <div className="hs-cap" style={{ left: pct + "%" }} />
          </div>
        </>
      )}
      <span className="dsp-value">
        {inlineEndLabel ? endLabel : `${value}${unit}`}
      </span>
    </div>
  );
}
