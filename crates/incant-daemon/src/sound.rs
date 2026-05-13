use rodio::{OutputStream, Sink, Source};
use std::io::Cursor;
use std::sync::mpsc::{channel, Sender};
use tracing::warn;

pub struct SoundEffects {
    tx: Sender<(Effect, f32)>,
}

impl SoundEffects {
    pub fn new(volume: f32) -> Option<Self> {
        let (tx, rx) = channel::<(Effect, f32)>();

        std::thread::spawn(move || {
            let (stream, stream_handle) = OutputStream::try_default().ok()?;
            let sink = Sink::try_new(&stream_handle).ok()?;

            let start_sound = generate_beep(880.0, 0.08);
            let stop_sound = generate_beep(440.0, 0.08);
            let paste_sound = generate_chime();
            let cancel_sound = generate_beep(220.0, 0.15);

            while let Ok((effect, vol)) = rx.recv() {
                let bytes = match effect {
                    Effect::Start => &start_sound,
                    Effect::Stop => &stop_sound,
                    Effect::Paste => &paste_sound,
                    Effect::Cancel => &cancel_sound,
                };

                let cursor = Cursor::new(bytes.clone());
                match rodio::Decoder::new(cursor) {
                    Ok(source) => {
                        let source = source.amplify(vol);
                        sink.append(source);
                    }
                    Err(e) => warn!("Failed to decode sound effect: {}", e),
                }
            }

            Some(())
        });

        Some(Self { tx })
    }

    pub fn play(&self, effect: Effect, volume: f32) {
        let _ = self.tx.send((effect, volume));
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Effect {
    Start,
    Stop,
    Paste,
    Cancel,
}

/// Generate a simple sine-wave beep as a WAV file in memory.
fn generate_beep(freq: f32, duration_secs: f32) -> Vec<u8> {
    let sample_rate = 44100u32;
    let num_samples = (sample_rate as f32 * duration_secs) as usize;
    let mut samples: Vec<i16> = Vec::with_capacity(num_samples);

    for i in 0..num_samples {
        let t = i as f32 / sample_rate as f32;
        let envelope = if t < 0.01 {
            t / 0.01 // attack
        } else if t > duration_secs - 0.01 {
            (duration_secs - t) / 0.01 // decay
        } else {
            1.0
        };
        let sample = (t * freq * 2.0 * std::f32::consts::PI).sin() * envelope;
        samples.push((sample * i16::MAX as f32) as i16);
    }

    write_wav(&samples, sample_rate)
}

/// Generate a pleasant two-tone chime.
fn generate_chime() -> Vec<u8> {
    let sample_rate = 44100u32;
    let duration = 0.3f32;
    let num_samples = (sample_rate as f32 * duration) as usize;
    let mut samples: Vec<i16> = Vec::with_capacity(num_samples);

    for i in 0..num_samples {
        let t = i as f32 / sample_rate as f32;
        let envelope = (-t * 6.0).exp().max(0.0); // exponential decay
        let f1 = (t * 880.0 * 2.0 * std::f32::consts::PI).sin();
        let f2 = (t * 1100.0 * 2.0 * std::f32::consts::PI).sin();
        let sample = (f1 * 0.5 + f2 * 0.5) * envelope;
        samples.push((sample * i16::MAX as f32) as i16);
    }

    write_wav(&samples, sample_rate)
}

fn write_wav(samples: &[i16], sample_rate: u32) -> Vec<u8> {
    let mut buf = Vec::new();
    let cursor = Cursor::new(&mut buf);
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::new(cursor, spec).unwrap();
    for &sample in samples {
        writer.write_sample(sample).unwrap();
    }
    writer.finalize().unwrap();
    buf
}
