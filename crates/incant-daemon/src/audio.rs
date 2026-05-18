use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tracing::{debug, error, info, warn};

/// Start capturing audio from the default input device.
/// Chunks of mono f32 samples at the *native* sample rate are sent via `tx`
/// while `recording` is true.
/// Returns the cpal Stream (keep it alive to keep recording) and the native sample rate.
pub fn start_capture(
    recording: Arc<AtomicBool>,
    tx: tokio::sync::mpsc::Sender<Vec<f32>>,
    target_sample_rate: u32,
) -> Result<(cpal::Stream, u32)> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .context("no default input device found")?;

    info!("Using audio input device: {}", device.name().unwrap_or_default());

    let mut supported_configs = device
        .supported_input_configs()
        .context("failed to query supported input configs")?;

    // Try to find a config that matches our target sample rate and f32 format.
    let mut chosen_config = None;
    for config_range in supported_configs.by_ref() {
        if config_range.sample_format() == SampleFormat::F32 {
            let min_sr = config_range.min_sample_rate().0;
            let max_sr = config_range.max_sample_rate().0;
            if target_sample_rate >= min_sr && target_sample_rate <= max_sr {
                chosen_config = Some(config_range.with_sample_rate(cpal::SampleRate(target_sample_rate)));
                break;
            }
        }
    }

    // Fallback: pick any supported config and resample later.
    let config = if let Some(cfg) = chosen_config {
        info!("Using native {} Hz f32 capture", target_sample_rate);
        cfg
    } else {
        let fallback = device
            .default_input_config()
            .context("no default input config")?;
        warn!(
            "Target sample rate {} not supported natively, using {} Hz (will resample)",
            target_sample_rate,
            fallback.sample_rate().0
        );
        fallback
    };

    let sample_rate = config.sample_rate().0;
    let channels = config.channels();

    let stream = match config.sample_format() {
        SampleFormat::F32 => build_stream::<f32>(
            &device,
            &config.into(),
            recording,
            tx,
            channels,
        ),
        SampleFormat::I16 => build_stream::<i16>(
            &device,
            &config.into(),
            recording,
            tx.clone(),
            channels,
        ),
        SampleFormat::U16 => build_stream::<u16>(
            &device,
            &config.into(),
            recording,
            tx.clone(),
            channels,
        ),
        _ => {
            anyhow::bail!("unsupported sample format: {:?}", config.sample_format());
        }
    }
    .context("failed to build audio stream")?;

    stream.play().context("failed to start audio stream")?;
    debug!("Audio stream started");

    Ok((stream, sample_rate))
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    recording: Arc<AtomicBool>,
    tx: tokio::sync::mpsc::Sender<Vec<f32>>,
    channels: u16,
) -> Result<cpal::Stream>
where
    T: cpal::SizedSample + cpal::FromSample<T> + dasp_sample::ToSample<f32>,
{
    let err_fn = |err| error!("audio stream error: {}", err);

    let stream = device.build_input_stream(
        config,
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            if !recording.load(Ordering::Relaxed) {
                return;
            }

            // Convert to f32 and downmix to mono.
            let mono_samples: Vec<f32> = data
                .chunks(channels as usize)
                .map(|chunk| {
                    let sum: f32 = chunk.iter().map(|s| s.to_sample::<f32>()).sum();
                    sum / channels as f32
                })
                .collect();

            // Send raw native-rate samples to async consumer.
            let _ = tx.try_send(mono_samples);
        },
        err_fn,
        None,
    )?;

    Ok(stream)
}

/// Resample a buffer of f32 samples using high-quality sinc interpolation.
/// This is CPU-intensive and should be called from `spawn_blocking`.
pub fn resample_once(input: &[f32], from_rate: u32, to_rate: u32) -> Result<Vec<f32>> {
    use rubato::{
        Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
    };

    if from_rate == to_rate {
        return Ok(input.to_vec());
    }

    let ratio = to_rate as f64 / from_rate as f64;
    let chunk_size = 1024;
    let params = SincInterpolationParameters {
        sinc_len: 128,
        f_cutoff: 0.95,
        interpolation: SincInterpolationType::Linear,
        oversampling_factor: 128,
        window: WindowFunction::BlackmanHarris2,
    };
    let mut resampler =
        SincFixedIn::new(ratio, ratio * 1.2, params, chunk_size, 1)
            .context("creating sinc resampler")?;

    let mut output = Vec::with_capacity((input.len() as f64 * ratio) as usize);
    let mut pos = 0;

    while pos + chunk_size <= input.len() {
        let chunk = &input[pos..pos + chunk_size];
        let out = resampler
            .process(&[chunk], None)
            .context("resampling chunk")?;
        output.extend_from_slice(&out[0]);
        pos += chunk_size;
    }

    if pos < input.len() {
        let remaining = &input[pos..];
        let out = resampler
            .process_partial(Some(&[remaining]), None)
            .context("resampling final chunk")?;
        output.extend_from_slice(&out[0]);
    }

    Ok(output)
}

/// Save a buffer of f32 samples to a WAV file for debugging.
pub fn save_wav(path: &std::path::Path, samples: &[f32], sample_rate: u32) -> Result<()> {
    use hound::{WavSpec, WavWriter};
    let spec = WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = WavWriter::create(path, spec).context("create wav file")?;
    for sample in samples {
        let clipped = sample.max(-1.0).min(1.0);
        let int_sample = (clipped * i16::MAX as f32) as i16;
        writer.write_sample(int_sample)?;
    }
    writer.finalize().context("finalize wav")?;
    Ok(())
}
