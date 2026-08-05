import { Ms } from "./Icons";

/** Small, keyboard-reachable information affordance. The global tooltip
 * system turns its title into the consistent hover card used app-wide. */
export function ProcessingInfo({ label, text }: Readonly<{ label: string; text: string }>) {
  return (
    <button
      type="button"
      className="processing-info"
      title={text}
      data-tooltip-title={label}
      data-tooltip-text={text}
      aria-label={`${label} information`}
    >
      <Ms name="info" />
    </button>
  );
}
