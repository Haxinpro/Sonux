import type { KeyboardEvent } from "react";

interface SliderKeys {
  min: number;
  max: number;
  step: number;
  value: number;
  onChange: (value: number) => void;
}

/** Native-range keyboard semantics for custom styled sliders. */
export function handleSliderKey(
  event: KeyboardEvent,
  { min, max, step, value, onChange }: SliderKeys,
) {
  const largeStep = step * 10;
  let next: number | undefined;
  switch (event.key) {
    case "ArrowRight":
    case "ArrowUp":
      next = value + step;
      break;
    case "ArrowLeft":
    case "ArrowDown":
      next = value - step;
      break;
    case "PageUp":
      next = value + largeStep;
      break;
    case "PageDown":
      next = value - largeStep;
      break;
    case "Home":
      next = min;
      break;
    case "End":
      next = max;
      break;
    default:
      return;
  }
  event.preventDefault();
  event.stopPropagation();
  const clamped = Math.max(min, Math.min(max, next));
  const snapped = min + Math.round((clamped - min) / step) * step;
  onChange(Number(Math.max(min, Math.min(max, snapped)).toFixed(6)));
}
