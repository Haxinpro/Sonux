import { createContext, createElement, useContext, useLayoutEffect, useMemo, useSyncExternalStore, type ReactNode } from "react";
import {
  directionForLocale,
  getLanguagePackRevision,
  listAvailableLocales,
  resolveLocale,
  subscribeLanguagePacks,
  translate,
  type AvailableLocale,
  type InterpolationVariables,
  type LocalePreference,
  type TextDirection,
  type TranslationKey,
} from "./i18nCore";

interface I18nContextValue {
  locale: string;
  direction: TextDirection;
  preference: LocalePreference;
  availableLocales: AvailableLocale[];
  setPreference: (preference: LocalePreference) => void;
  t: (key: TranslationKey, variables?: InterpolationVariables) => string;
}

interface I18nProviderProps {
  children: ReactNode;
  preference: LocalePreference;
  onPreferenceChange: (preference: LocalePreference) => void;
  systemLocales?: readonly string[];
}

const ENGLISH_CONTEXT: I18nContextValue = {
  locale: "en",
  direction: "ltr",
  preference: { mode: "locale", locale: "en" },
  availableLocales: listAvailableLocales(),
  setPreference: () => {},
  t: (key, variables) => translate("en", key, variables),
};

const I18nContext = createContext<I18nContextValue>(ENGLISH_CONTEXT);

function browserLocales(): readonly string[] {
  if (typeof navigator === "undefined") return [];
  return navigator.languages.length > 0 ? navigator.languages : [navigator.language];
}

export function I18nProvider({ children, preference, onPreferenceChange, systemLocales }: I18nProviderProps) {
  const registryRevision = useSyncExternalStore(subscribeLanguagePacks, getLanguagePackRevision, getLanguagePackRevision);
  const locale = resolveLocale(preference, systemLocales ?? browserLocales());
  const direction = directionForLocale(locale);

  useLayoutEffect(() => {
    document.documentElement.lang = locale;
    document.documentElement.dir = direction;
  }, [direction, locale]);

  const value = useMemo<I18nContextValue>(() => ({
    locale,
    direction,
    preference,
    availableLocales: listAvailableLocales(),
    setPreference: onPreferenceChange,
    t: (key, variables) => translate(locale, key, variables),
  }), [direction, locale, onPreferenceChange, preference, registryRevision]);

  return createElement(I18nContext.Provider, { value }, children);
}

export function useI18n(): I18nContextValue {
  return useContext(I18nContext);
}

export type { TranslationKey } from "./i18nCore";
