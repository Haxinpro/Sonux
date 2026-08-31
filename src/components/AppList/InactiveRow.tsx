import { useMixerStore } from "../../store/mixer";
import type { SeenAppGroup } from "../../types";
import { relativeTime } from "../../lib/format";
import { IconButton } from "../IconButton";
import { AppIcon } from "./AppIcon";
import { ChannelSelect } from "./ChannelSelect";
import { useI18n } from "../../i18n";

/**
 * A previously-seen app that isn't currently playing. Routing edits here
 * are "pre-routing": they take effect the moment the app next plays audio.
 * Ignored apps use the same row minus the routing control - they are hidden
 * from Sonux until un-ignored, so there is nothing to route.
 */
export function InactiveRow({ app, ignored }: Readonly<{ app: SeenAppGroup; ignored?: boolean }>) {
  const { locale, t } = useI18n();
  const setAppGroupAssignment = useMixerStore((s) => s.setAppGroupAssignment);
  const setAppGroupIgnored = useMixerStore((s) => s.setAppGroupIgnored);
  const forgetAppGroup = useMixerStore((s) => s.forgetAppGroup);

  return (
    <div className="row row-inactive">
      <div className="ricon">
        <AppIcon iconPath={app.icon_path} />
      </div>
      <div className="rmain">
        <div className="rtitle" title={app.match_value}>
          <span className="rname">{app.alias ?? app.display_name}</span>
        </div>
        <div className="rsub">{t("applications.lastSeen", { time: relativeTime(app.last_seen, locale) })}</div>
      </div>
      <div className="rtrail">
        {!ignored && (
          <ChannelSelect
            value={app.assigned_sink}
            mixed={app.assignment_mixed}
            onChange={(sinkName) => void setAppGroupAssignment(app.identities, sinkName === "" ? null : sinkName)}
          />
        )}
        {ignored ? (
          <IconButton
            reveal
            icon="visibility"
            title={t("applications.stopIgnoringHint")}
            label={t("applications.stopIgnoring", { name: app.display_name })}
            onClick={() => void setAppGroupIgnored(app.identities, false)}
          />
        ) : (
          <IconButton
            reveal
            icon="visibility_off"
            title={t("applications.ignoreHint")}
            label={t("applications.ignore", { name: app.display_name })}
            onClick={() => void setAppGroupIgnored(app.identities, true)}
          />
        )}
        <IconButton
          reveal
          icon="delete"
          title={t("applications.forgetHint")}
          label={t("applications.forget", { name: app.display_name })}
          onClick={() => void forgetAppGroup(app.identities)}
        />
      </div>
    </div>
  );
}
