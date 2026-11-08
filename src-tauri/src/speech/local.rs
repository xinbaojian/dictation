//! 本地语音合成：sherpa-onnx 引擎懒加载 + 按内容哈希的音频缓存 + rodio 播放。
//!
//! - 引擎按模型懒加载后常驻（模型大，加载要 1~3 秒；`Arc<OfflineTts>` 按需重建）
//! - 合成结果缓存到 `tts-cache/<sha256(model|sid|text)>.wav`：复习页反复读同一个词
//!   时秒回；缓存总量超上限按最旧淘汰
//! - 播放在独立音频线程：rodio 的 `OutputStream` 不保证跨线程移动，
//!   通道消息（Play/Stop）让它始终只活在那一个线程上，「先停上一句」靠换 Sink 实现

use crate::error::{AppError, AppResult};
use crate::tts::models::{self, ModelSpec};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};

/// 缓存上限（字节）。一段词约 100~300KB，64MB 足够几百个词
const CACHE_LIMIT: u64 = 64 * 1024 * 1024;

/// 播放请求带世代号：快速翻词时合成完成顺序会乱（缓存命中毫秒级、
/// 首次建引擎数秒），旧请求的音频到达时若已有更新的请求 / stop，直接丢弃
enum PlayMsg {
    Play { seq: u64, samples: Vec<f32>, rate: u32 },
    Stop { seq: u64 },
}

/// 过期判定（纯函数便于单测）：只有不早于当前最新世代的消息才生效
fn is_current(latest: u64, seq: u64) -> bool {
    seq >= latest
}

pub struct LocalTts {
    data_dir: PathBuf,
    /// 已加载引擎（模型 id + 引擎）。换模型 / 删除模型后自动重建
    engine: Mutex<Option<(&'static str, Arc<sherpa_onnx::OfflineTts>)>>,
    sender: Sender<PlayMsg>,
    seq: AtomicU64,
}

impl LocalTts {
    pub fn new(data_dir: PathBuf) -> Self {
        let (tx, rx) = mpsc::channel::<PlayMsg>();
        std::thread::Builder::new()
            .name("tts-audio".into())
            .spawn(move || audio_loop(rx))
            .expect("启动音频线程失败");
        Self {
            data_dir,
            engine: Mutex::new(None),
            sender: tx,
            seq: AtomicU64::new(0),
        }
    }

