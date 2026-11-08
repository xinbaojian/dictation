//! 语音包下载：后台线程 + 进度事件 + 断点续传（Range）+ 解压落位。
//!
//! 同时只允许一个下载任务（`Downloads` 注册表兜底）；下载写 `.part` 文件，
//! 失败 / 取消后保留，下次继续从断点拉（服务器不支持 Range 时自动重下）。
//! 解压先落 `<id>.staging` / `<id>.tmp`，全部校验通过后原子 rename 到 `<id>/`。

use crate::error::{AppError, AppResult};
use crate::tts::models::{self, ModelSpec, Source};
use reqwest::header::CONTENT_LENGTH;
use serde::Serialize;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::Emitter;

/// 前端进度事件名（listen("tts-download-progress")）
pub const EVENT: &str = "tts-download-progress";

/// 进度事件载荷；phase: downloading | extracting | done | error | canceled
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadEvent {
    pub model_id: String,
    pub phase: &'static str,
    pub downloaded: u64,
    pub total: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// 供 list_tts_models 回显的进行中任务快照（页面刷新后恢复进度条）
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveInfo {
    pub model_id: String,
    pub phase: &'static str,
    pub downloaded: u64,
    pub total: u64,
}

#[derive(Default)]
pub struct Downloads {
    /// 全局同时只允许一个下载任务；记最近一次进度供快照回显
    inner: Mutex<Option<Active>>,
}

struct Active {
    model_id: String,
    cancel: Arc<AtomicBool>,
    info: ActiveInfo,
}

impl Downloads {
    /// 注册一个下载任务，返回取消开关；已有任务在跑则报错
    pub fn start(&self, model_id: &str) -> AppResult<Arc<AtomicBool>> {
        let mut guard = self.inner.lock().map_err(|_| AppError::msg("下载状态异常"))?;
        if guard.is_some() {
            return Err(AppError::msg("已有语音包在下载，请等它结束"));
        }
        let cancel = Arc::new(AtomicBool::new(false));
        *guard = Some(Active {
            model_id: model_id.to_string(),
            info: ActiveInfo {
                model_id: model_id.to_string(),
                phase: "downloading",
                downloaded: 0,
                total: 0,
            },
            cancel: cancel.clone(),
        });
        Ok(cancel)
    }

    pub fn cancel(&self, model_id: &str) -> bool {
        let Ok(guard) = self.inner.lock() else { return false };
        match guard.as_ref() {
            Some(a) if a.model_id == model_id => {
                a.cancel.store(true, Ordering::Relaxed);
                true
            }
            _ => false,
        }
    }

    /// 下载线程汇报进度；任务结束（任何结果）时 finish 清空注册
    pub fn update(&self, downloaded: u64, total: u64, phase: &'static str) {
        if let Ok(mut guard) = self.inner.lock() {
            if let Some(a) = guard.as_mut() {
                a.info.downloaded = downloaded;
                a.info.total = total;
                a.info.phase = phase;
            }
        }
    }

    pub fn finish(&self, model_id: &str) {
        if let Ok(mut guard) = self.inner.lock() {
            if guard.as_ref().is_some_and(|a| a.model_id == model_id) {
                *guard = None;
            }
        }
    }

    pub fn snapshot(&self) -> Option<ActiveInfo> {
        self.inner.lock().ok().and_then(|g| g.as_ref().map(|a| a.info.clone()))
    }
}

/// 分段 Range 请求头（审查 P2-1）：`bytes=start-end`（含两端）。
/// 分段是为了给每段请求配总超时——reqwest blocking 没有单次读超时，
/// 不分段的话断流会让 read 永久阻塞，取消开关也拦不住。
pub fn segment_range(start: u64, end_inclusive: u64) -> String {
    format!("bytes={start}-{end_inclusive}")
}

/// 每段大小：8MB。慢速网络（≥45KB/s）也能在段超时内完成；
/// 段边界同时是取消开关与断点续传的检查点
const SEGMENT: u64 = 8 * 1024 * 1024;
/// 单段总超时（含建立连接与读完整段）
const SEGMENT_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResumePlan {
    /// 服务器回 206：按 Range 续传
    Append,
    /// 服务器回 200 / 本地没有断点：从头重下
    Restart,
}

pub fn resume_plan(existing: u64, status: u16) -> ResumePlan {
    if existing > 0 && status == 206 {
        ResumePlan::Append
    } else {
        ResumePlan::Restart
    }
}

/// `.part` 断点文件路径（与目标同名 + `.part`）
fn part_path(dest: &Path) -> PathBuf {
    let mut name = dest.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    dest.with_file_name(name)
}

/// PoC 集成测试复用下载实现（不打事件，见 speech::local::tests）
#[cfg(test)]
pub fn download_source_for_test(
    url: &str,
    dest: &Path,
    cancel: &AtomicBool,
    on_progress: impl FnMut(u64, u64),
) -> AppResult<()> {
    download_source(url, dest, cancel, on_progress)
}

/// 下载单个源到 dest（分段 Range + 每段总超时 + 断点续传）。
/// 进度回调只做展示，不抛错。失败 / 取消保留 .part，下次从断点继续。
fn download_source(
    url: &str,
    dest: &Path,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(u64, u64),
) -> AppResult<()> {
    let part = part_path(dest);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        // 每段请求一个总超时：断流最多卡到本段超时，不会永久挂死
        .timeout(SEGMENT_TIMEOUT)
        .build()
        .map_err(|e| AppError::msg(format!("初始化下载失败：{e}")))?;

    let mut have = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    let mut total: Option<u64> = None;
    let mut buf = vec![0u8; 64 * 1024];

    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(AppError::msg("已取消"));
        }
        let end = have.saturating_add(SEGMENT - 1);
        let mut resp = client
            .get(url)
            .header("Range", segment_range(have, end))
            .send()
            .map_err(|e| AppError::msg(format!("连接下载服务器失败：{e}（请检查网络后重试，会从断点继续）")))?;
        let status = resp.status().as_u16();
        if status == 416 {
            // 请求区间越过文件末尾：说明 .part 已写到头（断点续传完成态）
            break;
        }
        if !resp.status().is_success() {
            return Err(AppError::msg(format!("下载失败（HTTP {status}），请稍后重试")));
        }

        let plan = resume_plan(have, status);
        let mut file = match plan {
            ResumePlan::Append => std::fs::OpenOptions::new().create(true).append(true).open(&part)?,
            ResumePlan::Restart => {
                // 服务器无视 Range 回了全量（或本地没有断点）：从头写
                have = 0;
                std::fs::OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .open(&part)?
            }
        };

        if total.is_none() {
            // 206 时总大小只能从 Content-Range 尾段取（Content-Length 只是本段的量）；
            // 200 时 Content-Length 就是总大小。取不到就保持未知，进度按字节显示
            total = if status == 206 {
                resp.headers()
                    .get(reqwest::header::CONTENT_RANGE)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.rsplit('/').next())
                    .and_then(|v| v.parse::<u64>().ok())
                    .filter(|t| *t > 0)
            } else {
                resp.headers()
                    .get(CONTENT_LENGTH)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                    .filter(|t| *t > 0)
            };
        }
        on_progress(have, total.unwrap_or(0));

        let mut since_emit = 0u64;
        let before = have;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(AppError::msg("已取消"));
            }
            let n = resp.read(&mut buf)?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n])?;
            have += n as u64;
            since_emit += n as u64;
            if since_emit >= 512 * 1024 {
                on_progress(have, total.unwrap_or(0));
                since_emit = 0;
            }
        }
        file.flush()?;
        on_progress(have, total.unwrap_or(0));

        if let Some(t) = total {
            if have >= t {
                break;
            }
        }
        if have == before {
            // 本段一个字节都没拿到（还没到文件末尾）：防住同区间空转死循环
            return Err(AppError::msg("下载没有进展，请检查网络后重试（会从断点继续）"));
        }
        // 段结束，回到循环顶：检查取消、请求下一段
    }

    if let Some(t) = total {
        if have != t {
            return Err(AppError::msg(format!("下载不完整（{have}/{t} 字节），请重试")));
        }
    }
    std::fs::rename(&part, dest)?;
    Ok(())
}

