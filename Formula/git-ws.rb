class GitWs < Formula
  desc "Fast Git branch and worktree workspace helper"
  homepage "https://github.com/sho918/git-ws"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/sho918/git-ws/releases/download/v0.3.1/git-ws-v0.3.1-aarch64-apple-darwin.tar.gz"
      sha256 "13c5141e17ba75893348ad73e0f8090a591415be9b51cc6d5cdb44baaa075224"
    end

    on_intel do
      url "https://github.com/sho918/git-ws/releases/download/v0.3.1/git-ws-v0.3.1-x86_64-apple-darwin.tar.gz"
      sha256 "12a0b2303385be7ca8bf06e667a7817ac51de667bb8beda8d4c7e971db1b8b60"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/sho918/git-ws/releases/download/v0.3.1/git-ws-v0.3.1-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "8c89be89faf59f58066a675139c8853ca27b8799ecd3c0f94874f7639dffdd5c"
    end
  end

  def install
    bin.install "git-ws"
  end

  test do
    assert_match "git ws: fast Git branch/worktree workspace helper", shell_output("#{bin}/git-ws --help")
  end
end
