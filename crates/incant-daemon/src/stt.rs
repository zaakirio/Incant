use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tracing::info;

use crate::config::Config;

/// A single model file we know how to download and verify.
struct ModelFile {
    name: &'static str,
    /// Hex-encoded SHA-256 of the file content.
    sha256: &'static str,
    /// Expected size in bytes.
    size: u64,
}

/// Which sherpa-onnx model family this entry is. Determines how files are
/// wired into `OfflineRecognizerConfig` at load time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelKind {
    /// Parakeet-style transducer (encoder + decoder + joiner).
    Transducer,
    /// Whisper (encoder + decoder; may reference external .weights).
    Whisper,
    /// Moonshine (preprocessor + encoder + cached/uncached decoders).
    Moonshine,
}

/// One known, downloadable STT model. Adding a new model is one struct literal
/// in `MODELS` plus (if the family is new) one match arm in `SttEngine::new`.
pub struct ModelDef {
    /// Friendly name used in config (`model = "parakeet"`) and CLI.
    pub name: &'static str,
    /// Subdirectory under `<cache_dir>/models/` where files are stored.
    pub dir_name: &'static str,
    /// One-line summary shown by `incant model list`.
    pub description: &'static str,
    /// HuggingFace repo to pull from.
    repo: &'static str,
    /// Pinned commit so a moving `main` branch can't change the bits.
    revision: &'static str,
    /// Files to download + verify.
    files: &'static [ModelFile],
    /// Which load path this model takes.
    kind: ModelKind,
    /// File whose presence in the dir uniquely identifies this model. Used
    /// both for "already downloaded?" checks and runtime kind detection.
    marker_file: &'static str,
}

/// Parakeet TDT 0.6B v3 INT8 — NVIDIA's multilingual successor to v2.
/// Supports 25 European languages (English + Bulgarian, Croatian, Czech,
/// Danish, Dutch, Estonian, Finnish, French, German, Greek, Hungarian,
/// Italian, Latvian, Lithuanian, Maltese, Polish, Portuguese, Romanian,
/// Slovak, Slovenian, Spanish, Swedish, Russian, Ukrainian).
/// Pinned to a specific HuggingFace commit so the integrity of the bits we
/// load into ONNX Runtime is not at the mercy of a moving `main` branch.
const PARAKEET_REPO: &str = "csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8";
const PARAKEET_REVISION: &str = "2bda32ec70b097a55adaa07d9a7173915b43cc78";
const PARAKEET_FILES: &[ModelFile] = &[
    ModelFile {
        name: "encoder.int8.onnx",
        sha256: "acfc2b4456377e15d04f0243af540b7fe7c992f8d898d751cf134c3a55fd2247",
        size: 652_184_281,
    },
    ModelFile {
        name: "decoder.int8.onnx",
        sha256: "179e50c43d1a9de79c8a24149a2f9bac6eb5981823f2a2ed88d655b24248db4e",
        size: 11_845_275,
    },
    ModelFile {
        name: "joiner.int8.onnx",
        sha256: "3164c13fc2821009440d20fcb5fdc78bff28b4db2f8d0f0b329101719c0948b3",
        size: 6_355_277,
    },
    ModelFile {
        name: "tokens.txt",
        sha256: "d58544679ea4bc6ac563d1f545eb7d474bd6cfa467f0a6e2c1dc1c7d37e3c35d",
        size: 93_939,
    },
];

/// Whisper large-v3-turbo INT8 — OpenAI's multilingual ASR model (~809M params),
/// distilled for speed. Supports 99 languages including English and Russian, with
/// strong robustness on mumbled / fast / disfluent speech at the cost of much
/// higher CPU latency than Parakeet. Encoder ships with an external 2.4 GB
/// weights file because the FP32 graph exceeds ONNX's 2 GB protobuf limit; the
/// int8 graph references it via external_data so it must live alongside the
/// .onnx file at load time.
const WHISPER_REPO: &str = "csukuangfj/sherpa-onnx-whisper-turbo";
const WHISPER_REVISION: &str = "2ca6ff69fc878651b770880507669577ac41c2ff";
const WHISPER_FILES: &[ModelFile] = &[
    ModelFile {
        name: "turbo-encoder.int8.onnx",
        sha256: "b02dcdf54f348741e93fe732b67d933c8dcb6735655f710640143081db38878b",
        size: 674_716_297,
    },
    ModelFile {
        name: "turbo-decoder.int8.onnx",
        sha256: "20accd02388482eb3a46bd615631adfdc85e1eb2c7db9ea3f02a40ffe6b81547",
        size: 361_080_764,
    },
    ModelFile {
        name: "turbo-encoder.weights",
        sha256: "746f879ecf066450ab0cdecc05383380b85157270ff6c0a9fb7cfdd917036e12",
        size: 2_600_325_120,
    },
    ModelFile {
        name: "turbo-tokens.txt",
        sha256: "b34b360dbb493e781e479794586d661700670d65564001f23024971d1f2fa126",
        size: 816_730,
    },
];

