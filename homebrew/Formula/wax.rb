class Wax < Formula
  desc "Design-system component tracker CLI"
  homepage "https://github.com/Daio-io/wax"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/Daio-io/wax/releases/download/v0.1.0-alpha.1/wax-0.1.0-alpha.1-aarch64-apple-darwin.tar.gz"
      sha256 "d1a56d34e3fcf2401e8b8075e20f51785b3a2aa6e868c2f9146a7659fe6b0a79"
    end
    if Hardware::CPU.intel?
      url "https://github.com/Daio-io/wax/releases/download/v0.1.0-alpha.1/wax-0.1.0-alpha.1-x86_64-apple-darwin.tar.gz"
      sha256 "7577f80c04e83fa6ac74045c36160aa2f29b5f1877a1e61978788ea3ed447ede"
    end
  end

  def install
    bin.install "wax" => "wax"
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
