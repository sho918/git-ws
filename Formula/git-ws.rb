class GitWs < Formula
  desc "Fast Git branch and worktree workspace helper"
  homepage "https://github.com/sho918/git-ws"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/sho918/git-ws/releases/download/v0.3.0/git-ws-v0.3.0-aarch64-apple-darwin.tar.gz"
      sha256 "b3356cceb7e9eea04867fa13cab999afc6148a09b43a5e566210268a4eb54ba8"
    end

    on_intel do
      url "https://github.com/sho918/git-ws/releases/download/v0.3.0/git-ws-v0.3.0-x86_64-apple-darwin.tar.gz"
      sha256 "071e45a1ec8c8a2a00e304515ea4b93c6bf4a32f5adb4bc00e24c4f7a08a60f7"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/sho918/git-ws/releases/download/v0.3.0/git-ws-v0.3.0-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "a337e316247f5ba1ffc726db13cb36c29f56b271711b1c0e6a790c9b265ba6d5"
    end
  end

  def install
    bin.install "git-ws"
  end

  test do
    assert_match "git ws: fast Git branch/worktree workspace helper", shell_output("#{bin}/git-ws --help")
  end
end