const MOONSHINE_REPO: &str = "csukuangfj/sherpa-onnx-moonshine-tiny-en-int8";
const MOONSHINE_REVISION: &str = "bf2b762c076d8ea61e2af0b3851c9564fb77552e";
const MOONSHINE_FILES: &[ModelFile] = &[
    ModelFile {
        name: "preprocess.onnx",
        sha256: "f33addce61a143460fe753b5ee5b7db255e5140b5b779c065b94f6c83ff0bf4e",
        size: 6_800_738,
    },
    ModelFile {
        name: "encode.int8.onnx",
        sha256: "8774dfba578de027ec6595c2c654a0836434489bc963a0db124a7f181f571acb",
        size: 18_249_187,
    },
    ModelFile {
        name: "cached_decode.int8.onnx",
        sha256: "2aff28bba6a03d8dcf5c9feac45462629bae37317442299f28115ad09da773f6",
        size: 45_264_830,
    },
    ModelFile {
        name: "uncached_decode.int8.onnx",
        sha256: "216737000dd5881a17aa043f6bbd286add33e4c3b0ae257153e2ec15438bdc41",
        size: 53_216_096,
    },
    ModelFile {
        name: "tokens.txt",
        sha256: "1165c2aeb9f72f457a83be2d459a09054f27490acd9b41bd43794dfd25e296ea",
        size: 436_688,
    },
];

/// Registry of every model `incant` knows how to download and load.
/// Ordering is significant: the first entry is the default if config doesn't
/// pick one. Adding a new model: append a `ModelDef` literal here, and if it
/// uses a new `ModelKind`, add a match arm in `SttEngine::new`.
pub const MODELS: &[ModelDef] = &[
    ModelDef {
        name: "parakeet",
        dir_name: "parakeet-tdt-0.6b-v3-int8",
        description: "Parakeet TDT 0.6B v3 (25 European languages, ~670 MB, fast — default)",
        repo: PARAKEET_REPO,
        revision: PARAKEET_REVISION,
        files: PARAKEET_FILES,
        kind: ModelKind::Transducer,
        marker_file: "encoder.int8.onnx",
    },
    ModelDef {
        name: "whisper",
        dir_name: "whisper-large-v3-turbo-int8",
        description: "Whisper large-v3-turbo (99 languages, ~3.4 GB, robust on mumbled / fast speech)",
        repo: WHISPER_REPO,
        revision: WHISPER_REVISION,
        files: WHISPER_FILES,
        kind: ModelKind::Whisper,
        marker_file: "turbo-encoder.int8.onnx",
    },
    ModelDef {
        name: "moonshine",
        dir_name: "moonshine-tiny-en-int8",
        description: "Moonshine Tiny (English only, ~120 MB, fastest)",
        repo: MOONSHINE_REPO,
        revision: MOONSHINE_REVISION,
        files: MOONSHINE_FILES,
        kind: ModelKind::Moonshine,
        marker_file: "preprocess.onnx",
    },
];

/// Look up a model definition by its friendly name.
pub fn find_by_name(name: &str) -> Option<&'static ModelDef> {
    MODELS.iter().find(|m| m.name == name)
}

/// Detect which model lives at `dir` by checking for marker files.
pub fn detect_in_dir(dir: &Path) -> Option<&'static ModelDef> {
    MODELS.iter().find(|m| dir.join(m.marker_file).exists())
}

/// Canonical on-disk location for a given model under `<cache_dir>/models/`.
pub fn model_dir(cache_dir: &Path, def: &ModelDef) -> PathBuf {
    cache_dir.join("models").join(def.dir_name)
}