    fn next_seq(&self) -> u64 {
        self.seq.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// 取指定模型的引擎；模型变了（换语音包 / 删除重装）就丢弃旧引擎重建。
    fn engine(&self, spec: &'static ModelSpec) -> AppResult<Arc<sherpa_onnx::OfflineTts>> {
        let mut guard = self.engine.lock().map_err(|_| AppError::msg("语音组件状态异常"))?;
        if let Some((id, tts)) = guard.as_ref() {
            if *id == spec.id {
                return Ok(tts.clone());
            }
        }
        let dir = models::model_dir(&self.data_dir, spec.id);
        let config = models::build_config(spec, &dir);
        let tts = sherpa_onnx::OfflineTts::create(&config).ok_or_else(|| {
            AppError::msg("本地语音引擎初始化失败（语音包可能损坏，可在设置里删除后重新下载）")
        })?;
        let tts = Arc::new(tts);
        *guard = Some((spec.id, tts.clone()));
        Ok(tts)
    }

    /// 合成（优先缓存）。返回 (f32 样本, 采样率)
    fn synth(&self, spec: &'static ModelSpec, text: &str) -> AppResult<(Vec<f32>, u32)> {
        let key = cache_key(spec.id, spec.sid, text);
        let cache_dir = self.data_dir.join("tts-cache");
        let cache_file = cache_dir.join(format!("{key}.wav"));
        if let Some((samples, rate)) = read_wav(&cache_file) {
            if !samples.is_empty() {
                return Ok((samples, rate));
            }
        }

        let tts = self.engine(spec)?;
        let gen = sherpa_onnx::GenerationConfig {
            sid: spec.sid,
            speed: 1.0,
            ..Default::default()
        };
        // 无进度回调：词句很短，合成是整体完成的
        let audio = tts
            .generate_with_config(text, &gen, None::<fn(&[f32], f32) -> bool>)
            .ok_or_else(|| AppError::msg("本地语音没有合成出内容，请重试或换个语音包"))?;
        let rate = audio.sample_rate().max(1) as u32;
        let samples = audio.samples().to_vec();
        if samples.is_empty() {
            return Err(AppError::msg("本地语音合成了空音频，请重试或换个语音包"));
        }
        write_wav(&cache_dir, &cache_file, &samples, rate);
        Ok((samples, rate))
    }

    fn play(&self, seq: u64, samples: Vec<f32>, rate: u32) {
        let _ = self.sender.send(PlayMsg::Play { seq, samples, rate });
    }
}

impl crate::speech::LocalVoice for LocalTts {
    fn speak_model(&self, model_id: &str, text: &str) -> AppResult<()> {
        let Some(spec) = models::find(model_id) else {
            return Err(AppError::msg("未知语音包"));
        };
        if !models::is_installed(spec, &self.data_dir) {
            return Err(AppError::msg("语音包还没下载完成"));
        }
        // 先停上一句（占住世代号），再合成：合成期间若有新的朗读 / stop，
        // 世代号被推进，这次合成的结果到达音频线程时会被丢弃
        self.stop();
        let seq = self.next_seq();
        let (samples, rate) = self.synth(spec, text)?;
        self.play(seq, samples, rate);
        Ok(())
    }

