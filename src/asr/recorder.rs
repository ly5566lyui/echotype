use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::io::Write;
use std::path::Path;
use std::sync::{
    atomic::{AtomicBool, AtomicU32, Ordering},
    Arc, Mutex,
};
use tracing::{debug, info};

/// 单次录音时长上限 (秒)：防止忘记按停止键导致持续录音/识别爆内存
pub const MAX_RECORD_SECS: u64 = 60;

/// 静音判定 RMS 阈值 (约 -42dBFS)
const SILENCE_THRESHOLD: f64 = 250.0;
/// 能量检测帧长 (毫秒 @16kHz)
const FRAME_MS: usize = 20;
/// 裁剪时保留的前后呼吸空间 (毫秒)
const PAD_MS: usize = 100;

/// 能量法静音裁剪：去掉首尾静音段 (VAD-lite)，降低噪声误识别并缩短上传体积。
/// 全静音输入返回空向量；短于单帧的输入原样返回。
pub fn trim_silence(pcm: &[i16]) -> Vec<i16> {
    let frame = 16000 / 1000 * FRAME_MS;
    if pcm.len() <= frame {
        return pcm.to_vec();
    }

    let frame_rms = |seg: &[i16]| -> f64 {
        let n = seg.len() as f64;
        (seg.iter().map(|&s| (s as f64) * (s as f64)).sum::<f64>() / n).sqrt()
    };

    let mut first: Option<usize> = None;
    let mut last: Option<usize> = None;
    for (i, chunk) in pcm.chunks(frame).enumerate() {
        if frame_rms(chunk) > SILENCE_THRESHOLD {
            if first.is_none() {
                first = Some(i);
            }
            last = Some(i);
        }
    }

    let (Some(first), Some(last)) = (first, last) else {
        return Vec::new(); // 整段静音
    };

    let pad = 16000 / 1000 * PAD_MS;
    let start = (first * frame).saturating_sub(pad);
    let end = ((last + 1) * frame + pad).min(pcm.len());
    pcm[start..end].to_vec()
}

/// 静音自动结束: 语音能量检测使用与 trim_silence 一致的阈值 (f32 域)
const SPEECH_RMS_F32: f32 = (SILENCE_THRESHOLD / 32768.0) as f32;
/// 判定"已开始说话"所需的能量帧数 (每帧约为一个回调块, 累计约 60ms)
const SPEECH_FRAMES_REQUIRED: u32 = 3;

#[derive(Clone)]
pub struct AudioRecorder {
    is_recording: Arc<AtomicBool>,
    /// 录音线程完成重采样写入的标志 (stop 依据它等待，消除固定 sleep 竞态)
    finished: Arc<AtomicBool>,
    buffer_16k_mono: Arc<Mutex<Vec<i16>>>,
    /// 累计检测到的有声能量块数 (达到阈值即认为"已开始说话")
    loud_blocks: Arc<AtomicU32>,
    /// 是否已开始说话
    speech_detected: Arc<AtomicBool>,
    /// 说话后的持续静音时长 (毫秒)
    silence_ms: Arc<AtomicU32>,
}

impl AudioRecorder {
    pub fn new() -> Self {
        Self {
            is_recording: Arc::new(AtomicBool::new(false)),
            finished: Arc::new(AtomicBool::new(true)),
            buffer_16k_mono: Arc::new(Mutex::new(Vec::new())),
            loud_blocks: Arc::new(AtomicU32::new(0)),
            speech_detected: Arc::new(AtomicBool::new(false)),
            silence_ms: Arc::new(AtomicU32::new(0)),
        }
    }

    /// 静音自动结束判定: 已开始说话且随后持续静音达到 timeout_ms
    pub fn silence_auto_stop_reached(&self, timeout_ms: u32) -> bool {
        self.speech_detected.load(Ordering::SeqCst)
            && self.silence_ms.load(Ordering::SeqCst) >= timeout_ms
    }

    /// 在录音回调末尾更新能量状态 (每回调块的时长约 chunk_ms 毫秒)
    fn note_energy(&self, rms: f32, chunk_ms: u32) {
        if rms > SPEECH_RMS_F32 {
            self.loud_blocks.fetch_add(1, Ordering::Relaxed);
            self.speech_detected.store(
                self.loud_blocks.load(Ordering::Relaxed) >= SPEECH_FRAMES_REQUIRED,
                Ordering::SeqCst,
            );
            self.silence_ms.store(0, Ordering::SeqCst);
        } else if self.speech_detected.load(Ordering::SeqCst) {
            self.silence_ms.fetch_add(chunk_ms, Ordering::Relaxed);
        }
    }

