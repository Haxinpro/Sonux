#!/usr/bin/env bash
set -euo pipefail

project_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
cd "$project_dir"

missing=()
for command_name in node npm cargo rustc cc pkg-config install; do
  if ! command -v "$command_name" >/dev/null 2>&1; then
    missing+=("$command_name")
  fi
done

if ((${#missing[@]})); then
  printf 'Missing build commands: %s\n' "${missing[*]}" >&2
  printf 'Install the build dependencies listed in README.md, then run this script again.\n' >&2
  exit 1
fi

missing_libraries=()
for library in gtk+-3.0 webkit2gtk-4.1 libpipewire-0.3 libmysofa fftw3f ayatana-appindicator3-0.1; do
  if ! pkg-config --exists "$library"; then
    missing_libraries+=("$library")
  fi
done

if ((${#missing_libraries[@]})); then
  printf 'Missing development libraries: %s\n' "${missing_libraries[*]}" >&2
  printf 'Install the build dependencies listed in README.md, then run this script again.\n' >&2
  exit 1
fi

printf 'Building Sonux...\n'
npm ci
npm run build
cargo build --release --locked --manifest-path src-tauri/Cargo.toml

bin_home=${XDG_BIN_HOME:-"$HOME/.local/bin"}
data_home=${XDG_DATA_HOME:-"$HOME/.local/share"}
app_id=dev.sonux.audio

install -Dm755 target/release/sonux "$bin_home/sonux"
install -Dm644 src-tauri/icons/128x128.png \
  "$data_home/icons/hicolor/128x128/apps/$app_id.png"
install -Dm644 LICENSE "$data_home/doc/sonux/GPL-3.0.txt"
install -Dm644 THIRD_PARTY_NOTICES.md "$data_home/doc/sonux/THIRD_PARTY_NOTICES.md"
install -Dm644 src/styles/fonts/LICENSE.txt "$data_home/doc/sonux/FIRA_CODE_LICENSE.txt"
install -Dm644 third_party/licenses/APACHE-2.0.txt \
  "$data_home/doc/sonux/APACHE-2.0.txt"
install -Dm644 src-tauri/assets/test-audio/LICENSES.md \
  "$data_home/doc/sonux/TEST_AUDIO_LICENSES.md"
install -Dm644 third_party/spatial/licenses/AALTO_HRTF_ATTRIBUTION.md \
  "$data_home/doc/sonux/AALTO_HRTF_ATTRIBUTION.md"

desktop_file="$data_home/applications/$app_id.desktop"
install -d "$(dirname -- "$desktop_file")"
{
  printf '%s\n' '[Desktop Entry]'
  printf '%s\n' 'Type=Application'
  printf '%s\n' 'Name=Sonux'
  printf '%s\n' 'Comment=PipeWire gaming audio router and mixer'
  printf 'Exec=%s\n' "$bin_home/sonux"
  printf 'TryExec=%s\n' "$bin_home/sonux"
  printf 'Icon=%s\n' "$app_id"
  printf '%s\n' 'Terminal=false'
  printf '%s\n' 'Categories=AudioVideo;Audio;Utility;'
  printf '%s\n' 'Keywords=audio;mixer;pipewire;gaming;microphone;'
} > "$desktop_file"
chmod 644 "$desktop_file"

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$data_home/applications" >/dev/null 2>&1 || true
fi

printf '\nSonux installed successfully.\n'
printf 'Application: %s\n' "$bin_home/sonux"
printf 'Launcher:    %s\n' "$desktop_file"
printf 'Licenses:    %s\n' "$data_home/doc/sonux"
if [[ :$PATH: != *":$bin_home:"* ]]; then
  printf 'Note: add %s to PATH to launch Sonux by typing "sonux".\n' "$bin_home"
fi
