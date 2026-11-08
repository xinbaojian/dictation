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

/// Range 续传请求头：从已有字节数继续
pub fn range_value(existing: u64) -> String {
    format!("bytes={existing}-")
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResumePlan {
    /// 服务器回 206：从 .part 现有长度续写
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

/// 下载单个源到 dest。进度回调只做展示，不抛错。
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
    let existing = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);

    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        // 不设总超时：大文件整个请求可能持续很久，卡死由取消开关兜底
        .build()
        .map_err(|e| AppError::msg(format!("初始化下载失败：{e}")))?;
    let mut req = client.get(url);
    if existing > 0 {
        req = req.header("Range", range_value(existing));
    }
    let mut resp = req
        .send()
        .map_err(|e| AppError::msg(format!("连接下载服务器失败：{e}（请检查网络后重试）")))?;
    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        return Err(AppError::msg(format!("下载失败（HTTP {status}），请稍后重试")));
    }

    let plan = resume_plan(existing, resp.status().as_u16());
    // 续传 = 追加写；重下 = 覆写（create/write/truncate 三件套缺一会拒绝打开）
    let mut file = match plan {
        ResumePlan::Append => std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&part)?,
        ResumePlan::Restart => std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&part)?,
    };

    // 206 的 Content-Length 是「剩余字节」，完整大小要加上已有断点
    let head_len: u64 = resp
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let total = match plan {
        ResumePlan::Append => head_len + existing,
        ResumePlan::Restart => head_len,
    };
    let mut downloaded = match plan {
        ResumePlan::Append => existing,
        ResumePlan::Restart => 0,
    };
    on_progress(downloaded, total);

    let mut buf = vec![0u8; 64 * 1024];
    let mut since_emit = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(AppError::msg("已取消"));
        }
        let n = resp.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        downloaded += n as u64;
        since_emit += n as u64;
        if since_emit >= 512 * 1024 {
            on_progress(downloaded, total);
            since_emit = 0;
        }
    }
    file.flush()?;
    on_progress(downloaded, total);
    if total > 0 && downloaded != total {
        return Err(AppError::msg(format!(
            "下载不完整（{downloaded}/{total} 字节），请重试"
        )));
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

/// 解压 .tar.bz2 到 dest（保留可执行位等属性）
pub fn extract_tar_bz2(archive: &Path, dest: &Path) -> AppResult<()> {
    std::fs::create_dir_all(dest)?;
    let f = std::fs::File::open(archive)?;
    let dec = bzip2::read::BzDecoder::new(f);
    let mut tar = tar::Archive::new(dec);
    tar.set_preserve_permissions(true);
    for entry in tar.entries()? {
        let mut entry = entry?;
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
    fn range_value_format() {
        assert_eq!(range_value(0), "bytes=0-");
        assert_eq!(range_value(1234), "bytes=1234-");
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

    /// 手工伪造含 `../evil.txt` 条目的 tar.bz2（tar-rs 的 Builder 造不出来，
    /// 只能拼原始 512 字节头）：解压必须拒绝且不得把文件写到目录外。
    #[test]
    fn extract_rejects_hand_crafted_traversal_entry() {
        let dir = tmp("evil");
        let archive = dir.join("evil.tar.bz2");
        {
            let f = std::fs::File::create(&archive).unwrap();
            let mut enc = BzEncoder::new(f, Compression::fast());
            // GNU tar 头：name[100] mode[8] uid[8] gid[8] size[12] mtime[12] chksum[8] type[1]
            let mut header = [0u8; 512];
            header[..11].copy_from_slice(b"../evil.txt");
            header[100..108].copy_from_slice(b"0000644\0");
            header[108..116].copy_from_slice(b"0000000\0");
            header[116..124].copy_from_slice(b"0000000\0");
            header[124..136].copy_from_slice(b"00000000003\0"); // size = 3 字节
            header[136..148].copy_from_slice(b"00000000000\0");
            header[156] = b'0'; // 普通文件（必须先于校验和写入）
            // chksum：除 chksum 字段（按 8 个空格计）外逐字节求和
            let sum: u32 = header
                .iter()
                .enumerate()
                .map(|(i, b)| if (148..156).contains(&i) { b' ' as u32 } else { *b as u32 })
                .sum();
            header[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
            enc.write_all(&header).unwrap();
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
