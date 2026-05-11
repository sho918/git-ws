class GitWs < Formula
  desc "Fast Git branch and worktree workspace helper"
  homepage "https://github.com/sho918/git-ws"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/sho918/git-ws/releases/download/v0.2.0/git-ws-v0.2.0-aarch64-apple-darwin.tar.gz"
      sha256 "5fa0854f2ac12c8f2b24c6ab48bd215786f757c57adc83796a406eb4bc546645"
    end

    on_intel do
      url "https://github.com/sho918/git-ws/releases/download/v0.2.0/git-ws-v0.2.0-x86_64-apple-darwin.tar.gz"
      sha256 "0925326b4ee7bcd2eeab475e634b4081012463b2f19999d82c2a7766e7c420f6"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/sho918/git-ws/releases/download/v0.2.0/git-ws-v0.2.0-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "e566a719cbf597bd78e73fbf33835c9267de7c709164e9b64be70a32f349b8a7"
    end
  end

  def install
    bin.install "git-ws"
  end

  test do
    assert_match "git ws: fast Git branch/worktree workspace helper", shell_output("#{bin}/git-ws --help")
  end
end