/// Has every required file already been downloaded for this model?
pub fn is_downloaded(cache_dir: &Path, def: &ModelDef) -> bool {
    let dir = model_dir(cache_dir, def);
    def.files.iter().all(|f| dir.join(f.name).exists())
}

pub struct SttEngine {
    recognizer: sherpa_onnx::OfflineRecognizer,
}

impl SttEngine {
    pub fn new(config: &Config) -> Result<Self> {
        let model_path = &config.model_path;
        let def = detect_in_dir(model_path).ok_or_else(|| {
            anyhow::anyhow!(
                "No recognized model files found in {:?}. Run `incant model use <name>` (or `incant-daemon download-model`) first.",
                model_path
            )
        })?;

        info!("Loading {} model from {:?}", def.name, model_path);
        match def.kind {
            ModelKind::Transducer => Self::load_transducer(model_path, config),
            ModelKind::Whisper => Self::load_whisper(model_path, config),
            ModelKind::Moonshine => Self::load_moonshine(model_path, config),
        }
    }

    fn load_moonshine(model_path: &Path, config: &Config) -> Result<Self> {
        let preprocessor = model_path.join("preprocess.onnx");
        let encoder = model_path.join("encode.int8.onnx");
        let cached_decoder = model_path.join("cached_decode.int8.onnx");
        let uncached_decoder = model_path.join("uncached_decode.int8.onnx");
        let tokens = model_path.join("tokens.txt");

        let mut recognizer_config = sherpa_onnx::OfflineRecognizerConfig::default();
        recognizer_config.model_config.moonshine = sherpa_onnx::OfflineMoonshineModelConfig {
            preprocessor: Some(preprocessor.to_string_lossy().into()),
            encoder: Some(encoder.to_string_lossy().into()),
            cached_decoder: Some(cached_decoder.to_string_lossy().into()),
            uncached_decoder: Some(uncached_decoder.to_string_lossy().into()),
            ..Default::default()
        };
        recognizer_config.model_config.tokens = Some(tokens.to_string_lossy().into());
        recognizer_config.model_config.provider = Some(detect_provider());
        recognizer_config.model_config.num_threads = resolve_num_threads(config.num_threads);
        recognizer_config.decoding_method = Some("greedy_search".into());

        let recognizer = sherpa_onnx::OfflineRecognizer::create(&recognizer_config)
            .ok_or_else(|| anyhow::anyhow!("creating Moonshine recognizer failed"))?;

        Ok(SttEngine { recognizer })
    }

    fn load_whisper(model_path: &Path, config: &Config) -> Result<Self> {
        let encoder = if model_path.join("turbo-encoder.onnx").exists() {
            model_path.join("turbo-encoder.onnx")
        } else {
            model_path.join("turbo-encoder.int8.onnx")
        };
        let decoder = if model_path.join("turbo-decoder.onnx").exists() {
            model_path.join("turbo-decoder.onnx")
        } else {
            model_path.join("turbo-decoder.int8.onnx")
        };
        let tokens = model_path.join("turbo-tokens.txt");

        let mut recognizer_config = sherpa_onnx::OfflineRecognizerConfig::default();
        recognizer_config.model_config.whisper = sherpa_onnx::OfflineWhisperModelConfig {
            encoder: Some(encoder.to_string_lossy().into()),
            decoder: Some(decoder.to_string_lossy().into()),
            ..Default::default()
        };
        recognizer_config.model_config.tokens = Some(tokens.to_string_lossy().into());
        recognizer_config.model_config.provider = Some(detect_provider());
        recognizer_config.model_config.num_threads = resolve_num_threads(config.num_threads);
        recognizer_config.decoding_method = Some("greedy_search".into());

        let recognizer = sherpa_onnx::OfflineRecognizer::create(&recognizer_config)
            .ok_or_else(|| anyhow::anyhow!("creating Whisper recognizer failed"))?;

        Ok(SttEngine { recognizer })
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

        let mut recognizer_config = sherpa_onnx::OfflineRecognizerConfig::default();
        recognizer_config.model_config.transducer = sherpa_onnx::OfflineTransducerModelConfig {
            encoder: Some(encoder.to_string_lossy().into()),
            decoder: Some(decoder.to_string_lossy().into()),
            joiner: Some(joiner.to_string_lossy().into()),
            ..Default::default()
        };
        recognizer_config.model_config.tokens = Some(tokens.to_string_lossy().into());
        recognizer_config.model_config.provider = Some(detect_provider());
        recognizer_config.model_config.num_threads = resolve_num_threads(config.num_threads);
        recognizer_config.decoding_method = Some("greedy_search".into());

        let recognizer = sherpa_onnx::OfflineRecognizer::create(&recognizer_config)
            .ok_or_else(|| anyhow::anyhow!("creating Transducer recognizer failed"))?;

        Ok(SttEngine { recognizer })
    }

