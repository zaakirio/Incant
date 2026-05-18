# Maintainer: Zaakir <zaakir@omarchy.org>
pkgname=incant
pkgver=0.1.0
pkgrel=1
pkgdesc="Voice dictation daemon for Hyprland / Wayland"
arch=('x86_64')
url="https://github.com/zaakirio/Incant"
license=('MIT')
depends=(
    'pipewire'
    'pipewire-alsa'
    'gtk4'
    'gtk4-layer-shell'
    'wtype'
    'dotool'
    'wl-clipboard'
)
makedepends=('rust' 'cargo' 'cmake')
source=("$pkgname-$pkgver.tar.gz::$url/archive/v$pkgver.tar.gz")
sha256sums=('SKIP')

# NOTE — current limitation:
# This PKGBUILD relies on `sherpa-rs` downloading prebuilt sherpa-onnx
# shared libraries to ~/.cache/sherpa-rs during `cargo build`, then scrapes
# that cache to ship the .so files inside the package. This works for the
# author's machine but breaks anywhere that:
#   * uses a clean build chroot (devtools / `extra-x86_64-build`),
#   * runs as a build user without ~/.cache populated,
#   * or has HOME pointed somewhere unexpected.
#
# Until a proper build path is in place (either vendoring the libs in a
# separate `incant-sherpa-onnx` package, downloading them in source[], or
# linking against a system onnxruntime), the GitHub Releases tarball
# produced by .github/workflows/release.yml is the recommended install
# path — see README.md.

build() {
    cd "$pkgname-$pkgver"
    cargo build --release --locked
}

package() {
    cd "$pkgname-$pkgver"

    install -Dm755 "target/release/incant-daemon"  "$pkgdir/usr/bin/incant-daemon"
    install -Dm755 "target/release/incant"         "$pkgdir/usr/bin/incant"
    install -Dm755 "target/release/incant-overlay" "$pkgdir/usr/bin/incant-overlay"

    # Install sherpa-onnx shared libraries scraped from the sherpa-rs cache.
    # The daemon's RUNPATH ($ORIGIN/../lib/incant) finds these without
    # /etc/ld.so.conf.d or LD_LIBRARY_PATH.
    _sherpa_lib=$(find "$HOME/.cache/sherpa-rs" -name "libsherpa-onnx-c-api.so" -path "*/sherpa-onnx-*/lib/*" | head -1)
    if [ -n "$_sherpa_lib" ]; then
        _sherpa_dir=$(dirname "$_sherpa_lib")
        install -dm755 "$pkgdir/usr/lib/incant"
        install -Dm644 "$_sherpa_dir/libsherpa-onnx-c-api.so"   "$pkgdir/usr/lib/incant/"
        install -Dm644 "$_sherpa_dir/libsherpa-onnx-cxx-api.so" "$pkgdir/usr/lib/incant/"
        install -Dm644 "$_sherpa_dir/libonnxruntime.so"          "$pkgdir/usr/lib/incant/"
    else
        echo "ERROR: sherpa-onnx libraries not found in ~/.cache/sherpa-rs" >&2
        echo "       Run 'cargo build --release' once before makepkg, or use" >&2
        echo "       the prebuilt tarball from GitHub Releases." >&2
        return 1
    fi

    install -Dm644 "config/incant.toml"             "$pkgdir/usr/share/incant/incant.toml"
    install -Dm644 "systemd/incant-daemon.service"  "$pkgdir/usr/lib/systemd/user/incant-daemon.service"
    install -Dm644 "hyprland/incant.conf"           "$pkgdir/usr/share/hyprland/incant.conf"
    install -Dm644 "README.md"                      "$pkgdir/usr/share/doc/incant/README.md"
    install -Dm644 "LICENSE"                        "$pkgdir/usr/share/licenses/incant/LICENSE"
}
