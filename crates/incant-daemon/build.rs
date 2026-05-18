// Build script for incant-daemon.
//
// We link against the sherpa-onnx shared libraries which `sherpa-rs` downloads
// at build time. Embedding a sane RUNPATH into the binary means we do not need
// to rely on `LD_LIBRARY_PATH=...` in the systemd unit or on a system-wide
// `/etc/ld.so.conf.d/incant.conf` file.
//
// We probe two RUNPATH locations relative to the binary:
//   $ORIGIN              - libs next to the binary (release tarball layout)
//   $ORIGIN/../lib/incant - system layout (/usr/bin + /usr/lib/incant)
//
// The dynamic linker tries each in order until it finds the .so files.
//
// $ORIGIN expansion in RUNPATH is a standard ELF feature handled by ld.so(8).

fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "linux" {
        // Quote $ORIGIN so the shell doesn't try to expand it at link time.
        println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN:$ORIGIN/../lib/incant");
        // Use RUNPATH (DT_RUNPATH) rather than the older DT_RPATH so
        // LD_LIBRARY_PATH still takes precedence if a user needs to override.
        println!("cargo:rustc-link-arg=-Wl,--enable-new-dtags");
    }
}
