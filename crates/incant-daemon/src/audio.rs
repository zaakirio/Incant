use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tracing::{debug, error, info, warn};

/// Start capturing audio from the default input device.
/// Samples are appended to `buffer` while `recording` is true.
/// Returns the cpal Stream (keep it alive to keep recording).
pub fn start_capture(
    recording: Arc<AtomicBool>,
    buffer: Arc<Mutex<Vec<f32>>>,
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
    let (config, needs_resample) = if let Some(cfg) = chosen_config {
        info!("Using native {} Hz f32 capture", target_sample_rate);
        (cfg, false)
    } else {
        let fallback = device
            .default_input_config()
            .context("no default input config")?;
        warn!(
            "Target sample rate {} not supported natively, using {} Hz with resampling",
            target_sample_rate,
            fallback.sample_rate().0
        );
        (fallback, true)
    };

    let sample_rate = config.sample_rate().0;
    let channels = config.channels();

    let stream = match config.sample_format() {
        SampleFormat::F32 => build_stream::<f32>(
            &device,
            &config.into(),
            recording,
            buffer,
            channels,
            sample_rate,
            target_sample_rate,
            needs_resample,
        ),
        SampleFormat::I16 => build_stream::<i16>(
            &device,
            &config.into(),
            recording,
            buffer,
            channels,
            sample_rate,
            target_sample_rate,
            needs_resample,
        ),
        SampleFormat::U16 => build_stream::<u16>(
            &device,
            &config.into(),
            recording,
            buffer,
            channels,
            sample_rate,
            target_sample_rate,
            needs_resample,
        ),
        _ => {
            anyhow::bail!("unsupported sample format: {:?}", config.sample_format());
        }
    }
    .context("failed to build audio stream")?;

    stream.play().context("failed to start audio stream")?;
    debug!("Audio stream started");

    Ok((stream, if needs_resample { target_sample_rate } else { sample_rate }))
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    recording: Arc<AtomicBool>,
    buffer: Arc<Mutex<Vec<f32>>>,
    channels: u16,
    sample_rate: u32,
    target_sample_rate: u32,
    needs_resample: bool,
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

            let final_samples = if needs_resample && sample_rate != target_sample_rate {
                // Simple linear resampling for now.
                // For production, use rubato::SincFixedIn.
                resample_linear(&mono_samples, sample_rate, target_sample_rate)
            } else {
                mono_samples
            };

            // Append directly (mutex is held briefly).
            buffer.lock().unwrap().extend_from_slice(&final_samples);
        },
        err_fn,
        None,
    )?;

    Ok(stream)
}

fn resample_linear(input: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate {
        return input.to_vec();
    }
    let ratio = to_rate as f64 / from_rate as f64;
    let output_len = (input.len() as f64 * ratio) as usize;
    let mut output = Vec::with_capacity(output_len);
    for i in 0..output_len {
        let src_idx = i as f64 / ratio;
        let src_floor = src_idx.floor() as usize;
        let src_ceil = (src_floor + 1).min(input.len() - 1);
        let frac = src_idx - src_floor as f64;
        let sample = input[src_floor] * (1.0 - frac as f32) + input[src_ceil] * frac as f32;
        output.push(sample);
    }
    output
}

/// Save a buffer of f32 samples (assumed 16kHz) to a WAV file for debugging.
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
