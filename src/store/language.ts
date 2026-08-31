import type { LocalePreference } from "../i18nCore";

export const LANGUAGE_STORAGE_KEY = "sonux-language";

export function readLanguagePreference(): LocalePreference {
  const saved = localStorage.getItem(LANGUAGE_STORAGE_KEY);
  if (!saved || saved === "system") return { mode: "system" };
  return { mode: "locale", locale: saved };
}

export function saveLanguagePreference(preference: LocalePreference): void {
  localStorage.setItem(LANGUAGE_STORAGE_KEY, preference.mode === "system" ? "system" : preference.locale);
}
