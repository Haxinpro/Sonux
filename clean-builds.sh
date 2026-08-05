#!/usr/bin/env bash
set -euo pipefail

project_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)

if [[ ! -f "$project_dir/package.json" ||
      ! -f "$project_dir/src-tauri/Cargo.toml" ||
      ! -f "$project_dir/install.sh" ]]; then
  printf 'Refusing to clean: this does not look like the Sonux project directory.\n' >&2
  exit 1
fi

if (($# > 1)) || (($# == 1)) && [[ $1 != --yes ]]; then
  printf 'Usage: %s [--yes]\n' "$0" >&2
  exit 1
fi

generated_paths=(
  "$project_dir/target"
  "$project_dir/node_modules"
  "$project_dir/dist"
  "$project_dir/src-tauri/gen"
)

printf 'The following Sonux-generated paths will be removed if they exist:\n'
for generated_path in "${generated_paths[@]}"; do
  printf '  %s\n' "$generated_path"
done
printf '\nSource files, the installed application, and ~/.config/sonux are not removed.\n'

if (($# == 0)); then
  read -r -p 'Continue? [y/N] ' response
  case $response in
    y|Y|yes|YES) ;;
    *)
      printf 'Cancelled.\n'
      exit 0
      ;;
  esac
fi

for generated_path in "${generated_paths[@]}"; do
  case $generated_path in
    "$project_dir"/*) rm -rf -- "$generated_path" ;;
    *)
      printf 'Refusing to remove path outside the Sonux project: %s\n' "$generated_path" >&2
      exit 1
      ;;
  esac
done

printf 'Sonux build files were removed. Run ./install.sh to build them again.\n'
