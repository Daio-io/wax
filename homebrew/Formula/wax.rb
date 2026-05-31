class Wax < Formula
  desc "Design-system component tracker CLI"
  homepage "https://github.com/Daio-io/wax"
  version "0.1.0-alpha.1"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/Daio-io/wax/releases/download/v#{version}/wax-#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "REPLACE_WITH_AARCH64_APPLE_DARWIN_SHA256"
    end
    if Hardware::CPU.intel?
      url "https://github.com/Daio-io/wax/releases/download/v#{version}/wax-#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "REPLACE_WITH_X86_64_APPLE_DARWIN_SHA256"
    end
  end

  def install
    bin.install "wax-#{version}-#{Hardware::CPU.arm? ? "aarch64-apple-darwin" : "x86_64-apple-darwin"}/wax" => "wax"
  end

  def caveats
    <<~EOS
      Language packs are not bundled with the CLI binary.

      After installing wax, run:
        wax init --non-interactive --language compose
        wax language install compose
    EOS
  end

  test do
    assert_match "wax", shell_output("#{bin}/wax --help")
  end
end
