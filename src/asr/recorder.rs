use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::io::Write;
use std::path::Path;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tracing::{debug, info};

pub struct AudioRecorder {
    is_recording: Arc<AtomicBool>,
    buffer_16k_mono: Arc<Mutex<Vec<i16>>>,
}

impl AudioRecorder {
    pub fn new() -> Self {
        Self {
            is_recording: Arc::new(AtomicBool::new(false)),
            buffer_16k_mono: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// 开始录音：自动将声卡采样率与声道数混音并重采样为 16kHz 单声道 16-bit PCM
    pub fn start(&self) -> Result<()> {
        self.buffer_16k_mono.lock().unwrap().clear();
        self.is_recording.store(true, Ordering::SeqCst);

        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .context("No default audio input device found")?;

        let supported_config = device
            .default_input_config()
            .context("Failed to get default audio input config")?;

        let in_rate = supported_config.sample_rate().0;
        let in_channels = supported_config.channels() as usize;
        let sample_format = supported_config.sample_format();

        info!(
            "Starting audio capture on device: {:?} ({} Hz, {} ch, {:?})",
            device.name().unwrap_or_default(),
            in_rate,
            in_channels,
            sample_format
        );

        let is_recording = self.is_recording.clone();
        let buffer_16k_mono = self.buffer_16k_mono.clone();

        std::thread::spawn(move || {
            let err_fn = |err| tracing::error!("An error occurred on audio stream: {}", err);
            let raw_mono = Arc::new(Mutex::new(Vec::<f32>::new()));
            let raw_mono_cb = raw_mono.clone();
            let is_rec_cb = is_recording.clone();

            let stream_config: cpal::StreamConfig = supported_config.into();

            let stream = match sample_format {
                cpal::SampleFormat::I16 => device.build_input_stream(
                    &stream_config,
                    move |data: &[i16], _: &_| {
                        if is_rec_cb.load(Ordering::SeqCst) {
                            let mut mono = raw_mono_cb.lock().unwrap();
                            for frame in data.chunks(in_channels) {
                                let sum: f32 =
                                    frame.iter().map(|&s| s as f32 / 32768.0).sum();
                                mono.push(sum / frame.len() as f32);
                            }
                        }
                    },
                    err_fn,
                    None,
                ),
                cpal::SampleFormat::F32 => device.build_input_stream(
                    &stream_config,
                    move |data: &[f32], _: &_| {
                        if is_rec_cb.load(Ordering::SeqCst) {
                            let mut mono = raw_mono_cb.lock().unwrap();
                            for frame in data.chunks(in_channels) {
                                let sum: f32 = frame.iter().copied().sum();
                                mono.push(sum / frame.len() as f32);
                            }
                        }
                    },
                    err_fn,
                    None,
                ),
                _ => {
                    tracing::error!("Unsupported sample format: {:?}", sample_format);
                    return;
                }
            };

            match stream {
                Ok(s) => {
                    if let Err(e) = s.play() {
                        tracing::error!("Failed to start audio stream: {:?}", e);
                        return;
                    }
                    while is_recording.load(Ordering::SeqCst) {
                        std::thread::sleep(std::time::Duration::from_millis(30));
                    }
                    drop(s);

                    // 线性重采样至 16000 Hz
                    let src = raw_mono.lock().unwrap();
                    let out = resample_to_16k(&src, in_rate);
                    *buffer_16k_mono.lock().unwrap() = out;
                }
                Err(e) => {
                    tracing::error!("Failed to build input stream: {:?}", e);
                }
            }
        });

        Ok(())
    }

    /// 停止录音并取出 16kHz 单声道 i16 PCM 数据
    pub fn stop(&self) -> Vec<i16> {
        self.is_recording.store(false, Ordering::SeqCst);
        // 等待录音线程完成重采样写入
        std::thread::sleep(std::time::Duration::from_millis(60));
        let data = self.buffer_16k_mono.lock().unwrap().clone();
        debug!("Stopped audio capture. Captured {} samples (16kHz mono).", data.len());
        data
    }
}

/// 将任意采样率单声道 f32 信号线性重采样为 16000 Hz i16 PCM
fn resample_to_16k(input: &[f32], in_rate: u32) -> Vec<i16> {
    if input.is_empty() {
        return Vec::new();
    }
    let out_rate = 16000u32;
    if in_rate == out_rate {
        return input
            .iter()
            .map(|&s| (s.clamp(-1.0, 1.0) * 32767.0) as i16)
            .collect();
    }

    let ratio = in_rate as f64 / out_rate as f64;
    let out_len = (input.len() as f64 / ratio) as usize;
    let mut output = Vec::with_capacity(out_len);

    for i in 0..out_len {
        let src_pos = i as f64 * ratio;
        let idx = src_pos as usize;
        let frac = (src_pos - idx as f64) as f32;
        let s0 = input[idx];
        let s1 = if idx + 1 < input.len() {
            input[idx + 1]
        } else {
            s0
        };
        let interpolated = s0 + frac * (s1 - s0);
        output.push((interpolated.clamp(-1.0, 1.0) * 32767.0) as i16);
    }
    output
}

/// 将 16kHz 单声道 i16 PCM 保存为标准 WAV 文件
pub fn save_wav_16k_mono<P: AsRef<Path>>(path: P, pcm_data: &[i16]) -> Result<()> {
    let mut file = std::fs::File::create(path.as_ref())
        .with_context(|| format!("Failed to create WAV file at {:?}", path.as_ref()))?;

    let sample_rate = 16000u32;
    let num_channels = 1u16;
    let bits_per_sample = 16u16;
    let byte_rate = sample_rate * num_channels as u32 * (bits_per_sample as u32 / 8);
    let block_align = num_channels * (bits_per_sample / 8);
    let data_size = (pcm_data.len() * 2) as u32;
    let riff_size = 36 + data_size;

    file.write_all(b"RIFF")?;
    file.write_all(&riff_size.to_le_bytes())?;
    file.write_all(b"WAVE")?;
    file.write_all(b"fmt ")?;
    file.write_all(&16u32.to_le_bytes())?; // PCM chunk size
    file.write_all(&1u16.to_le_bytes())?; // PCM format = 1
    file.write_all(&num_channels.to_le_bytes())?;
    file.write_all(&sample_rate.to_le_bytes())?;
    file.write_all(&byte_rate.to_le_bytes())?;
    file.write_all(&block_align.to_le_bytes())?;
    file.write_all(&bits_per_sample.to_le_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&data_size.to_le_bytes())?;

    for &sample in pcm_data {
        file.write_all(&sample.to_le_bytes())?;
    }
    Ok(())
}
