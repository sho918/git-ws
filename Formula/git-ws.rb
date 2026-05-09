class GitWs < Formula
  desc "Fast Git branch and worktree workspace helper"
  homepage "https://github.com/sho918/git-ws"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/sho918/git-ws/releases/download/v0.1.1/git-ws-v0.1.1-aarch64-apple-darwin.tar.gz"
      sha256 "5c96282a97bbd052d9541fc199cdbd34d71ef1eb952878c4c9aa19b07e81d7b5"
    end

    on_intel do
      url "https://github.com/sho918/git-ws/releases/download/v0.1.1/git-ws-v0.1.1-x86_64-apple-darwin.tar.gz"
      sha256 "aa9a63b447a694c9c4cd79625b29609cf97d14e1fb3b4e1ef59d0134b67b3b28"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/sho918/git-ws/releases/download/v0.1.1/git-ws-v0.1.1-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "9024b4b790e75257c864b4b1f20b5709f6173bfc5dd593d0dc4fa5a8167b076e"
    end
  end

  def install
    bin.install "git-ws"
  end

  test do
    assert_match "git ws: fast Git branch/worktree workspace helper", shell_output("#{bin}/git-ws --help")
  end
end
