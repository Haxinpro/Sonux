import { useCallback, useState } from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { I18nProvider } from "./i18n";
import type { LocalePreference } from "./i18nCore";
import { reloadLanguagePacks } from "./languagePacks";
import { readLanguagePreference, saveLanguagePreference } from "./store/language";
import { bootTheme } from "./store/theme";
import "material-symbols/outlined.css";
import "./styles/globals.css";

// Apply the saved theme before first paint to avoid a flash of the default.
bootTheme();

function LanguageRoot() {
  const [preference, setPreference] = useState<LocalePreference>(readLanguagePreference);
  const changePreference = useCallback((next: LocalePreference) => {
    saveLanguagePreference(next);
    setPreference(next);
  }, []);
  return <I18nProvider preference={preference} onPreferenceChange={changePreference}><App /></I18nProvider>;
}

async function start() {
  await reloadLanguagePacks().then((catalog) => {
    if (catalog.warnings.length > 0) console.warn("Sonux language-pack warnings:", catalog.warnings);
  }).catch((reason: unknown) => {
    console.warn("Sonux language packs are unavailable:", reason);
  });
  const root = document.getElementById("root");
  if (root) ReactDOM.createRoot(root).render(<LanguageRoot />);
}

void start();
