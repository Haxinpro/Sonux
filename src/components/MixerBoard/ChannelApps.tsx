import { useMixerStore } from "../../store/mixer";
import type { AppIdentity, VirtualSink } from "../../types";
import { applicationGroupKey, groupRouteState, groupSeenApps } from "../../lib/appGroups";
import { AppIcon } from "../AppList/AppIcon";
import { MenuCheckItem } from "../MenuItem";
import { Popover } from "../Popover";
import { useI18n } from "../../i18n";

interface Entry {
  key: string;
  name: string;
  iconPath: string | null;
  active: boolean;
  /** Live stream indices; one app can briefly own several PipeWire streams. */
  streamIndexes: number[];
  identities: AppIdentity[];
  desktopId: string | null;
  liveRoutes: Array<string | null>;
  liveIdentityKeys: Set<string>;
  historyRoutes: Map<string, string | null>;
}

const identityKey = (identity: AppIdentity) => `${identity.match_prop}\0${identity.match_value}`;

function addIdentity(identities: AppIdentity[], identity: AppIdentity) {
  if (!identities.some((candidate) => (
    candidate.match_prop === identity.match_prop && candidate.match_value === identity.match_value
  ))) identities.push(identity);
}

/**
 * Channel membership editor: every known app (live and not running) with a
 * checkbox. Checking moves/assigns the app to this channel; unchecking
 * sends it back to the default output.
 */
export function ChannelApps({
  channel,
  open,
  onClose,
}: Readonly<{
  channel: VirtualSink;
  open: boolean;
  onClose: () => void;
}>) {
  const { t } = useI18n();
  const appStreams = useMixerStore((s) => s.appStreams);
  const seenApps = useMixerStore((s) => s.seenApps);
  const routeAppGroup = useMixerStore((s) => s.routeAppGroup);
  const setAppGroupAssignment = useMixerStore((s) => s.setAppGroupAssignment);

  const entriesByKey = new Map<string, Entry>();
  for (const s of appStreams) {
    const key = applicationGroupKey(s);
    const existing = entriesByKey.get(key);
    if (existing) {
      existing.streamIndexes.push(s.index);
      existing.active ||= s.active;
      existing.desktopId ??= s.desktop_id;
      addIdentity(existing.identities, s);
      existing.liveRoutes.push(s.assigned_sink);
      existing.liveIdentityKeys.add(identityKey(s));
    } else {
      entriesByKey.set(key, {
        key,
        name: s.alias ?? s.app_name,
        iconPath: s.icon_path,
        active: s.active,
        streamIndexes: [s.index],
        identities: [{ match_prop: s.match_prop, match_value: s.match_value }],
        desktopId: s.desktop_id,
        liveRoutes: [s.assigned_sink],
        liveIdentityKeys: new Set([identityKey(s)]),
        historyRoutes: new Map(),
      });
    }
  }
  for (const app of groupSeenApps(seenApps)) {
    if (app.ignored) continue;
    const existing = entriesByKey.get(app.group_key);
    if (existing) {
      for (const identity of app.identities) addIdentity(existing.identities, identity);
      for (const member of seenApps.filter((candidate) => applicationGroupKey(candidate) === app.group_key)) {
        if (!existing.liveIdentityKeys.has(identityKey(member))) {
          existing.historyRoutes.set(identityKey(member), member.assigned_sink);
        }
      }
      existing.iconPath ??= app.icon_path;
    } else {
      entriesByKey.set(app.group_key, {
        key: app.group_key,
        name: app.alias ?? app.display_name,
        iconPath: app.icon_path,
        active: false,
        streamIndexes: [],
        identities: [...app.identities],
        desktopId: app.desktop_id,
        liveRoutes: [],
        liveIdentityKeys: new Set(),
        historyRoutes: new Map(
          seenApps
            .filter((candidate) => applicationGroupKey(candidate) === app.group_key)
            .map((member) => [identityKey(member), member.assigned_sink]),
        ),
      });
    }
  }
  const entries = Array.from(entriesByKey.values()).map((entry) => {
    const routes = [...entry.liveRoutes, ...entry.historyRoutes.values()];
    return { ...entry, ...groupRouteState(routes, channel.name) };
  });
  entries.sort((a, b) => Number(b.checked) - Number(a.checked) || Number(b.mixed) - Number(a.mixed) || a.name.localeCompare(b.name));

  const toggle = (entry: Entry & { checked: boolean; mixed: boolean }) => {
    if (entry.streamIndexes.length > 0) {
      void routeAppGroup(
        entry.streamIndexes,
        entry.identities,
        entry.desktopId,
        entry.checked ? "" : channel.name,
      );
    } else {
      void setAppGroupAssignment(entry.identities, entry.checked ? null : channel.name);
    }
  };

  return (
    <Popover open={open} onClose={onClose} side="bottom" align="center" style={{ minWidth: 250 }}>
      {entries.length === 0 && (
        <div className="menu-item static muted">{t("mixer.apps.none")}</div>
      )}
      {entries.map((entry) => (
        <MenuCheckItem key={entry.key} checked={entry.checked} mixed={entry.mixed} onClick={() => toggle(entry)}>
          <span className="channel-apps-icon">
            <AppIcon iconPath={entry.iconPath} />
          </span>
          <span className="channel-apps-name">{entry.name}</span>
          {entry.active ? (
            <span className="eq on channel-apps-eq" aria-hidden="true">
              <i />
              <i />
              <i />
            </span>
          ) : (
            entry.streamIndexes.length === 0 && <span className="channel-apps-off">{t("common.state.off")}</span>
          )}
        </MenuCheckItem>
      ))}
    </Popover>
  );
}