/// 归一化归档内路径：拒绝 `..`、绝对路径、盘符前缀。
/// tar-rs 的 unpack 自带防护，这里显式再挡一层，让「恶意条目」有明确的中文报错与单测。
pub fn safe_relative(p: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::Normal(s) => out.push(s),
            Component::CurDir => {}
            _ => return None,
        }
    }
    Some(out)
}

/// 解压 .tar.bz2 到 dest（保留可执行位等属性）。
/// 条目类型只放行普通文件与目录：link 条目（symlink/hardlink）的 target
/// 不受 safe_relative 检查，先写链接再经它写文件就能逃出 dest（审查 P2-5；
/// 官方模型归档只含普通文件与目录，实测解压通过）。
pub fn extract_tar_bz2(archive: &Path, dest: &Path) -> AppResult<()> {
    std::fs::create_dir_all(dest)?;
    let f = std::fs::File::open(archive)?;
    let dec = bzip2::read::BzDecoder::new(f);
    let mut tar = tar::Archive::new(dec);
    tar.set_preserve_permissions(true);
    for entry in tar.entries()? {
        let mut entry = entry?;
        match entry.header().entry_type() {
            tar::EntryType::Regular | tar::EntryType::Directory => {}
            _ => {
                return Err(AppError::msg(
                    "语音包内容含非法条目（链接等），已中止（下载源可能被篡改）",
                ));
            }
        }
        let rel = entry.path()?;
        let Some(rel) = safe_relative(&rel) else {
            return Err(AppError::msg("语音包内容含非法路径，已中止（下载源可能被篡改）"));
        };
        let target = dest.join(rel);
        // Entry::unpack 只负责写文件本体，归档里没有目录条目时要自己建父目录
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        entry.unpack(&target)?;
    }
    Ok(())
}

