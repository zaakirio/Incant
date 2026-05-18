use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tracing::{info, warn};

use crate::config::Config;

pub enum SttEngine {
    Moonshine(sherpa_rs::moonshine::MoonshineRecognizer),
    Transducer(sherpa_rs::transducer::TransducerRecognizer),
}

impl SttEngine {
    pub fn new(config: &Config) -> Result<Self> {
        let model_path = &config.model_path;

        if model_path.join("preprocess.onnx").exists() || model_path.join("preprocess.int8.onnx").exists() {
            info!("Loading Moonshine model from {:?}", model_path);
            return Self::load_moonshine(model_path, config);
        }

        if model_path.join("encoder.onnx").exists()
            || model_path.join("encoder.int8.onnx").exists()
        {
            info!("Loading Transducer (Parakeet) model from {:?}", model_path);
            return Self::load_transducer(model_path, config);
        }

        anyhow::bail!(
            "No recognized model files found in {:?}. Run `incant-daemon download-model` first.",
            model_path
        );
    }

    fn load_moonshine(model_path: &Path, config: &Config) -> Result<Self> {
        let preprocessor = model_path.join("preprocess.onnx");
        let encoder = model_path.join("encode.int8.onnx");
        let cached_decoder = model_path.join("cached_decode.int8.onnx");
        let uncached_decoder = model_path.join("uncached_decode.int8.onnx");
        let tokens = model_path.join("tokens.txt");

        let recognizer = sherpa_rs::moonshine::MoonshineRecognizer::new(
            sherpa_rs::moonshine::MoonshineConfig {
                preprocessor: preprocessor.to_string_lossy().into(),
                encoder: encoder.to_string_lossy().into(),
                cached_decoder: cached_decoder.to_string_lossy().into(),
                uncached_decoder: uncached_decoder.to_string_lossy().into(),
                tokens: tokens.to_string_lossy().into(),
                provider: Some(detect_provider()),
                num_threads: Some(config.num_threads.max(1)),
                debug: config.debug,
            },
        )
        .map_err(|e| anyhow::anyhow!("creating Moonshine recognizer: {}", e))?;

        Ok(SttEngine::Moonshine(recognizer))
    }

    fn load_transducer(model_path: &Path, config: &Config) -> Result<Self> {
        let encoder = if model_path.join("encoder.onnx").exists() {
            model_path.join("encoder.onnx")
        } else {
            model_path.join("encoder.int8.onnx")
        };
        let decoder = if model_path.join("decoder.onnx").exists() {
            model_path.join("decoder.onnx")
        } else {
            model_path.join("decoder.int8.onnx")
        };
        let joiner = if model_path.join("joiner.onnx").exists() {
            model_path.join("joiner.onnx")
        } else {
            model_path.join("joiner.int8.onnx")
        };
        let tokens = model_path.join("tokens.txt");

        let recognizer = sherpa_rs::transducer::TransducerRecognizer::new(
            sherpa_rs::transducer::TransducerConfig {
                encoder: encoder.to_string_lossy().into(),
                decoder: decoder.to_string_lossy().into(),
                joiner: joiner.to_string_lossy().into(),
                tokens: tokens.to_string_lossy().into(),
                provider: Some(detect_provider()),
                num_threads: config.num_threads.max(1),
                sample_rate: config.sample_rate as i32,
                feature_dim: 80,
                decoding_method: "greedy_search".to_string(),
                model_type: "nemo_transducer".to_string(),
                debug: config.debug,
                ..Default::default()
            },
        )
        .map_err(|e| anyhow::anyhow!("creating Transducer recognizer: {}", e))?;

        Ok(SttEngine::Transducer(recognizer))
    }

    pub fn transcribe(&mut self, samples: &[f32], sample_rate: u32) -> Result<String> {
        match self {
            SttEngine::Moonshine(r) => {
                let result = r.transcribe(sample_rate, samples);
                Ok(result.text)
            }
            SttEngine::Transducer(r) => {
                let text = r.transcribe(sample_rate, samples);
                Ok(text)
            }
        }
    }
}

