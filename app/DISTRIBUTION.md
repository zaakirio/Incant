# Distributing Incant

Incant ships as two pieces: the **engine** (Python, on PyPI) and the **menu bar app** (a signed, notarized macOS app). A user installs the engine, then the app; the app's onboarding walks them through anything missing.

## The whole picture

```
User runs:  brew install --cask incant
              └─ installs Incant.app  (this repo)
              └─ depends on the incant engine (PyPI)  → pip/pipx/uv
App first launch → onboarding checks everything, offers copy-paste fixes
Updates:    Sparkle in-app auto-update (app) + pip/brew (engine)
```

## 1. Engine → PyPI

The engine lives in the sibling `incant` repo (a normal Python package).

```sh
cd ../incant
uv build
uv publish            # needs a PyPI token (UV_PUBLISH_TOKEN or --token)
```

Users then get it via `uv tool install incant`, `pipx install incant`, or `pip install incant`.

## 2. App → signed, notarized download

**One-time setup (what you provide):**

1. Apple Developer Program membership ($99/yr).
2. A **Developer ID Application** certificate (Xcode → Settings → Accounts → Manage Certificates → +). Note: the "Apple Development" cert already on this machine is **not** enough — notarization requires "Developer ID Application".
3. Store a notary credential once:
   ```sh
   xcrun notarytool store-credentials incant-notary \
     --apple-id "you@example.com" --team-id "TEAMID" \
     --password "app-specific-password"   # from appleid.apple.com
   ```

**Each release:**

```sh
SIGN_ID="Developer ID Application: Your Name (TEAMID)" ./release.sh
```

This builds, signs with hardened runtime, notarizes, staples, and writes `dist/Incant-<version>.zip` plus its sha256. Attach the zip to a GitHub release tagged `v<version>`.

## 3. Homebrew cask

`Casks/incant.rb` is the cask. Per release, bump `version` and `sha256` (printed by `release.sh`). Publish it via a tap repo (e.g. `zaakirio/homebrew-incant`) so users run:

```sh
brew tap zaakirio/incant
brew install --cask incant
```

The cask declares a dependency on the engine and runs `incant install --yes` in a postflight, so a cask install lands both halves wired up.

## 4. In-app auto-update (Sparkle)

The app is wired for [Sparkle](https://sparkle-project.org): it reads an appcast feed and updates itself. To go live:

1. Generate an EdDSA key once: `./bin/generate_keys` (from Sparkle), keep the private key safe, put the public key in `Info.plist` as `SUPublicEDKey`.
2. Host `appcast.xml` (GitHub Pages or the releases page); set its URL as `SUFeedURL` in `Info.plist`.
3. Each release, sign the zip (`sign_update Incant-<v>.zip`) and add an `<item>` to the appcast.

Until the feed and key are set, "Check for Updates" is inert — the app still runs fine.

## Gate summary

Everything except **step 2's Apple Developer ID** is buildable today. The signing identity is the one thing only you can provision; once `SIGN_ID` points at a real Developer ID Application cert, `release.sh` produces a shippable, notarized build.
