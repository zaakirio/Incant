// Library face of `incant-daemon`. Exposes just the pieces the CLI needs
// to read the model registry and trigger downloads — the rest of the daemon
// (audio, IPC, overlay management) stays internal to the binary.
//
// Both this lib.rs and main.rs declare these modules, so they're compiled
// twice (once per crate target). The cost is a few extra seconds of build
// time; the benefit is a single source of truth for `MODELS`.

pub mod config;
pub mod stt;
