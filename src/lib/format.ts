/** Relative time for "last seen" labels. */
export function relativeTime(unixSeconds: number, locale = "en"): string {
  const delta = Math.max(0, Math.floor(Date.now() / 1000) - unixSeconds);
  const formatter = new Intl.RelativeTimeFormat(locale, { numeric: "auto", style: "narrow" });
  if (delta < 90) return formatter.format(0, "second");
  if (delta < 3600) return formatter.format(-Math.round(delta / 60), "minute");
  if (delta < 86400) return formatter.format(-Math.round(delta / 3600), "hour");
  return formatter.format(-Math.round(delta / 86400), "day");
}
