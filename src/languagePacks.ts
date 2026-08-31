import { invoke } from "@tauri-apps/api/core";
import { replaceCustomLanguagePacks, validateLanguagePackWithWarnings, type LanguagePack } from "./i18nCore";

export interface LanguagePackCatalog {
  bundled: LanguagePack[];
  custom: unknown[];
  warnings: string[];
  location: string;
}

export async function reloadLanguagePacks(): Promise<LanguagePackCatalog> {
  const catalog = await invoke<LanguagePackCatalog>("get_language_pack_catalog");
  const warnings = [...catalog.warnings];
  const validatedPacks: LanguagePack[] = [];
  const locales = new Set<string>();
  for (const pack of catalog.custom) {
    const packName = typeof pack === "object" && pack !== null && "name" in pack && typeof pack.name === "string"
      ? pack.name
      : "Language pack";
    try {
      const validated = validateLanguagePackWithWarnings(pack);
      const localeKey = validated.pack.locale.toLowerCase();
      if (locales.has(localeKey)) {
        warnings.push(`${packName}: locale '${validated.pack.locale}' duplicates another language pack after canonicalization.`);
        continue;
      }
      locales.add(localeKey);
      validatedPacks.push(validated.pack);
      warnings.push(...validated.warnings.map((warning) => `${packName}: ${warning}`));
    } catch (reason: unknown) {
      warnings.push(`${packName}: ${reason instanceof Error ? reason.message : String(reason)}`);
    }
  }
  replaceCustomLanguagePacks(validatedPacks);
  return { ...catalog, warnings };
}

export function openLanguagePackLocation(): Promise<void> {
  return invoke("open_language_pack_location");
}
