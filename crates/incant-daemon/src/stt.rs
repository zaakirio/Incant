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

pub async fn download_model(cache_dir: &Path) -> Result<()> {
    let parakeet_dir = cache_dir.join("models/parakeet-tdt-0.6b-v3-int8");
    let moonshine_dir = cache_dir.join("models/moonshine-tiny-en-int8");

    // Try Parakeet first (user's preference).
    if !parakeet_dir.join("encoder.onnx").exists() && !parakeet_dir.join("encoder.int8.onnx").exists() {
        info!("Downloading Parakeet-TDT-0.6B-v2...");
        if let Err(e) = download_parakeet(&parakeet_dir).await {
            warn!("Parakeet download failed ({}), falling back to Moonshine", e);
            return download_moonshine(&moonshine_dir).await;
        }
        return Ok(());
    }

    // If Parakeet exists, we're done.
    if parakeet_dir.join("encoder.onnx").exists() || parakeet_dir.join("encoder.int8.onnx").exists() {
        info!("Parakeet model already exists at {:?}", parakeet_dir);
        return Ok(());
    }

    download_moonshine(&moonshine_dir).await
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
        info!("Downloading {} ...", url);
        let response = reqwest::get(&url).await?;
        if !response.status().is_success() {
            anyhow::bail!("Failed to download {}: {}", url, response.status());
        }
        let bytes = response.bytes().await?;
        std::fs::write(&dest, bytes)?;
        info!("Downloaded {} ({} bytes)", file, dest.metadata()?.len());
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
        info!("Downloading {} ...", url);
        let response = reqwest::get(&url).await?;
        if !response.status().is_success() {
            anyhow::bail!("Failed to download {}: {}", url, response.status());
        }
        let bytes = response.bytes().await?;
        std::fs::write(&dest, bytes)?;
        info!("Downloaded {} ({} bytes)", file, dest.metadata()?.len());
    }

    info!("Moonshine model downloaded to {:?}", model_dir);
    Ok(())
}