/// 归档顶层通常是单个版本目录（如 `kokoro-…-v1_1/`）：取其内容并入模型目录；
/// 若没有统一的顶层目录，则把 staging 顶层全部并入。
pub fn merge_extracted(staging: &Path, model_tmp: &Path) -> AppResult<()> {
    let top: Vec<_> = std::fs::read_dir(staging)?
        .filter_map(|e| e.ok())
        .collect();
    let source = if top.len() == 1 && top[0].path().is_dir() {
        top[0].path()
    } else {
        staging.to_path_buf()
    };
    std::fs::create_dir_all(model_tmp)?;
    for entry in std::fs::read_dir(&source)? {
        let entry = entry?;
        std::fs::rename(entry.path(), model_tmp.join(entry.file_name()))?;
    }
    Ok(())
}

/// 后台线程主体：下载所有源 → 解压合并 → 校验 → 原子落位 → 清理归档。
/// 进度同时发事件（前端）与写注册表（list_tts_models 快照恢复用）。
fn install(
    app: &tauri::AppHandle,
    spec: &ModelSpec,
    cancel: &AtomicBool,
    data_dir: &Path,
    registry: &Downloads,
) -> AppResult<()> {
    let root = models::model_root(data_dir);
    std::fs::create_dir_all(&root)?;
    let staging = root.join(format!("{}.staging", spec.id));
    let model_tmp = root.join(format!("{}.tmp", spec.id));
    let final_dir = models::model_dir(data_dir, spec.id);
    let _ = std::fs::remove_dir_all(&staging);
    let _ = std::fs::remove_dir_all(&model_tmp);
    std::fs::create_dir_all(&staging)?;
    std::fs::create_dir_all(&model_tmp)?;

    let report = |downloaded: u64, total: u64| {
        registry.update(downloaded, total, "downloading");
        let _ = app.emit(
            EVENT,
            DownloadEvent {
                model_id: spec.id.into(),
                phase: "downloading",
                downloaded,
                total,
                message: None,
            },
        );
    };
    let mark_phase = |phase: &'static str| {
        registry.update(0, 0, phase);
        let _ = app.emit(
            EVENT,
            DownloadEvent {
                model_id: spec.id.into(),
                phase,
                downloaded: 0,
                total: 0,
                message: None,
            },
        );
    };

    for src in spec.sources {
        match src {
            Source::TarBz2 { url } => {
                let dest = root.join(format!("{}.tar.bz2", spec.id));
                download_source(url, &dest, cancel, report)?;
                mark_phase("extracting");
                extract_tar_bz2(&dest, &staging)?;
                merge_extracted(&staging, &model_tmp)?;
                let _ = std::fs::remove_dir_all(&staging);
                let _ = std::fs::remove_file(&dest);
            }
            Source::File { url, name } => {
                let dest = model_tmp.join(name);
                download_source(url, &dest, cancel, report)?;
            }
        }
    }

    models::verify(spec, &model_tmp)?;
    let _ = std::fs::remove_dir_all(&final_dir);
    std::fs::rename(&model_tmp, &final_dir)?;
    Ok(())
}

