import { getCurrentWindow } from "@tauri-apps/api/window";
import { Ms, SinkMark } from "../Icons";
import { useI18n } from "../../i18n";

/**
 * Frameless window chrome: brand, current screen and window controls.
 * The close button triggers the normal close-requested flow, which the
 * Rust side intercepts to hide to tray.
 */
export function TitleBar({ screen }: Readonly<{ screen: string }>) {
  const { t } = useI18n();
  const win = getCurrentWindow();

  return (
    <header data-tauri-drag-region className="headerbar">
      <div data-tauri-drag-region className="hb-brand">
        <div className="hb-logo">
          <SinkMark />
        </div>
        <div data-tauri-drag-region className="hb-title">
          {t("app.name")}
        </div>
      </div>
      <div data-tauri-drag-region className="hb-sub">
        {screen}
      </div>
      <div data-tauri-drag-region className="hb-spacer" />
      <div className="wctl">
        <button type="button" className="wbtn" aria-label={t("window.minimize")} onClick={() => void win.minimize()}>
          <Ms name="remove" />
        </button>
        <button
          type="button"
          className="wbtn"
          aria-label={t("window.maximize")}
          onClick={() => void win.toggleMaximize()}
        >
          <Ms name="crop_square" style={{ fontSize: 13 }} />
        </button>
        <button
          type="button"
          className="wbtn close"
          aria-label={t("window.closeToTray")}
          title={t("window.closeToTrayHint")}
          onClick={() => void win.close()}
        >
          <Ms name="close" />
        </button>
      </div>
    </header>
  );
}
