class GitWs < Formula
  desc "Fast Git branch and worktree workspace helper"
  homepage "https://github.com/sho918/git-ws"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/sho918/git-ws/releases/download/v0.3.4/git-ws-v0.3.4-aarch64-apple-darwin.tar.gz"
      sha256 "8f2bf555d3ee12b6ecd26f53b0d4eea4d115900922b546619849cbebe71a4294"
    end

    on_intel do
      url "https://github.com/sho918/git-ws/releases/download/v0.3.4/git-ws-v0.3.4-x86_64-apple-darwin.tar.gz"
      sha256 "0073c95d413c58de6fca029e27f54ed2e8fa3160be9f4bdea974ed9ae188f514"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/sho918/git-ws/releases/download/v0.3.4/git-ws-v0.3.4-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "ea4804c04e57ae5a7c5085e178694b7326cfa55a4d9059be73594bb05e23a284"
    end
  end

  def install
    bin.install "git-ws"
  end

  test do
    assert_match "git ws: fast Git branch/worktree workspace helper", shell_output("#{bin}/git-ws --help")
  end
end
