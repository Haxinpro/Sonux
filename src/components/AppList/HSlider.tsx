import { useCallback, useEffect, useRef } from "react";
import { handleSliderKey } from "../../lib/sliderKeyboard";

interface HSliderProps {
  value: number;
  max: number;
  ariaLabel: string;
  onChange: (value: number) => void;
  valueLabel?: string;
}

/** Horizontal per-app volume slider. */
export function HSlider({ value, max, ariaLabel, onChange, valueLabel }: Readonly<HSliderProps>) {
  const trackRef = useRef<HTMLDivElement>(null);
  const dragging = useRef(false);

  const setFromEvent = useCallback(
    (clientX: number) => {
      const el = trackRef.current;
      if (!el) return;
      const r = el.getBoundingClientRect();
      const pct = Math.max(0, Math.min(1, (clientX - r.left) / r.width));
      onChange(Math.round(pct * max));
    },
    [onChange, max],
  );

  useEffect(() => {
    const move = (e: PointerEvent) => {
      if (dragging.current) setFromEvent(e.clientX);
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
  }, [setFromEvent]);

  const pct = (Math.max(0, Math.min(max, value)) / max) * 100;

  return (
    <div className="hslider">
      <div
        className="hs-track"
        ref={trackRef}
        role="slider"
        tabIndex={0}
        aria-label={ariaLabel}
        aria-valuemin={0}
        aria-valuemax={max}
        aria-valuenow={value}
        aria-valuetext={valueLabel ?? `${value}%`}
        onKeyDown={(event) => handleSliderKey(event, {
          min: 0, max, step: 1, value, onChange,
        })}
        onPointerDown={(e) => {
          dragging.current = true;
          setFromEvent(e.clientX);
        }}
      >
        <div className="hs-fill" style={{ width: pct + "%" }} />
        <div
          className="hs-cap"
          style={{ left: pct + "%" }}
          onPointerDown={(e) => {
            e.stopPropagation();
            dragging.current = true;
          }}
        />
      </div>
      <div className="hs-val">{valueLabel ?? `${value}%`}</div>
    </div>
  );
}
