import { useMixerStore } from "../../store/mixer";
import type { VirtualSink } from "../../types";
import { AppIcon } from "../AppList/AppIcon";
import { MenuCheckItem } from "../MenuItem";
import { Popover } from "../Popover";

interface Entry {
  key: string;
  name: string;
  iconPath: string | null;
  checked: boolean;
  active: boolean;
  /** Live stream indices; one app can briefly own several PipeWire streams. */
  streamIndexes: number[];
  matchProp: string;
  matchValue: string;
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
  const appStreams = useMixerStore((s) => s.appStreams);
  const seenApps = useMixerStore((s) => s.seenApps);
  const routeApp = useMixerStore((s) => s.routeApp);
  const setAppAssignment = useMixerStore((s) => s.setAppAssignment);

  const entriesByKey = new Map<string, Entry>();
  const seenKeys = new Set<string>();
  for (const s of appStreams) {
    const key = `${s.match_prop}\0${s.match_value}`;
    seenKeys.add(key);
    const existing = entriesByKey.get(key);
    if (existing) {
      existing.streamIndexes.push(s.index);
      existing.checked ||= s.assigned_sink === channel.name;
      existing.active ||= s.active;
    } else {
      entriesByKey.set(key, {
        key,
        name: s.alias ?? s.app_name,
        iconPath: s.icon_path,
        checked: s.assigned_sink === channel.name,
        active: s.active,
        streamIndexes: [s.index],
        matchProp: s.match_prop,
        matchValue: s.match_value,
      });
    }
  }
  for (const a of seenApps) {
    const key = `${a.match_prop}\0${a.match_value}`;
    if (a.ignored || seenKeys.has(key)) continue;
    entriesByKey.set(key, {
      key,
      name: a.alias ?? a.display_name,
      iconPath: a.icon_path,
      checked: a.assigned_sink === channel.name,
      active: false,
      streamIndexes: [],
      matchProp: a.match_prop,
      matchValue: a.match_value,
    });
  }
  const entries = Array.from(entriesByKey.values());
  entries.sort((a, b) => Number(b.checked) - Number(a.checked) || a.name.localeCompare(b.name));

  const toggle = (entry: Entry) => {
    if (entry.streamIndexes.length > 0) {
      for (const streamIndex of entry.streamIndexes) {
        void routeApp(streamIndex, entry.checked ? "" : channel.name);
      }
      void setAppAssignment(
        { match_prop: entry.matchProp, match_value: entry.matchValue },
        entry.checked ? null : channel.name,
      );
    } else {
      void setAppAssignment(
        { match_prop: entry.matchProp, match_value: entry.matchValue },
        entry.checked ? null : channel.name,
      );
    }
  };

  return (
    <Popover open={open} onClose={onClose} side="bottom" align="center" style={{ minWidth: 250 }}>
      {entries.length === 0 && (
        <div className="menu-item static muted">No apps discovered yet</div>
      )}
      {entries.map((entry) => (
        <MenuCheckItem key={entry.key} checked={entry.checked} onClick={() => toggle(entry)}>
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
            entry.streamIndexes.length === 0 && <span className="channel-apps-off">off</span>
          )}
        </MenuCheckItem>
      ))}
    </Popover>
  );
}
