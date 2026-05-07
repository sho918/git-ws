#!/bin/sh

set -eu

usage() {
  printf 'usage: %s <vX.Y.Z> <SHA256SUMS>\n' "$0" >&2
}

if [ "$#" -ne 2 ]; then
  usage
  exit 2
fi

version=$1
sums=$2
formula=${GIT_WS_FORMULA:-Formula/git-ws.rb}

if ! printf '%s\n' "$version" | grep -Eq '^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$'; then
  printf 'invalid release version: %s\n' "$version" >&2
  exit 2
fi

if [ ! -f "$sums" ]; then
  printf 'checksum file not found: %s\n' "$sums" >&2
  exit 2
fi

checksum_for() {
  name=$1
  checksum=$(awk -v name="$name" '$2 == name { print $1 }' "$sums")
  if [ -z "$checksum" ]; then
    printf 'checksum not found for %s\n' "$name" >&2
    exit 1
  fi
  if ! printf '%s\n' "$checksum" | grep -Eq '^[0-9a-fA-F]{64}$'; then
    printf 'invalid checksum for %s: %s\n' "$name" "$checksum" >&2
    exit 1
  fi
  printf '%s\n' "$checksum"
}

darwin_arm="git-ws-${version}-aarch64-apple-darwin.tar.gz"
darwin_intel="git-ws-${version}-x86_64-apple-darwin.tar.gz"
linux_intel="git-ws-${version}-x86_64-unknown-linux-gnu.tar.gz"

darwin_arm_sha=$(checksum_for "$darwin_arm")
darwin_intel_sha=$(checksum_for "$darwin_intel")
linux_intel_sha=$(checksum_for "$linux_intel")

tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT
cat > "$tmp" <<EOF
class GitWs < Formula
  desc "Fast Git branch and worktree workspace helper"
  homepage "https://github.com/sho918/git-ws"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/sho918/git-ws/releases/download/${version}/${darwin_arm}"
      sha256 "${darwin_arm_sha}"
    end

    on_intel do
      url "https://github.com/sho918/git-ws/releases/download/${version}/${darwin_intel}"
      sha256 "${darwin_intel_sha}"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/sho918/git-ws/releases/download/${version}/${linux_intel}"
      sha256 "${linux_intel_sha}"
    end
  end

  def install
    bin.install "git-ws"
  end

  test do
    assert_match "git ws: fast Git branch/worktree workspace helper", shell_output("#{bin}/git-ws --help")
  end
end
EOF

mv "$tmp" "$formula"
printf 'updated %s for %s\n' "$formula" "$version"
