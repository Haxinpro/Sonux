#!/usr/bin/env bash
set -euo pipefail

bin_home=${XDG_BIN_HOME:-"$HOME/.local/bin"}
data_home=${XDG_DATA_HOME:-"$HOME/.local/share"}
app_id=dev.sonux.audio

if command -v systemctl >/dev/null 2>&1; then
  systemctl --user disable --now sonux.service >/dev/null 2>&1 || true
fi

rm -f -- \
  "$bin_home/sonux" \
  "$data_home/applications/$app_id.desktop" \
  "$data_home/icons/hicolor/128x128/apps/$app_id.png" \
  "$data_home/doc/sonux/GPL-3.0.txt" \
  "$data_home/doc/sonux/THIRD_PARTY_NOTICES.md" \
  "$data_home/doc/sonux/FIRA_CODE_LICENSE.txt" \
  "$data_home/doc/sonux/APACHE-2.0.txt" \
  "$data_home/doc/sonux/TEST_AUDIO_LICENSES.md" \
  "$data_home/doc/sonux/AALTO_HRTF_ATTRIBUTION.md"

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$data_home/applications" >/dev/null 2>&1 || true
fi

printf 'Sonux was uninstalled. Settings in ~/.config/sonux were kept.\n'
