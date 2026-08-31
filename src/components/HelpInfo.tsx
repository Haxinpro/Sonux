import { useId } from "react";
import { useI18n } from "../i18n";
import { Ms } from "./Icons";

/** Explanatory help for a page or setting. Deliberately uses a question mark;
 * ProcessingInfo keeps the separate information/status meaning. */
export function HelpInfo({ label, text, className = "" }: Readonly<{ label: string; text: string; className?: string }>) {
  const { t } = useI18n();
  const descriptionId = useId();
  return (
    <button
      type="button"
      className={`processing-info help-info${className ? ` ${className}` : ""}`}
      title={text}
      data-tooltip-title={label}
      data-tooltip-text={text}
      aria-label={t("common.help", { label })}
      aria-describedby={descriptionId}
    >
      <Ms name="help" />
      <span id={descriptionId} className="sr-only">{text}</span>
    </button>
  );
}
