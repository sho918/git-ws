class GitWs < Formula
  desc "Fast Git branch and worktree workspace helper"
  homepage "https://github.com/sho918/git-ws"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/sho918/git-ws/releases/download/v0.3.2/git-ws-v0.3.2-aarch64-apple-darwin.tar.gz"
      sha256 "99dea0d3f163e4d4f60e042f0dd0f3d90cfc3de8cb8f10e8046327876257edbf"
    end

    on_intel do
      url "https://github.com/sho918/git-ws/releases/download/v0.3.2/git-ws-v0.3.2-x86_64-apple-darwin.tar.gz"
      sha256 "572e9cdf80e9bcb6581c3181810018cdccb0a679af8ead8d98370691e4cc2edd"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/sho918/git-ws/releases/download/v0.3.2/git-ws-v0.3.2-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "af1a1b574ff28840481a3722c8abdaf8cf55962c35fa169c622bf16c34594506"
    end
  end

  def install
    bin.install "git-ws"
  end

  test do
    assert_match "git ws: fast Git branch/worktree workspace helper", shell_output("#{bin}/git-ws --help")
  end
end
