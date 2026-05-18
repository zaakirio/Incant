use crate::config::Config;

#[test]
fn test_default_config() {
    let config = Config::default();
    assert_eq!(config.sample_rate, 16000);
    assert_eq!(config.buffer_size, 4096);
    assert!(config.show_overlay);
    assert_eq!(config.minimum_key_time_ms, 150);
    assert!(config.double_tap_lock_enabled);
    assert_eq!(config.double_tap_window_ms, 300);
    assert!(config.use_double_tap_only);
    assert!(!config.debug);
}

#[test]
fn test_config_parse() {
    let toml = r#"
model_path = "/home/user/.cache/incant/models/parakeet-tdt-0.6b-v3-int8"
sample_rate = 16000
buffer_size = 4096
show_overlay = true
num_threads = 4
debug = true
minimum_key_time_ms = 200
double_tap_lock_enabled = false
double_tap_window_ms = 500
use_double_tap_only = true

output_methods = ["wtype", "wl-copy"]
"#;

    let config: Config = toml::from_str(toml).unwrap();
    assert_eq!(config.num_threads, 4);
    assert!(config.debug);
    assert_eq!(config.minimum_key_time_ms, 200);
    assert!(!config.double_tap_lock_enabled);
    assert_eq!(config.double_tap_window_ms, 500);
    assert!(config.use_double_tap_only);
    assert_eq!(config.output_methods, vec!["wtype", "wl-copy"]);
}
