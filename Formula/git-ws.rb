class GitWs < Formula
  desc "Fast Git branch and worktree workspace helper"
  homepage "https://github.com/sho918/git-ws"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/sho918/git-ws/releases/download/v0.3.5/git-ws-v0.3.5-aarch64-apple-darwin.tar.gz"
      sha256 "a6990a80b5ca73b9606d87c8b006869393fa90b6c5bfac0ebe0b0b4edd46bbba"
    end

    on_intel do
      url "https://github.com/sho918/git-ws/releases/download/v0.3.5/git-ws-v0.3.5-x86_64-apple-darwin.tar.gz"
      sha256 "e0fb6a5428ebf6d33981558a77d50966229b3f168c9cafe166cab64378512a9e"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/sho918/git-ws/releases/download/v0.3.5/git-ws-v0.3.5-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "83a542a71e00ff3005a3de89998cabeebafb27721164527012ebb9b786ebfa68"
    end
  end

  def install
    bin.install "git-ws"
  end

  test do
    assert_match "git ws: fast Git branch/worktree workspace helper", shell_output("#{bin}/git-ws --help")
  end
end
