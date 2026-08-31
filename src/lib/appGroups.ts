import type { AppIdentity, AppStream, SeenApp, SeenAppGroup } from "../types";

export function applicationGroupKey(app: Pick<SeenApp | AppStream, "desktop_id" | "match_prop" | "match_value">): string {
  const desktopId = app.desktop_id?.trim().toLowerCase();
  return desktopId
    ? `desktop:${desktopId}`
    : `identity:${app.match_prop}\0${app.match_value}`;
}

export function groupRouteState(routes: readonly (string | null)[], channel: string) {
  const checked = routes.length > 0 && routes.every((route) => route === channel);
  const mixed = routes.some((route) => route === channel) && !checked;
  return { checked, mixed };
}

function identityOf(app: Pick<SeenApp, "match_prop" | "match_value">): AppIdentity {
  return { match_prop: app.match_prop, match_value: app.match_value };
}

/** Collapse helper-process history into canonical desktop applications while
 * retaining every exact identity needed by PipeWire and WirePlumber rules. */
export function groupSeenApps(apps: readonly SeenApp[]): SeenAppGroup[] {
  const groups = new Map<string, SeenApp[]>();
  for (const app of apps) {
    const key = applicationGroupKey(app);
    const members = groups.get(key);
    if (members) members.push(app);
    else groups.set(key, [app]);
  }

  return Array.from(groups, ([group_key, members]) => {
    const sorted = [...members].sort((left, right) => right.last_seen - left.last_seen);
    const representative = sorted[0];
    const firstAssignment = members[0].assigned_sink;
    const assignment_mixed = members.some((member) => member.assigned_sink !== firstAssignment);
    const assigned_sinks = Array.from(new Set(
      members.flatMap((member) => member.assigned_sink === null ? [] : [member.assigned_sink]),
    ));
    const firstAlias = members[0].alias;
    const sharedAlias = firstAlias !== null
      && members.every((member) => member.alias === firstAlias)
      ? firstAlias
      : null;
    const icon = sorted.find((member) => member.icon_path !== null)?.icon_path ?? representative.icon_path;

    return {
      ...representative,
      group_key,
      identities: members.map(identityOf),
      icon_path: icon,
      alias: sharedAlias,
      ignored: members.every((member) => member.ignored),
      assigned_sink: assignment_mixed ? null : firstAssignment,
      assignment_mixed,
      assigned_sinks,
    };
  });
}
