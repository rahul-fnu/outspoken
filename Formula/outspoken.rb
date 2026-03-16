class Outspoken < Formula
  desc "AI-powered dictation app CLI"
  homepage "https://github.com/rahul-fnu/outspoken"
  url "https://github.com/rahul-fnu/outspoken.git",
      tag:      "v0.1.0",
      revision: "HEAD"
  license "MIT"
  head "https://github.com/rahul-fnu/outspoken.git", branch: "main"

  depends_on "cmake" => :build
  depends_on "rust" => :build
  depends_on :macos

  def install
    cd "src-tauri" do
      system "cargo", "build", "--release",
             "--bin", "outspoken",
             "--no-default-features",
             "--features", "metal"
      bin.install "target/release/outspoken"
    end
  end

  test do
    assert_match "outspoken", shell_output("#{bin}/outspoken --help", 2)
  end
end