/// 线程入口：包一层事件收尾（done / error / canceled），任何退出路径都清注册表
pub fn run(
    app: tauri::AppHandle,
    spec: &'static ModelSpec,
    cancel: Arc<AtomicBool>,
    data_dir: PathBuf,
    registry: Arc<Downloads>,
) {
    let result = install(&app, spec, cancel.as_ref(), &data_dir, &registry);
    let canceled = cancel.load(Ordering::Relaxed);
    let (phase, message) = match &result {
        Ok(()) => ("done", None),
        Err(e) => (
            if canceled { "canceled" } else { "error" },
            Some(e.to_string()),
        ),
    };
    let _ = app.emit(
        EVENT,
        DownloadEvent {
            model_id: spec.id.into(),
            phase,
            downloaded: 0,
            total: 0,
            message,
        },
    );
    registry.finish(spec.id);
}

#[cfg(test)]
mod tests {
    use super::*;
    use bzip2::write::BzEncoder;
    use bzip2::Compression;

    fn tmp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dictation-tts-dl-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn segment_range_format() {
        assert_eq!(segment_range(0, 8 * 1024 * 1024 - 1), "bytes=0-8388607");
        assert_eq!(segment_range(8388608, 16777215), "bytes=8388608-16777215");
    }

    #[test]
    fn resume_plan_branches() {
        assert_eq!(resume_plan(500, 206), ResumePlan::Append);
        // 没有断点时即使服务器支持 Range 也从头下
        assert_eq!(resume_plan(0, 206), ResumePlan::Restart);
        // 服务器无视 Range（回 200 全量）也要重下，不能接着旧 .part 追加
        assert_eq!(resume_plan(500, 200), ResumePlan::Restart);
    }

    #[test]
    fn safe_relative_rejects_traversal() {
        assert_eq!(safe_relative(Path::new("a/b.txt")), Some(PathBuf::from("a/b.txt")));
        assert_eq!(safe_relative(Path::new("./a")), Some(PathBuf::from("a")));
        assert_eq!(safe_relative(Path::new("../evil.txt")), None);
        assert_eq!(safe_relative(Path::new("/etc/passwd")), None);
        assert_eq!(safe_relative(Path::new("a/../../evil")), None);
    }