    fn stop(&self) {
        let seq = self.next_seq();
        let _ = self.sender.send(PlayMsg::Stop { seq });
    }
}

/// 音频线程：持有设备句柄（rodio 0.23：`MixerDeviceSink`，drop 即停声），
/// 创建失败不退出线程，下次播放再试。「先停上一句」= 丢弃当前 `Player`；
/// 过期消息（快速翻词时旧合成后到、stop 之后才完成的合成）直接丢弃。
fn audio_loop(rx: mpsc::Receiver<PlayMsg>) {
    let mut device: Option<rodio::MixerDeviceSink> = None;
    // Player 只是为了「持有期间继续播放、丢弃时停止」，不读它的内容
    let mut current: Option<rodio::Player> = None;
    let mut latest: u64 = 0;
    for msg in rx {
        let seq = match &msg {
            PlayMsg::Play { seq, .. } | PlayMsg::Stop { seq } => *seq,
        };
        if !is_current(latest, seq) {
            continue; // 过期请求：比它新的朗读或 stop 已经发生
        }
        latest = seq;
        match msg {
            PlayMsg::Stop { .. } => {
                drop(current.take()); // drop Player 即停止这条音轨
            }
            PlayMsg::Play { samples, rate, .. } => {
                drop(current.take()); // 先停上一句
                if device.is_none() {
                    match rodio::DeviceSinkBuilder::open_default_sink() {
                        Ok(d) => device = Some(d),
                        Err(e) => {
                            eprintln!("初始化音频输出失败：{e}");
                            continue;
                        }
                    }
                }
                let Some(dev) = device.as_ref() else { continue };
                let player = rodio::Player::connect_new(dev.mixer());
                let channels = std::num::NonZeroU16::new(1).expect("声道数非零");
                // synth 侧已保证 rate >= 1
                let sample_rate = std::num::NonZeroU32::new(rate).expect("采样率非零");
                player.append(rodio::buffer::SamplesBuffer::new(channels, sample_rate, samples));
                current = Some(player);
            }
        }
    }
}

/// 缓存键：模型 + 说话人 + 文本 的 SHA-256（速度固定 1.0 不参与）
fn cache_key(model_id: &str, sid: i32, text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(model_id.as_bytes());
    hasher.update(b"|");
    hasher.update(sid.to_le_bytes());
    hasher.update(b"|");
    hasher.update(text.as_bytes());
    let digest = hasher.finalize();
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn write_wav(cache_dir: &std::path::Path, file: &std::path::Path, samples: &[f32], rate: u32) {
    if std::fs::create_dir_all(cache_dir).is_err() {
        return;
    }
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    if let Ok(mut writer) = hound::WavWriter::create(file, spec) {
        for s in samples {
            let v = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
            let _ = writer.write_sample(v);
        }
        let _ = writer.finalize();
    }
    enforce_cache_limit(cache_dir);
}

fn read_wav(file: &std::path::Path) -> Option<(Vec<f32>, u32)> {
    let reader = hound::WavReader::open(file).ok()?;
    let spec = reader.spec();
    if spec.channels != 1 || spec.bits_per_sample != 16 {
        return None;
    }
    let samples: Vec<f32> = reader
        .into_samples::<i16>()
        .map(|s| s.map(|v| v as f32 / 32768.0))
        .collect::<Result<_, _>>()
        .ok()?;
    Some((samples, spec.sample_rate))
}

/// 缓存总量超限后按 mtime 从旧到新删除
fn enforce_cache_limit(dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<(std::path::PathBuf, u64, std::time::SystemTime)> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "wav"))
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            Some((e.path(), meta.len(), meta.modified().ok()?))
        })
        .collect();
    let total: u64 = files.iter().map(|(_, len, _)| len).sum();
    if total <= CACHE_LIMIT {
        return;
    }
    files.sort_by_key(|(_, _, mtime)| *mtime);
    let mut remaining = total;
    for (path, len, _) in files {
        if remaining <= CACHE_LIMIT {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            remaining -= len;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 审查 P2-3 的核心判定：过期世代的消息必须被丢弃
    #[test]
    fn stale_generation_messages_are_dropped() {
        assert!(is_current(0, 1), "第一条消息生效");
        assert!(!is_current(5, 3), "比最新世代旧的丢弃（快速翻词时旧合成后到）");
        assert!(is_current(5, 5), "同世代不丢（Stop 后紧跟的同号不被误伤）");
        assert!(is_current(5, 9), "更新的消息生效");
    }

    #[test]
    fn cache_key_is_stable_and_sensitive() {
        let a = cache_key("kokoro-int8-v1.1", 47, "雪花");
        let b = cache_key("kokoro-int8-v1.1", 47, "雪花");
        let c = cache_key("kokoro-int8-v1.1", 46, "雪花");
        let d = cache_key("kokoro-v1.1", 47, "雪花");
        assert_eq!(a, b, "同输入同键");
        assert_ne!(a, c, "换音色要换键");
        assert_ne!(a, d, "换模型要换键");
        assert_eq!(a.len(), 64, "sha256 hex 长度");
    }

    /// 超限后从最旧开始删，删到不超限为止
    #[test]
    fn cache_evicts_oldest_first() {
        let dir = std::env::temp_dir().join(format!("dictation-tts-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // 3 个 1MB 的「旧」文件，mtime 依次变新
        let blob = vec![0u8; 1024 * 1024];
        for i in 0..3 {
            let p = dir.join(format!("old{i}.wav"));
            std::fs::write(&p, &blob).unwrap();
            let t = std::time::SystemTime::now() - std::time::Duration::from_secs(100 - i as u64);
            let f = std::fs::File::options().write(true).open(&p).unwrap();
            f.set_modified(t).unwrap();
        }
        // 一个 62MB 的「新」稀疏文件：总量 65MB 超过 64MB 上限，
        // 删掉最旧的 old0 后剩 64MB 恰好达标，应该停手
        let big = dir.join("big.wav");
        let f = std::fs::File::create(&big).unwrap();
        f.set_len(62 * 1024 * 1024).unwrap();
        drop(f);
        // 非 wav 不在淘汰范围
        std::fs::write(dir.join("new.txt"), b"x").unwrap();

        enforce_cache_limit(&dir);
        assert!(!dir.join("old0.wav").exists(), "最旧的先删");
        assert!(dir.join("old1.wav").exists(), "删够就停");
        assert!(dir.join("old2.wav").exists());
        assert!(dir.join("big.wav").exists(), "最新的保留");
        assert!(dir.join("new.txt").exists(), "非 wav 不动");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// WAV 往返：写进去的能按 f32 读回来
    #[test]
    fn wav_round_trip() {
        let dir = std::env::temp_dir().join(format!("dictation-tts-wav-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("x.wav");
        let samples: Vec<f32> = vec![0.0, 0.5, -0.5, 0.99];
        write_wav(&dir, &file, &samples, 22050);
        let (back, rate) = read_wav(&file).expect("能读回");
        assert_eq!(rate, 22050);
        assert_eq!(back.len(), 4);
        assert!((back[1] - 0.5).abs() < 0.001, "幅度误差应远小于 1/32768：{}", back[1]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 端到端验收（wiki/tts-local-models.md 的落地前验证）：真实下载 → 解压 →
    /// 建引擎 → 合成非空 → 缓存命中。需联网约 120MB，默认忽略：
    /// `cargo test kokoro_int8_end_to_end -- --ignored`
    /// 目录固定（不带 pid）：已装好就跳过下载，重跑只验引擎与合成。
    #[test]
    #[ignore = "需联网下载语音包（约 120MB，首次很慢），本机验收用"]
    fn kokoro_int8_end_to_end() {
        use crate::tts::download::{download_source_for_test, extract_tar_bz2, merge_extracted};
        use crate::tts::models;
        use std::sync::atomic::AtomicBool;

        let dir = std::env::temp_dir().join("dictation-tts-poc");
        std::fs::create_dir_all(&dir).unwrap();
        let spec = models::find("kokoro-int8-v1.1").expect("目录里有这个包");
        let final_dir = models::model_dir(&dir, spec.id);

        if !models::is_installed(spec, &dir) {
            let root = models::model_root(&dir);
            std::fs::create_dir_all(&root).unwrap();
            let staging = root.join("poc.staging");
            let model_tmp = root.join("poc.tmp");
            let _ = std::fs::remove_dir_all(&staging);
            let _ = std::fs::remove_dir_all(&model_tmp);
            let cancel = AtomicBool::new(false);
            for src in spec.sources {
                match src {
                    models::Source::TarBz2 { url } => {
                        let dest = root.join("poc.tar.bz2");
                        download_source_for_test(url, &dest, &cancel, |_, _| {}).unwrap();
                        extract_tar_bz2(&dest, &staging).unwrap();
                        merge_extracted(&staging, &model_tmp).unwrap();
                    }
                    models::Source::File { url, name } => {
                        download_source_for_test(url, &model_tmp.join(name), &cancel, |_, _| {})
                            .unwrap();
                    }
                }
            }
            models::verify(spec, &model_tmp)
                .expect("归档文件清单与目录表一致（不一致要改 tts/models.rs）");
            let _ = std::fs::remove_dir_all(&final_dir);
            std::fs::rename(&model_tmp, &final_dir).unwrap();
        }

        let tts = LocalTts::new(dir.clone());
        let (samples, rate) = tts.synth(spec, "雪花").unwrap();
        assert!(!samples.is_empty(), "必须合成出样本");
        assert!(rate >= 8000, "采样率合理：{rate}");
        // 第二次同文本应命中缓存且结果一致
        let (cached, rate2) = tts.synth(spec, "雪花").unwrap();
        assert_eq!((samples.len(), rate), (cached.len(), rate2), "缓存命中且结果一致");
        assert!(dir.join("tts-cache").read_dir().unwrap().next().is_some(), "缓存文件已落盘");
    }
}