    pub fn transcribe(&mut self, samples: &[f32], sample_rate: u32) -> Result<String> {
        let started = std::time::Instant::now();
        let audio_secs = samples.len() as f32 / sample_rate as f32;

        let stream = self.recognizer.create_stream();
        stream.accept_waveform(sample_rate as i32, samples);
        self.recognizer.decode(&stream);
        let result = stream
            .get_result()
            .ok_or_else(|| anyhow::anyhow!("getting transcription result failed"))?;

        let elapsed_ms = started.elapsed().as_millis();
        let rtf = if audio_secs > 0.0 {
            (elapsed_ms as f32 / 1000.0) / audio_secs
        } else {
            0.0
        };
        info!(
            "transcribe: {} ms for {:.2}s audio (RTF {:.2}x)",
            elapsed_ms, audio_secs, rtf
        );

        Ok(result.text)
    }
}

/// Resolve the configured thread count for ONNX Runtime.
///
/// `num_threads == 0` is the "auto" sentinel: pick a sensible default based on
/// the host's available parallelism. Anything else (positive) is taken as-is.
///
/// For Parakeet's encoder, ONNX Runtime scales well up to ~4–8 intra-op
/// threads on x86 CPUs; beyond that, contention with its own thread pool and
/// the audio / IPC threads makes things *slower*, not faster. So we cap at 8.
fn resolve_num_threads(configured: i32) -> i32 {
    if configured > 0 {
        return configured;
    }
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2);
    // Leave at least one core for the audio callback / overlay / IPC, and cap
    // at 8 where intra-op parallelism stops paying for itself.
    let auto = cores.saturating_sub(1).clamp(2, 8);
    tracing::info!(
        "STT threads: auto-selected {} (host has {} logical cores)",
        auto,
        cores
    );
    auto as i32
}

