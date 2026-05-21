class GitWs < Formula
  desc "Fast Git branch and worktree workspace helper"
  homepage "https://github.com/sho918/git-ws"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/sho918/git-ws/releases/download/v0.3.3/git-ws-v0.3.3-aarch64-apple-darwin.tar.gz"
      sha256 "2d24732732d863d78d163e578e52e217ccdd36aaef503a4791498f52d4c7f2ae"
    end

    on_intel do
      url "https://github.com/sho918/git-ws/releases/download/v0.3.3/git-ws-v0.3.3-x86_64-apple-darwin.tar.gz"
      sha256 "874f715d45b1e170c413e26be1fb76e6cf9adcac436fb179c39c13345ded5014"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/sho918/git-ws/releases/download/v0.3.3/git-ws-v0.3.3-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "fc79eccc350500a15bb472e1cb68c8ef448f499d0b197e75313b3750824ae390"
    end
  end

  def install
    bin.install "git-ws"
  end

  test do
    assert_match "git ws: fast Git branch/worktree workspace helper", shell_output("#{bin}/git-ws --help")
  end
end
