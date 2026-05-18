use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::protocol::{DaemonState, Meter, Status};

/// Press-and-hold state machine for the dictation workflow.
#[derive(Debug, Clone)]
pub struct AppState {
    pub status: Arc<Mutex<Status>>,
    pub meter: Arc<Mutex<Meter>>,
    pub recording: Arc<AtomicBool>,
    pub locked_mode: Arc<AtomicBool>,
    pub recording_start: Arc<Mutex<Option<Instant>>>,
    pub last_press_time: Arc<Mutex<Option<Instant>>>,
    pub audio_buffer: Arc<Mutex<Vec<f32>>>,
    pub last_result: Arc<Mutex<Option<String>>>,
    pub error_message: Arc<Mutex<Option<String>>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            status: Arc::new(Mutex::new(Status::Hidden)),
            meter: Arc::new(Mutex::new(Meter::default())),
            recording: Arc::new(AtomicBool::new(false)),
            locked_mode: Arc::new(AtomicBool::new(false)),
            recording_start: Arc::new(Mutex::new(None)),
            last_press_time: Arc::new(Mutex::new(None)),
            audio_buffer: Arc::new(Mutex::new(Vec::new())),
            last_result: Arc::new(Mutex::new(None)),
            error_message: Arc::new(Mutex::new(None)),
        }
    }
}

impl AppState {
    pub fn current(&self) -> DaemonState {
        DaemonState {
            status: *self.status.lock().unwrap(),
            meter: *self.meter.lock().unwrap(),
            message: self.error_message.lock().unwrap().clone(),
        }
    }

    pub fn set_status(&self, status: Status) {
        *self.status.lock().unwrap() = status;
    }

    pub fn set_meter(&self, avg: f32, peak: f32) {
        *self.meter.lock().unwrap() = Meter {
            average_power: avg,
            peak_power: peak,
        };
    }

    pub fn reset_meter(&self) {
        *self.meter.lock().unwrap() = Meter::default();
    }

    pub fn is_recording(&self) -> bool {
        self.recording.load(Ordering::SeqCst)
    }

    pub fn start_recording(&self) {
        self.recording.store(true, Ordering::SeqCst);
        *self.recording_start.lock().unwrap() = Some(Instant::now());
        self.audio_buffer.lock().unwrap().clear();
        self.reset_meter();
    }

    /// Stop recording and return how long it ran. Kept as part of the public
    /// `AppState` surface even though the current state machine clears the
    /// recording flag inline (see `ipc::process_command`); future refactors
    /// should route through this method so `recording_start` is properly
    /// cleared in one place.
    #[allow(dead_code)]
    pub fn stop_recording(&self) -> Option<Duration> {
        self.recording.store(false, Ordering::SeqCst);
        self.locked_mode.store(false, Ordering::SeqCst);
        self.recording_start
            .lock()
            .unwrap()
            .take()
            .map(|start| start.elapsed())
    }

    #[allow(dead_code)]
    pub fn recording_duration(&self) -> Option<Duration> {
        self.recording_start
            .lock()
            .unwrap()
            .as_ref()
            .map(|s| s.elapsed())
    }

    pub fn is_locked(&self) -> bool {
        self.locked_mode.load(Ordering::SeqCst)
    }

    pub fn set_locked(&self, locked: bool) {
        self.locked_mode.store(locked, Ordering::SeqCst);
    }

    pub fn last_press_elapsed(&self) -> Option<Duration> {
        self.last_press_time
            .lock()
            .unwrap()
            .as_ref()
            .map(|t| t.elapsed())
    }

    pub fn record_press(&self) {
        *self.last_press_time.lock().unwrap() = Some(Instant::now());
    }

    pub fn clear_audio(&self) {
        self.audio_buffer.lock().unwrap().clear();
    }

    pub fn append_audio(&self, samples: &[f32]) {
        self.audio_buffer.lock().unwrap().extend_from_slice(samples);
    }

    pub fn take_audio(&self) -> Vec<f32> {
        std::mem::take(&mut *self.audio_buffer.lock().unwrap())
    }

    pub fn set_result(&self, text: String) {
        *self.last_result.lock().unwrap() = Some(text);
    }

    #[allow(dead_code)]
    pub fn take_result(&self) -> Option<String> {
        self.last_result.lock().unwrap().take()
    }

    pub fn set_error(&self, msg: Option<String>) {
        *self.error_message.lock().unwrap() = msg;
    }
}
