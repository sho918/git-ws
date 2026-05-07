class GitWs < Formula
  desc "Fast Git branch and worktree workspace helper"
  homepage "https://github.com/sho918/git-ws"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/sho918/git-ws/releases/download/v0.1.0/git-ws-v0.1.0-aarch64-apple-darwin.tar.gz"
      sha256 "e1cb09791395a9dfd9bd8309d8b9aaf44b651f4a2702889fc9820439c685eb92"
    end

    on_intel do
      url "https://github.com/sho918/git-ws/releases/download/v0.1.0/git-ws-v0.1.0-x86_64-apple-darwin.tar.gz"
      sha256 "f062f95fdd5a16c033365806c5f00b5f4c9eac53b3d320642493094c1b6a9747"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/sho918/git-ws/releases/download/v0.1.0/git-ws-v0.1.0-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "733c1c7c54290ee7a312f8ab121c3f801d107dd2454ee47009f77f4278ebd940"
    end
  end

  def install
    bin.install "git-ws"
  end

  test do
    assert_match "git ws: fast Git branch/worktree workspace helper", shell_output("#{bin}/git-ws --help")
  end
end
