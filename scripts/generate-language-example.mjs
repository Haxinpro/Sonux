import { writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { ENGLISH_TRANSLATIONS, LANGUAGE_PACK_VERSION } from "../src/i18nCore.ts";

const output = fileURLToPath(new URL("../src/locales/custom-example.json", import.meta.url));
const example = {
  version: LANGUAGE_PACK_VERSION,
  locale: "zz-Example",
  name: "Example language",
  nativeName: "Replace me",
  direction: "ltr",
  translations: ENGLISH_TRANSLATIONS,
};

await writeFile(output, `${JSON.stringify(example, null, 2)}\n`, "utf8");