    /// 开始录音：自动将声卡采样率与声道数混音并重采样为 16kHz 单声道 16-bit PCM
    pub fn start(&self) -> Result<()> {
        self.buffer_16k_mono.lock().unwrap().clear();
        self.is_recording.store(true, Ordering::SeqCst);
        self.finished.store(false, Ordering::SeqCst);
        self.loud_blocks.store(0, Ordering::SeqCst);
        self.speech_detected.store(false, Ordering::SeqCst);
        self.silence_ms.store(0, Ordering::SeqCst);

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
        let finished_cb = self.finished.clone();
        let rec_cb = Arc::new(self.clone());

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
                            let prev_len = mono.len();
                            for frame in data.chunks(in_channels) {
                                let sum: f32 =
                                    frame.iter().map(|&s| s as f32 / 32768.0).sum();
                                mono.push(sum / frame.len() as f32);
                            }
                            // 实时能量检测 (静音自动结束用)
                            let new = &mono[prev_len..];
                            let n = new.len().max(1);
                            let rms = (new.iter().map(|s| s * s).sum::<f32>() / n as f32).sqrt();
                            let chunk_ms = (data.len() as u32 * 1000
                                / (in_channels as u32 * in_rate.max(1) as u32))
                            .max(1);
                            rec_cb.note_energy(rms, chunk_ms);
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
                            let prev_len = mono.len();
                            for frame in data.chunks(in_channels) {
                                let sum: f32 = frame.iter().copied().sum();
                                mono.push(sum / frame.len() as f32);
                            }
                            // 实时能量检测 (静音自动结束用)
                            let new = &mono[prev_len..];
                            let n = new.len().max(1);
                            let rms = (new.iter().map(|s| s * s).sum::<f32>() / n as f32).sqrt();
                            let chunk_ms = (data.len() as u32 * 1000
                                / (in_channels as u32 * in_rate.max(1) as u32))
                            .max(1);
                            rec_cb.note_energy(rms, chunk_ms);
                        }
                    },
                    err_fn,
                    None,
                ),
                _ => {
                    tracing::error!("Unsupported sample format: {:?}", sample_format);
                    finished_cb.store(true, Ordering::SeqCst);
                    return;
                }
            };

            match stream {
                Ok(s) => {
                    if let Err(e) = s.play() {
                        tracing::error!("Failed to start audio stream: {:?}", e);
                        finished_cb.store(true, Ordering::SeqCst);
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
                    finished_cb.store(true, Ordering::SeqCst);
                }
                Err(e) => {
                    tracing::error!("Failed to build input stream: {:?}", e);
                    finished_cb.store(true, Ordering::SeqCst);
                }
            }
        });

        Ok(())
    }

    /// 停止录音并取出 16kHz 单声道 i16 PCM 数据。
    /// 等待录音线程完成重采样写入 (上限 2 秒)，消除固定 sleep 猜测带来的竞态。
    pub fn stop(&self) -> Vec<i16> {
        self.is_recording.store(false, Ordering::SeqCst);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !self.finished.load(Ordering::SeqCst) && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let data = self.buffer_16k_mono.lock().unwrap().clone();
        debug!(
            "Stopped audio capture. Captured {} samples (16kHz mono).",
            data.len()
        );
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
pub fn save_wav_16k_mono<P: AsRef<Path>>(path: P, pcm_data: &[i16]) -> Result<()> {    let mut file = std::fs::File::create(path.as_ref())
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 生成 440Hz 正弦波测试信号
    fn sine(amplitude: i16, samples: usize) -> Vec<i16> {
        (0..samples)
            .map(|i| {
                (amplitude as f64
                    * (2.0 * std::f64::consts::PI * 440.0 * i as f64 / 16000.0).sin())
                    as i16
            })
            .collect()
    }

    #[test]
    fn trims_leading_and_trailing_silence() {
        let mut pcm = vec![0i16; 4800]; // 0.3s 静音
        pcm.extend(sine(8000, 4800)); // 0.3s "语音"
        pcm.extend(vec![0i16; 4800]); // 0.3s 静音

        let trimmed = trim_silence(&pcm);
        assert!(trimmed.len() < pcm.len(), "should trim silence");
        // 保留段应包含完整语音主体与 100ms 前后呼吸空间
        assert!(trimmed.len() >= 4800, "should keep the speech body");
    }

    #[test]
    fn all_silence_becomes_empty() {
        assert!(trim_silence(&vec![0i16; 3200]).is_empty());
    }

    #[test]
    fn short_input_passthrough() {
        let pcm = vec![100i16; 100];
        assert_eq!(trim_silence(&pcm), pcm);
    }

    #[test]
    fn speech_in_middle_kept_intact() {
        let mut pcm = vec![0i16; 1600]; // 0.1s 静音
        pcm.extend(sine(8000, 16000)); // 1.0s "语音"
        pcm.extend(vec![0i16; 1600]); // 0.1s 静音

        let trimmed = trim_silence(&pcm);
        assert!(trimmed.len() > 16000, "speech body must survive");
    }

    #[test]
    fn silence_auto_stop_state_machine() {
        // 未说话: 无论静音多久都不触发
        let rec = AudioRecorder::new();
        for _ in 0..200 {
            rec.note_energy(0.0, 20);
        }
        assert!(!rec.silence_auto_stop_reached(1800));

        // 说话后: 静音累计达到阈值才触发
        let rec = AudioRecorder::new();
        for _ in 0..5 {
            rec.note_energy(0.05, 20); // 5 个响块 -> 判定已开始说话
        }
        assert!(!rec.silence_auto_stop_reached(1800)); // 刚说完话，还没有静音
        for _ in 0..89 {
            rec.note_energy(0.0, 20); // 累计 1780ms 静音，尚未达标
        }
        assert!(!rec.silence_auto_stop_reached(1800));
        rec.note_energy(0.0, 20); // 累计 1800ms，达标
        assert!(rec.silence_auto_stop_reached(1800));

        // 说话会重置静音计时
        rec.note_energy(0.05, 20);
        assert!(!rec.silence_auto_stop_reached(1800));
    }

    #[test]
    fn auto_stop_requires_sustained_speech() {
        // 单个噪声尖峰不足以判定"已开始说话" (要求至少 3 个响块)
        let rec = AudioRecorder::new();
        rec.note_energy(0.5, 20);
        rec.note_energy(0.0, 20);
        for _ in 0..200 {
            rec.note_energy(0.0, 20);
        }
        assert!(!rec.silence_auto_stop_reached(1800));
    }
}
