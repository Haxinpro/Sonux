const RESTORE_WARNING_KEY = "sonux-restore-warning";

export function stashRestoreWarning(warning: string | null) {
  if (warning) localStorage.setItem(RESTORE_WARNING_KEY, warning);
  else localStorage.removeItem(RESTORE_WARNING_KEY);
}

export function takeRestoreWarning(): string | null {
  const warning = localStorage.getItem(RESTORE_WARNING_KEY);
  localStorage.removeItem(RESTORE_WARNING_KEY);
  return warning;
}
