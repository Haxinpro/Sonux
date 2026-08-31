import { useId } from "react";
import { Ms } from "./Icons";
import { useI18n } from "../i18n";

/** Small, keyboard-reachable information affordance. The global tooltip
 * system turns its title into the consistent hover card used app-wide. */
export function ProcessingInfo({ label, text }: Readonly<{ label: string; text: string }>) {
  const { t } = useI18n();
  const descriptionId = useId();
  return (
    <button
      type="button"
      className="processing-info"
      title={text}
      data-tooltip-title={label}
      data-tooltip-text={text}
      aria-label={t("common.information", { label })}
      aria-describedby={descriptionId}
    >
      <Ms name="info" />
      <span id={descriptionId} className="sr-only">{text}</span>
    </button>
  );
}
