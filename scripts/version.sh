#!/bin/sh

set -eu

usage() {
  printf 'usage: %s <X.Y.Z>\n' "$0" >&2
}

if [ "$#" -ne 1 ]; then
  usage
  exit 2
fi

version=$1

case "$version" in
  v*)
    printf 'version must not include a leading v: %s\n' "$version" >&2
    exit 2
    ;;
esac

if ! printf '%s\n' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$'; then
  printf 'invalid semver: %s\n' "$version" >&2
  exit 2
fi

tmp=$(mktemp)
awk -v version="$version" '
  BEGIN { in_package = 0; changed = 0 }
  /^\[package\]$/ { in_package = 1; print; next }
  /^\[/ && $0 != "[package]" { in_package = 0 }
  in_package && /^version = / && changed == 0 {
    print "version = \"" version "\""
    changed = 1
    next
  }
  { print }
  END { if (changed == 0) exit 1 }
' Cargo.toml > "$tmp"
mv "$tmp" Cargo.toml

cargo check

printf 'updated package version to %s\n' "$version"
printf 'release tag: v%s\n' "$version"
printf '\nnext steps:\n'
printf '  git add Cargo.toml Cargo.lock\n'
printf '  git commit -m "chore: release v%s"\n' "$version"
printf '  git tag -a v%s -m "Release v%s"\n' "$version" "$version"
printf '  git push origin HEAD --tags\n'
