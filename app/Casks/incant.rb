cask "incant" do
  version "0.1.0"
  sha256 "REPLACE_WITH_ZIP_SHA256"

  url "https://github.com/zaakirio/incant-app/releases/download/v#{version}/Incant-#{version}.zip"
  name "Incant"
  desc "Menu bar companion that narrates Claude Code, Codex, and OpenCode turns"
  homepage "https://github.com/zaakirio/incant-app"

  depends_on macos: ">= :sonoma"

  app "Incant.app"

  # The Python narration engine ships on PyPI, not Homebrew. The app's
  # onboarding detects a missing engine and shows the install command;
  # if `incant` is already on PATH we wire the hooks up here.
  postflight do
    incant = which("incant")
    system_command incant, args: ["install", "--yes"], must_succeed: false if incant
  end

  zap trash: [
    "~/.config/incant",
    "~/.local/state/incant",
    "~/Library/Preferences/com.zaakir.incant.plist",
  ]
end