    /// 真实构造一个小 tar.bz2 再解压：正常条目落位。tar-rs 的 Builder 会拒绝
    /// 构造 `..` 条目，所以恶意归档用手工 512 字节头伪造（见下一个测试）。
    #[test]
    fn extracts_tar_bz2() {
        let dir = tmp("extract");
        let archive = dir.join("mini.tar.bz2");
        {
            let f = std::fs::File::create(&archive).unwrap();
            let enc = BzEncoder::new(f, Compression::fast());
            let mut tar = tar::Builder::new(enc);
            let mut h = tar::Header::new_gnu();
            h.set_size(5);
            h.set_mode(0o644);
            h.set_cksum();
            tar.append_data(&mut h, "pkg/hello.txt", &b"world"[..]).unwrap();
            tar.into_inner().unwrap().finish().unwrap().flush().unwrap();
        }
        let out = dir.join("out");
        extract_tar_bz2(&archive, &out).unwrap();
        assert_eq!(std::fs::read_to_string(out.join("pkg/hello.txt")).unwrap(), "world");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 手工伪造 tar 头（tar-rs 的 Builder 造不出恶意条目，只能拼原始 512 字节）。
    /// typeflag：b'0' 普通文件；b'2' 符号链接（linkname 在 157 偏移）。
    fn raw_header(name: &str, size: u32, typeflag: u8, linkname: &str) -> [u8; 512] {
        let mut header = [0u8; 512];
        let name_bytes = name.as_bytes();
        header[..name_bytes.len()].copy_from_slice(name_bytes);
        header[100..108].copy_from_slice(b"0000644\0");
        header[108..116].copy_from_slice(b"0000000\0");
        header[116..124].copy_from_slice(b"0000000\0");
        header[124..136].copy_from_slice(format!("{size:011o}\0").as_bytes());
        header[136..148].copy_from_slice(b"00000000000\0");
        let link = linkname.as_bytes();
        if !link.is_empty() {
            header[157..157 + link.len()].copy_from_slice(link);
        }
        header[156] = typeflag; // 必须先于校验和写入
        let sum: u32 = header
            .iter()
            .enumerate()
            .map(|(i, b)| if (148..156).contains(&i) { b' ' as u32 } else { *b as u32 })
            .sum();
        header[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
        header
    }

    /// 手工伪造含 `../evil.txt` 条目的 tar.bz2：解压必须拒绝且不得把文件写到目录外。
    #[test]
    fn extract_rejects_hand_crafted_traversal_entry() {
        let dir = tmp("evil");
        let archive = dir.join("evil.tar.bz2");
        {
            let f = std::fs::File::create(&archive).unwrap();
            let mut enc = BzEncoder::new(f, Compression::fast());
            enc.write_all(&raw_header("../evil.txt", 3, b'0', "")).unwrap();
            enc.write_all(b"bad").unwrap();
            enc.write_all(&[0u8; 1024]).unwrap(); // 结束块
            enc.finish().unwrap().flush().unwrap();
        }
        let out = dir.join("out");
        let result = extract_tar_bz2(&archive, &out);
        assert!(result.is_err(), "路径穿越条目必须被拒");
        let err = result.unwrap_err().to_string();
        assert!(err.contains("非法路径") || err.contains(".."), "报错要指向路径问题：{err}");
        assert!(!dir.join("evil.txt").exists(), "穿越目标绝不能被写出");
        assert!(!out.join("evil.txt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 审查 P2-5：symlink 条目自身的路径合法，但它的 target 不受路径检查——
    /// 必须按条目类型直接拒绝（先写 evil -> /Users，再经 evil/ 写文件即可逃逸）。
    #[test]
    fn extract_rejects_symlink_entries() {
        let dir = tmp("symlink");
        let archive = dir.join("link.tar.bz2");
        {
            let f = std::fs::File::create(&archive).unwrap();
            let mut enc = BzEncoder::new(f, Compression::fast());
            enc.write_all(&raw_header("evil-link", 0, b'2', "/Users")).unwrap();
            enc.write_all(&[0u8; 1024]).unwrap();
            enc.finish().unwrap().flush().unwrap();
        }
        let out = dir.join("out");
        let result = extract_tar_bz2(&archive, &out);
        assert!(result.is_err(), "链接条目必须被拒");
        let err = result.unwrap_err().to_string();
        assert!(err.contains("非法条目"), "报错要指明是条目类型问题：{err}");
        assert!(!out.join("evil-link").exists(), "链接本身也不能落盘");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn merge_takes_single_top_dir_contents() {
        let dir = tmp("merge");
        let staging = dir.join("staging");
        let model_tmp = dir.join("model");
        std::fs::create_dir_all(staging.join("kokoro-v1_1/sub")).unwrap();
        std::fs::write(staging.join("kokoro-v1_1/model.onnx"), b"m").unwrap();
        std::fs::write(staging.join("kokoro-v1_1/sub/tokens.txt"), b"t").unwrap();
        merge_extracted(&staging, &model_tmp).unwrap();
        assert!(model_tmp.join("model.onnx").is_file());
        assert!(model_tmp.join("sub/tokens.txt").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 顶层不是单一目录（裸文件归档）：全部并入
    #[test]
    fn merge_without_top_dir_moves_everything() {
        let dir = tmp("merge2");
        let staging = dir.join("staging");
        let model_tmp = dir.join("model");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(staging.join("a.txt"), b"1").unwrap();
        std::fs::write(staging.join("b.txt"), b"2").unwrap();
        merge_extracted(&staging, &model_tmp).unwrap();
        assert!(model_tmp.join("a.txt").is_file() && model_tmp.join("b.txt").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn registry_allows_one_task_at_a_time() {
        let reg = Downloads::default();
        let c1 = reg.start("a").unwrap();
        assert!(reg.start("b").is_err(), "第二个任务要被拒");
        assert!(!reg.cancel("b"));
        assert!(reg.cancel("a"));
        reg.update(10, 100, "downloading");
        let snap = reg.snapshot().unwrap();
        assert_eq!((snap.model_id.as_str(), snap.downloaded, snap.total), ("a", 10, 100));
        reg.finish("other"); // 不匹配不清
        assert!(reg.snapshot().is_some());
        reg.finish("a");
        assert!(reg.snapshot().is_none());
        drop(c1);
    }
}
