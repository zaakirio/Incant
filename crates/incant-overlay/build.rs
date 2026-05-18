// Build script for incant-overlay.
//
// The overlay links against `gtk4-layer-shell`, which is not packaged on
// Debian stable, Ubuntu 22.04/24.04, RHEL/Rocky 9, or older Fedora releases.
// Our release pipeline builds it from source and bundles the resulting .so
// into the tarball under lib/incant/. Embedding a sane RUNPATH means the
// overlay finds it at runtime without LD_LIBRARY_PATH or system ld.so.conf.d
// changes.
//
// We probe two RUNPATH locations relative to the binary:
//   $ORIGIN               - libs next to the binary (build tree, dev runs)
//   $ORIGIN/../lib/incant - release tarball + system install layout
//
// $ORIGIN expansion in RUNPATH is a standard ELF feature handled by ld.so(8).
// We use DT_RUNPATH (via --enable-new-dtags) so LD_LIBRARY_PATH still wins
// for explicit user overrides.

fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "linux" {
        println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN:$ORIGIN/../lib/incant");
        println!("cargo:rustc-link-arg=-Wl,--enable-new-dtags");
    }
}