/// Detect whether CUDA is available for ONNX Runtime.
fn detect_provider() -> String {
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

/// Download the model selected by `model_path`'s basename (legacy entry point
/// used by `incant-daemon download-model`). Falls back to the first registry
/// entry (Parakeet) if the basename doesn't match any known model — preserves
/// pre-registry behavior where bare `download-model` pulled the default.
pub async fn download_model(cache_dir: &Path, model_path: &Path) -> Result<()> {
    let basename = model_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let def = MODELS
        .iter()
        .find(|m| m.dir_name == basename)
        .unwrap_or(&MODELS[0]);
    download_def(cache_dir, def).await
}

/// Download a model by friendly name. Used by `incant model use <name>` —
/// the daemon binary itself goes through `download_model`, so this looks
/// unused when the bin target is compiled alone.
#[allow(dead_code)]
pub async fn download_by_name(cache_dir: &Path, name: &str) -> Result<PathBuf> {
    let def = find_by_name(name).ok_or_else(|| {
        let known: Vec<&str> = MODELS.iter().map(|m| m.name).collect();
        anyhow::anyhow!("unknown model '{}'. Known: {}", name, known.join(", "))
    })?;
    download_def(cache_dir, def).await?;
    Ok(model_dir(cache_dir, def))
}

async fn download_def(cache_dir: &Path, def: &ModelDef) -> Result<()> {
    let dir = model_dir(cache_dir, def);
    if is_downloaded(cache_dir, def) {
        info!("{} model already exists at {:?}", def.name, dir);
        return Ok(());
    }
    info!("Downloading {} ({})...", def.name, def.description);
    download_model_files(&dir, def.repo, def.revision, def.files).await?;
    info!("{} model downloaded to {:?}", def.name, dir);
    Ok(())
}

/// Download every file in `files` from a pinned HuggingFace revision, verifying
/// SHA-256 and size after each transfer. Existing files matching the expected
/// hash are kept; mismatches are deleted and re-downloaded.
async fn download_model_files(
    model_dir: &Path,
    repo: &str,
    revision: &str,
    files: &[ModelFile],
) -> Result<()> {
    std::fs::create_dir_all(model_dir)
        .with_context(|| format!("creating model dir {:?}", model_dir))?;

    for f in files {
        let dest = model_dir.join(f.name);

        if dest.exists() {
            match verify_file(&dest, f) {
                Ok(()) => {
                    info!("{} present and verified, skipping", f.name);
                    continue;
                }
                Err(e) => {
                    tracing::warn!("{} failed verification ({}); re-downloading", f.name, e);
                    let _ = std::fs::remove_file(&dest);
                }
            }
        }

        let url = format!(
            "https://huggingface.co/{}/resolve/{}/{}",
            repo, revision, f.name
        );
        if let Err(e) = download_file_with_resume(&url, &dest, f).await {
            let part = part_path(&dest);
            let _ = std::fs::remove_file(part);
            return Err(e);
        }
    }

    Ok(())
}

fn part_path(dest: &Path) -> std::path::PathBuf {
    dest.with_file_name(format!(
        "{}.__part__",
        dest.file_name().unwrap_or_default().to_string_lossy()
    ))
}

/// SHA-256 the file at `path` and confirm it matches `expected`.
fn verify_file(path: &Path, expected: &ModelFile) -> Result<()> {
    let meta = std::fs::metadata(path).with_context(|| format!("stat {:?}", path))?;
    if meta.len() != expected.size {
        anyhow::bail!(
            "size mismatch for {:?}: expected {} bytes, got {}",
            path,
            expected.size,
            meta.len()
        );
    }

    let mut file =
        std::fs::File::open(path).with_context(|| format!("opening {:?} for checksum", path))?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)
        .with_context(|| format!("reading {:?} for checksum", path))?;
    let got = hex::encode(hasher.finalize());

    if got != expected.sha256 {
        anyhow::bail!(
            "sha256 mismatch for {:?}: expected {}, got {}",
            path,
            expected.sha256,
            got
        )
    }
    Ok(())
}

/// Download a file with resume support via HTTP Range requests. Writes to
/// `dest.__part__`, verifies SHA-256 + size, then atomically renames to `dest`.
async fn download_file_with_resume(url: &str, dest: &Path, expected: &ModelFile) -> Result<()> {
    let part = part_path(dest);

    let existing_size = if part.exists() {
        part.metadata()?.len()
    } else {
        0
    };

    let existing_size = if existing_size > expected.size {
        let _ = std::fs::remove_file(&part);
        0
    } else {
        existing_size
    };

    let client = reqwest::Client::new();
    let mut request = client.get(url);

    if existing_size > 0 {
        info!("Resuming {} from {} bytes", expected.name, existing_size);
        request = request.header("Range", format!("bytes={}-", existing_size));
    } else {
        info!("Downloading {} ...", expected.name);
    }

    let mut response = request.send().await?;
    let status = response.status();

    if !status.is_success() && status != reqwest::StatusCode::PARTIAL_CONTENT {
        anyhow::bail!("Failed to download {}: {}", url, status);
    }

    let resumed = status == reqwest::StatusCode::PARTIAL_CONTENT;
    let mut downloaded = if resumed { existing_size } else { 0 };

    let progress = ProgressBar::new(expected.size);
    progress.set_style(
        ProgressStyle::with_template(
            "  {msg:<28} [{bar:30.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})",
        )
        .unwrap()
        .progress_chars("=>-"),
    );
    progress.set_message(expected.name.to_string());
    progress.set_position(downloaded);

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(!resumed)
        .append(resumed)
        .write(true)
        .open(&part)
        .with_context(|| format!("opening part file {:?}", part))?;

    while let Some(chunk) = response.chunk().await? {
        std::io::Write::write_all(&mut file, &chunk)?;
        downloaded += chunk.len() as u64;
        progress.set_position(downloaded);
    }
    progress.finish_and_clear();

    verify_file(&part, expected)
        .with_context(|| format!("verifying downloaded {}", expected.name))?;

    std::fs::rename(&part, dest).with_context(|| format!("renaming {:?} to {:?}", part, dest))?;

    info!(
        "Downloaded and verified {} ({} bytes)",
        expected.name, downloaded
    );
    Ok(())
}
