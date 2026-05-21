use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_model_path")]
    pub model_path: PathBuf,

    #[serde(default = "default_sample_rate")]
    pub sample_rate: u32,

    #[serde(default = "default_socket_path")]
    pub socket_path: PathBuf,

    #[serde(default = "default_cache_dir")]
    pub cache_dir: PathBuf,

    #[serde(default = "default_buffer_size")]
    pub buffer_size: usize,

    #[serde(default = "default_output_methods")]
    pub output_methods: Vec<String>,

    #[serde(default = "default_true")]
    pub show_overlay: bool,

    #[serde(default)]
    pub num_threads: i32,

    #[serde(default)]
    pub debug: bool,

    /// Minimum recording duration in milliseconds before transcription proceeds.
    /// Recordings shorter than this are silently discarded.
    #[serde(default = "default_minimum_key_time_ms")]
    pub minimum_key_time_ms: u64,

    /// Maximum time in milliseconds to stay in Preparing state waiting for
    /// audio energy. After this we promote to Recording regardless.
    #[serde(default = "default_max_preparing_duration_ms")]
    pub max_preparing_duration_ms: u64,

    /// Peak audio power threshold for promoting from Preparing → Recording.
    /// Only used in single-press (non double-tap-only) mode. If the mic peak
    /// power stays below this after minimum_key_time_ms, the overlay stays
    /// hidden until speech is detected or max_preparing_duration_ms elapses.
    /// This prevents modifier+key combos (Alt-Tab, etc.) from flashing the HUD.
    #[serde(default = "default_promotion_peak_threshold")]
    pub promotion_peak_threshold: f32,

    /// Enable double-tap to lock recording mode.
    #[serde(default = "default_true")]
    pub double_tap_lock_enabled: bool,

    /// Double-tap window in milliseconds.
    #[serde(default = "default_double_tap_window_ms")]
    pub double_tap_window_ms: u64,

    /// Use double-tap only (no press-and-hold).
    /// When true, a single press does nothing; you must tap twice quickly to
    /// start locked recording. This eliminates all modifier-combo false
    /// positives at the cost of the natural hold-to-record feel.
    #[serde(default = "default_false")]
    pub use_double_tap_only: bool,

    /// Sound effect volume (0.0 - 1.0).
    #[serde(default = "default_sound_volume")]
    pub sound_volume: f32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            model_path: default_model_path(),
            sample_rate: default_sample_rate(),
            socket_path: default_socket_path(),
            cache_dir: default_cache_dir(),
            buffer_size: default_buffer_size(),
            output_methods: default_output_methods(),
            show_overlay: true,
            num_threads: 0,
            debug: false,
            minimum_key_time_ms: default_minimum_key_time_ms(),
            max_preparing_duration_ms: default_max_preparing_duration_ms(),
            promotion_peak_threshold: default_promotion_peak_threshold(),
            double_tap_lock_enabled: true,
            double_tap_window_ms: default_double_tap_window_ms(),
            use_double_tap_only: true,
            sound_volume: default_sound_volume(),
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let config_dir = dirs::config_dir()
            .context("could not find config directory")?
            .join("incant");
        let config_file = config_dir.join("config.toml");

        if !config_file.exists() {
            std::fs::create_dir_all(&config_dir)?;
            let default = Config::default();
            let toml = toml::to_string_pretty(&default)?;
            std::fs::write(&config_file, toml)?;
            tracing::info!("Created default config at {:?}", config_file);
            return Ok(default);
        }

        let contents = std::fs::read_to_string(&config_file)
            .with_context(|| format!("reading config file {:?}", config_file))?;
        let config: Config = toml::from_str(&contents)
            .with_context(|| format!("parsing config file {:?}", config_file))?;

        Ok(config)
    }
}

fn default_model_path() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("~/.cache"))
        .join("incant/models/parakeet-tdt-0.6b-v3-int8")
}

fn default_sample_rate() -> u32 {
    16000
}

fn default_socket_path() -> PathBuf {
    dirs::runtime_dir()
        .or_else(dirs::cache_dir)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("incant/daemon.sock")
}

fn default_cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("~/.cache"))
        .join("incant")
}

fn default_buffer_size() -> usize {
    4096
}

fn default_output_methods() -> Vec<String> {
    vec![
        "wtype".to_string(),
        "dotool".to_string(),
        "wl-copy".to_string(),
    ]
}

fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
}

fn default_minimum_key_time_ms() -> u64 {
    150
}

fn default_max_preparing_duration_ms() -> u64 {
    2000
}

fn default_promotion_peak_threshold() -> f32 {
    0.02
}

fn default_double_tap_window_ms() -> u64 {
    300
}

fn default_sound_volume() -> f32 {
    0.3
}