/// Detect whether CUDA is available for ONNX Runtime.
fn detect_provider() -> String {
    // Check if nvidia-smi works and if onnxruntime-cuda is installed.
    let has_nvidia = std::process::Command::new("nvidia-smi")
        .arg("-L")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    let has_cuda_runtime = std::path::Path::new("/usr/lib/libonnxruntime.so").exists()
        || std::process::Command::new("pacman")
            .args(["-Q", "onnxruntime-cuda"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);

    if has_nvidia && has_cuda_runtime {
        info!("CUDA detected, using cuda provider");
        "cuda".to_string()
    } else {
        "cpu".to_string()
    }
}

pub async fn download_model(cache_dir: &Path, model_path: &Path) -> Result<()> {
    let parakeet_dir = cache_dir.join("models/parakeet-tdt-0.6b-v2-int8");
    let moonshine_dir = cache_dir.join("models/moonshine-tiny-en-int8");

    // Download the model that matches the expected path.
    if model_path.file_name().map(|n| n.to_string_lossy().contains("parakeet")).unwrap_or(false) {
        if !parakeet_dir.join("encoder.onnx").exists() && !parakeet_dir.join("encoder.int8.onnx").exists() {
            info!("Downloading Parakeet-TDT-0.6B-v2...");
            return download_parakeet(&parakeet_dir).await;
        }
        info!("Parakeet model already exists at {:?}", parakeet_dir);
        return Ok(());
    }

    // Default to Moonshine (smaller, faster, works with current sherpa-onnx).
    if !moonshine_dir.join("preprocess.onnx").exists() {
        info!("Downloading Moonshine Tiny...");
        return download_moonshine(&moonshine_dir).await;
    }
    info!("Moonshine model already exists at {:?}", moonshine_dir);
    Ok(())
}

async fn download_parakeet(model_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(model_dir)?;

    let base_url = "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v2/resolve/main";
    let files = vec![
        "encoder.onnx",
        "decoder.onnx",
        "joiner.onnx",
        "tokens.txt",
    ];

    for file in &files {
        let dest = model_dir.join(file);
        if dest.exists() {
            info!("{} already exists, skipping", file);
            continue;
        }
        let url = format!("{}/{}", base_url, file);
        if let Err(e) = download_file_with_resume(&url, &dest).await {
            // Clean up partial download so the next run retries.
            let part = dest.with_file_name(format!("{}.__part__", dest.file_name().unwrap_or_default().to_string_lossy()));
            let _ = std::fs::remove_file(part);
            return Err(e);
        }
    }

    info!("Parakeet model downloaded to {:?}", model_dir);
    Ok(())
}

async fn download_moonshine(model_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(model_dir)?;

    let base_url = "https://huggingface.co/csukuangfj/sherpa-onnx-moonshine-tiny-en-int8/resolve/main";
    let files = vec![
        "preprocess.onnx",
        "encode.int8.onnx",
        "cached_decode.int8.onnx",
        "uncached_decode.int8.onnx",
        "tokens.txt",
    ];

    for file in &files {
        let dest = model_dir.join(file);
        if dest.exists() {
            info!("{} already exists, skipping", file);
            continue;
        }
        let url = format!("{}/{}", base_url, file);
        if let Err(e) = download_file_with_resume(&url, &dest).await {
            let part = dest.with_file_name(format!("{}.__part__", dest.file_name().unwrap_or_default().to_string_lossy()));
            let _ = std::fs::remove_file(part);
            return Err(e);
        }
    }

    info!("Moonshine model downloaded to {:?}", model_dir);
    Ok(())
}

/// Download a file with resume support via HTTP Range requests.
/// Writes to `dest.__part__` and atomically renames to `dest` on success.
async fn download_file_with_resume(url: &str, dest: &Path) -> Result<()> {
    let part_path = dest.with_file_name(format!(
        "{}.__part__",
        dest.file_name().unwrap_or_default().to_string_lossy()
    ));

    let existing_size = if part_path.exists() {
        part_path.metadata()?.len()
    } else {
        0
    };

    let client = reqwest::Client::new();
    let mut request = client.get(url);

    if existing_size > 0 {
        info!("Resuming {} from {} bytes", url, existing_size);
        request = request.header("Range", format!("bytes={}-", existing_size));
    } else {
        info!("Downloading {} ...", url);
    }

    let mut response = request.send().await?;
    let status = response.status();

    // 206 Partial Content = resumed successfully
    // 200 OK = server doesn't support Range, starting from scratch
    if !status.is_success() && status != reqwest::StatusCode::PARTIAL_CONTENT {
        anyhow::bail!("Failed to download {}: {}", url, status);
    }

    let total_size = response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&part_path)
        .with_context(|| format!("opening part file {:?}", part_path))?;

    let mut downloaded = existing_size;
    let mut last_report = std::time::Instant::now();

    while let Some(chunk) = response.chunk().await? {
        std::io::Write::write_all(&mut file, &chunk)?;
        downloaded += chunk.len() as u64;

        if last_report.elapsed() > std::time::Duration::from_secs(3) {
            let fname = dest.file_name().unwrap_or_default().to_string_lossy();
            match total_size {
                Some(total) => {
                    let pct = (downloaded as f64 / total as f64) * 100.0;
                    info!("{}: {:.1}% ({}/{} bytes)", fname, pct, downloaded, total);
                }
                None => {
                    info!("{}: {} bytes downloaded", fname, downloaded);
                }
            }
            last_report = std::time::Instant::now();
        }
    }

    // Rename part file to final destination.
    std::fs::rename(&part_path, dest)
        .with_context(|| format!("renaming {:?} to {:?}", part_path, dest))?;

    info!("Downloaded {} ({} bytes)", dest.file_name().unwrap_or_default().to_string_lossy(), downloaded);
    Ok(())
}
