# Maintainer: Zaakir <zaakir@omarchy.org>
pkgname=incant
pkgver=0.1.0
pkgrel=1
pkgdesc="Voice dictation daemon for Hyprland / Wayland (Hex clone)"
arch=('x86_64')
url="https://github.com/zaakirio/incant"
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
optdepends=(
    'onnxruntime-cuda: CUDA acceleration for transcription'
)
source=("$pkgname-$pkgver.tar.gz::$url/archive/v$pkgver.tar.gz")
sha256sums=('SKIP')

build() {
    cd "$pkgname-$pkgver"
    cargo build --release --locked
}

package() {
    cd "$pkgname-$pkgver"

    install -Dm755 "target/release/incant-daemon" "$pkgdir/usr/bin/incant-daemon"
    install -Dm755 "target/release/incant" "$pkgdir/usr/bin/incant"
    install -Dm755 "target/release/incant-overlay" "$pkgdir/usr/bin/incant-overlay"

    # Install sherpa-onnx shared libraries.
    # They are downloaded by sherpa-rs during build to ~/.cache/sherpa-rs.
    _sherpa_lib=$(find "$HOME/.cache/sherpa-rs" -name "libsherpa-onnx-c-api.so" -path "*/sherpa-onnx-*/lib/*" | head -1)
    if [ -n "$_sherpa_lib" ]; then
        _sherpa_dir=$(dirname "$_sherpa_lib")
        install -dm755 "$pkgdir/usr/lib/incant"
        install -Dm644 "$_sherpa_dir/libsherpa-onnx-c-api.so" "$pkgdir/usr/lib/incant/"
        install -Dm644 "$_sherpa_dir/libsherpa-onnx-cxx-api.so" "$pkgdir/usr/lib/incant/"
        install -Dm644 "$_sherpa_dir/libonnxruntime.so" "$pkgdir/usr/lib/incant/"
    fi

    install -Dm644 "config/incant.toml" "$pkgdir/usr/share/incant/incant.toml"
    install -Dm644 "systemd/incant-daemon.service" "$pkgdir/usr/lib/systemd/user/incant-daemon.service"
    install -Dm644 "hyprland/incant.conf" "$pkgdir/usr/share/hyprland/incant.conf"
    install -Dm644 "README.md" "$pkgdir/usr/share/doc/incant/README.md"
    install -Dm644 "LICENSE" "$pkgdir/usr/share/licenses/incant/LICENSE"
}
