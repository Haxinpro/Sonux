import { beforeEach, describe, expect, it } from "vitest";
import example from "./locales/custom-example.json";
import {
  ENGLISH_TRANSLATIONS,
  clearCustomLanguagePacks,
  directionForLocale,
  listAvailableLocales,
  registerCustomLanguagePack,
  replaceCustomLanguagePacks,
  resolveLocale,
  translate,
  validateLanguagePack,
  validateLanguagePackWithWarnings,
} from "./i18nCore";

const FINNISH_PACK = {
  version: 1,
  locale: "fi",
  name: "Finnish",
  nativeName: "Suomi",
  direction: "ltr",
  translations: {
    "navigation.settings": "Asetukset",
    "settings.language.location": "Käännökset: {{location}}",
  },
};

describe("i18n core", () => {
  beforeEach(() => clearCustomLanguagePacks());

  it("keeps the editable example synchronized with every English key", () => {
    const source = JSON.stringify(example);
    expect(new TextEncoder().encode(source).length).toBeLessThanOrEqual(256 * 1024);
    expect(example.locale).toBe("zz-Example");
    expect(example.translations).toEqual(ENGLISH_TRANSLATIONS);
    expect(validateLanguagePack(example).translations).toEqual(ENGLISH_TRANSLATIONS);
  });

  it("validates and canonicalizes a partial custom pack", () => {
    const pack = validateLanguagePack({ ...FINNISH_PACK, locale: "pt-br" });
    expect(pack.locale).toBe("pt-BR");
    expect(pack.translations).toEqual(FINNISH_PACK.translations);
  });

  it("uses custom text with per-key English fallback and interpolation", () => {
    registerCustomLanguagePack(FINNISH_PACK);
    expect(translate("fi", "navigation.settings")).toBe("Asetukset");
    expect(translate("fi", "navigation.mixer")).toBe("Mixer");
    expect(translate("fi", "settings.language.location", { location: "/tmp" })).toBe("Käännökset: /tmp");
  });

  it("selects locale-aware custom plural forms", () => {
    registerCustomLanguagePack({
      ...FINNISH_PACK,
      locale: "ru",
      translations: {
        "settings.backups.countMany": {
          one: "{{count}} резервная копия",
          few: "{{count}} резервные копии",
          other: "{{count}} резервных копий",
        },
      },
    });
    expect(translate("ru", "settings.backups.countMany", { count: 21 })).toBe("21 резервная копия");
    expect(translate("ru", "settings.backups.countMany", { count: 24 })).toBe("24 резервные копии");
    expect(translate("ru", "settings.backups.countMany", { count: 25 })).toBe("25 резервных копий");
  });

  it("warns when plural forms are supplied for text without a count", () => {
    const result = validateLanguagePackWithWarnings({
      ...FINNISH_PACK,
      translations: { "navigation.settings": { other: "Asetukset" } },
    });
    expect(result.pack.translations).toEqual({});
    expect(result.warnings).toHaveLength(1);
  });

  it("resolves system and explicit preferences against available packs", () => {
    registerCustomLanguagePack(FINNISH_PACK);
    expect(resolveLocale({ mode: "system" }, ["sv-SE", "fi-FI"])).toBe("fi");
    expect(resolveLocale({ mode: "locale", locale: "fi-FI" })).toBe("fi");
    expect(resolveLocale({ mode: "locale", locale: "de" })).toBe("en");
    expect(directionForLocale("fi")).toBe("ltr");
    expect(listAvailableLocales().map(({ locale }) => locale)).toEqual(["en", "fi"]);
  });

  it("progressively matches parent locales", () => {
    registerCustomLanguagePack({ ...FINNISH_PACK, locale: "zh-Hant" });
    expect(resolveLocale({ mode: "system" }, ["zh-Hant-TW"])).toBe("zh-Hant");
  });

  it("rejects canonically equivalent locales in one registry update", () => {
    expect(() => replaceCustomLanguagePacks([
      { ...FINNISH_PACK, locale: "iw" },
      { ...FINNISH_PACK, locale: "he" },
    ])).toThrow(/duplicated after canonicalization/);
  });

  it("replaces the runtime registry atomically", () => {
    replaceCustomLanguagePacks([
      FINNISH_PACK,
      { ...FINNISH_PACK, locale: "de", name: "German", nativeName: "Deutsch" },
    ]);
    expect(listAvailableLocales().map(({ locale }) => locale)).toEqual(["en", "de", "fi"]);
  });

  it("ignores stale, unsafe, and placeholder-breaking translations", () => {
    const result = validateLanguagePackWithWarnings({
      ...FINNISH_PACK,
      translations: {
        "navigation.settings": "Asetukset",
        "navigation.unknown": "Tuntematon",
        "navigation.mixer": "<b>Mikseri</b>",
        "settings.language.location": "{{path}}",
      },
    });
    expect(result.pack.translations).toEqual({ "navigation.settings": "Asetukset" });
    expect(result.warnings).toHaveLength(3);
  });

  it("rejects metadata surprises and attempts to replace English", () => {
    expect(() => validateLanguagePack({ ...FINNISH_PACK, extra: true })).toThrow(/unsupported field/);
    expect(() => validateLanguagePack({ ...FINNISH_PACK, locale: "en" })).toThrow(/cannot replace bundled English/);
    expect(() => validateLanguagePack({ ...FINNISH_PACK, direction: "sideways" })).toThrow(/ltr or rtl/);
  });
});
