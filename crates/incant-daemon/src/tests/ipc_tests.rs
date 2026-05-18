//! State-machine tests for the IPC press/release/cancel handlers.
//!
//! These verify the behavior we most care about getting right for the
//! default user experience:
//!
//! * In double-tap-only mode, a lone press must never start recording
//!   (so `Alt`, `Alt+Tab`, `Alt+F4` etc. cannot accidentally trigger us).
//! * A second press within `double_tap_window_ms` *does* start a locked
//!   recording.
//! * A second press *outside* the window is treated as a fresh first tap.
//! * In locked recording, a further press stops the recording.
//! * `Release` is a no-op in double-tap-only mode.
//! * `Cancel` always clears state.

use crate::config::Config;
use crate::ipc::process_command;
use crate::protocol::{Command, Status};
use crate::state::AppState;

fn double_tap_only_config() -> Config {
    Config {
        use_double_tap_only: true,
        double_tap_lock_enabled: true,
        double_tap_window_ms: 300,
        ..Config::default()
    }
}

fn press_and_hold_config() -> Config {
    Config {
        use_double_tap_only: false,
        double_tap_lock_enabled: true,
        double_tap_window_ms: 300,
        ..Config::default()
    }
}

#[tokio::test]
async fn lone_press_in_double_tap_only_does_not_record() {
    let state = AppState::default();
    let config = double_tap_only_config();

    let resp = process_command(Command::Press, &state, &config).await;
    assert!(resp.ok, "first tap should be accepted: {:?}", resp.message);
    assert!(!state.is_recording(), "lone tap must not start recording");
    assert!(!state.is_locked());
}

#[tokio::test]
async fn double_tap_within_window_starts_locked_recording() {
    let state = AppState::default();
    let config = double_tap_only_config();

    let _ = process_command(Command::Press, &state, &config).await;
    assert!(!state.is_recording());

    // Second tap immediately afterwards is within the 300 ms window.
    let resp = process_command(Command::Press, &state, &config).await;
    assert!(resp.ok);
    assert!(state.is_recording(), "second tap should start recording");
    assert!(
        state.is_locked(),
        "double-tap-only always enters locked mode"
    );
}

#[tokio::test]
async fn third_tap_in_locked_mode_stops_recording() {
    let state = AppState::default();
    let config = double_tap_only_config();

    // Promote to locked recording.
    let _ = process_command(Command::Press, &state, &config).await;
    let _ = process_command(Command::Press, &state, &config).await;
    assert!(state.is_recording());
    assert!(state.is_locked());

    // Third tap = stop.
    let resp = process_command(Command::Press, &state, &config).await;
    assert!(resp.ok);
    assert!(!state.is_recording(), "third tap should stop recording");
    assert!(!state.is_locked());
}

#[tokio::test]
async fn second_tap_after_window_is_treated_as_fresh_first_tap() {
    let state = AppState::default();
    let mut config = double_tap_only_config();
    // Make the double-tap window so short that any real-world delay misses it.
    config.double_tap_window_ms = 1;

    let _ = process_command(Command::Press, &state, &config).await;
    // Sleep past the window.
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    let resp = process_command(Command::Press, &state, &config).await;
    assert!(resp.ok);
    assert!(
        !state.is_recording(),
        "second tap outside double-tap window must not start recording in double-tap-only mode"
    );
}

#[tokio::test]
async fn release_is_noop_in_double_tap_only_mode() {
    let state = AppState::default();
    let config = double_tap_only_config();

    // Get into locked recording.
    let _ = process_command(Command::Press, &state, &config).await;
    let _ = process_command(Command::Press, &state, &config).await;
    assert!(state.is_recording());

    // Release must NOT stop the recording in double-tap-only mode.
    let resp = process_command(Command::Release, &state, &config).await;
    assert!(resp.ok);
    assert!(
        state.is_recording(),
        "release must not stop a locked recording"
    );
}

#[tokio::test]
async fn press_and_hold_starts_recording_on_first_press() {
    let state = AppState::default();
    let config = press_and_hold_config();

    let resp = process_command(Command::Press, &state, &config).await;
    assert!(resp.ok);
    assert!(
        state.is_recording(),
        "press-and-hold should record on first press"
    );
    assert!(!state.is_locked(), "first press should not be locked");
}

#[tokio::test]
async fn press_and_hold_release_stops_recording() {
    let state = AppState::default();
    let config = press_and_hold_config();

    let _ = process_command(Command::Press, &state, &config).await;
    assert!(state.is_recording());

    let resp = process_command(Command::Release, &state, &config).await;
    assert!(resp.ok);
    assert!(
        !state.is_recording(),
        "release should stop recording in press-and-hold mode"
    );
}

#[tokio::test]
async fn press_and_hold_double_tap_locks() {
    let state = AppState::default();
    let config = press_and_hold_config();

    // Press → release (quick): the press itself starts recording, release stops it.
    let _ = process_command(Command::Press, &state, &config).await;
    let _ = process_command(Command::Release, &state, &config).await;
    assert!(!state.is_recording());

    // Second press within the window should now lock.
    let resp = process_command(Command::Press, &state, &config).await;
    assert!(resp.ok);
    assert!(state.is_recording());
    assert!(
        state.is_locked(),
        "double-tap should promote to locked recording"
    );
}

#[tokio::test]
async fn cancel_clears_recording_state() {
    let state = AppState::default();
    let config = double_tap_only_config();

    let _ = process_command(Command::Press, &state, &config).await;
    let _ = process_command(Command::Press, &state, &config).await;
    assert!(state.is_recording());

    let resp = process_command(Command::Cancel, &state, &config).await;
    assert!(resp.ok);
    assert!(!state.is_recording(), "cancel should stop recording");
}

#[tokio::test]
async fn ping_returns_ok() {
    let state = AppState::default();
    let config = Config::default();
    let resp = process_command(Command::Ping, &state, &config).await;
    assert!(resp.ok);
    assert_eq!(resp.message, "pong");
}

#[tokio::test]
async fn status_does_not_change_state() {
    let state = AppState::default();
    let config = Config::default();
    state.set_status(Status::Hidden);
    let resp = process_command(Command::Status, &state, &config).await;
    assert!(resp.ok);
    assert!(!state.is_recording());
}
