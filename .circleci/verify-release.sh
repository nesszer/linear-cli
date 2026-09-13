#!/usr/bin/env bash
set -euo pipefail

: "${CIRCLE_TAG:?This job must run from a release tag}"
if [[ ! "$CIRCLE_TAG" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "Unsupported release tag: $CIRCLE_TAG" >&2
  exit 1
fi

version="${CIRCLE_TAG#v}"
cargo_version="$(awk -F'"' '/^version = "/ { print $2; exit }' Cargo.toml)"
test "$cargo_version" = "$version"

target_specs_text="$(python3 .circleci/release_targets.py --tsv)"
test -n "$target_specs_text"
mapfile -t target_specs <<< "$target_specs_text"
test "${#target_specs[@]}" -gt 0
expected=()
for spec in "${target_specs[@]}"; do
  IFS=$'\t' read -r _ archive _ _ _ _ <<< "$spec"
  expected+=("$archive")
done
mkdir -p release
for spec in "${target_specs[@]}"; do
  IFS=$'\t' read -r _ archive _ _ _ _ <<< "$spec"
  found="$(find artifacts -type f -name "$archive" -print -quit)"
  test -n "$found"
  cp -- "$found" "release/$archive"

  proof="$(dirname -- "$found")/version.txt"
  test -f "$proof"
  test "$(tr -d '\r\n' < "$proof")" = "linear-cli $version"
done

actual_count="$(find artifacts -type f \( -name 'linear-cli-*.tar.gz' -o -name 'linear-cli-*.zip' \) | wc -l)"
test "$actual_count" -eq "${#expected[@]}"

command -v file >/dev/null
extract_dir="$(mktemp -d)"
trap 'rm -rf -- "$extract_dir"' EXIT
for spec in "${target_specs[@]}"; do
  IFS=$'\t' read -r target archive archive_type binary file_pattern checks_version <<< "$spec"
  destination="$extract_dir/$target"
  mkdir -p "$destination"
  if [[ "$archive_type" == "tar.gz" ]]; then
    test "$(tar -tzf "release/$archive")" = "$binary"
    tar -xzf "release/$archive" -C "$destination"
    binary_path="$destination/$binary"
    test -x "$binary_path"
    file_description="$(file -b "$binary_path")"
    grep -Eq "$file_pattern" <<<"$file_description"
    if [[ "$checks_version" == "true" ]]; then
      reported="$($binary_path --version)"
      test "$reported" = "linear-cli $version"
    fi
  else
    test "$(unzip -Z1 "release/$archive" | tr -d '\r')" = "$binary"
    unzip -q "release/$archive" -d "$destination"
    binary_path="$destination/$binary"
    test -f "$binary_path"
    file_description="$(file -b "$binary_path")"
    grep -Eq "$file_pattern" <<<"$file_description"
  fi
done

sha256sum release/linear-cli-* > release/SHA256SUMS
test "$(wc -l < release/SHA256SUMS)" -eq "${#expected[@]}"
python3 .circleci/release-manifest.py "$version"
test "$(find release -maxdepth 1 -type f | wc -l)" -eq 7
