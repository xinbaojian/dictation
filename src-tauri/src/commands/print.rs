use crate::error::{AppError, AppResult};
use std::path::PathBuf;

/// 文件名净化：去掉路径分隔符与控制字符，空名回退「打印」。
/// 纯函数，单测在文件末尾。
fn sanitize_name(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|c| !c.is_control() && *c != '/' && *c != '\\' && *c != ':')
        .collect::<String>()
        .trim()
        .to_string();
    if cleaned.is_empty() {
        "打印".to_string()
    } else {
        cleaned
    }
}

/// 把要打印的完整 HTML 写进临时目录（dictation-print/ 下，按名字覆盖），
/// 返回文件路径；打开浏览器由 print_html 负责（打开动作不做单测）。
fn write_temp_html(name: &str, html: &str) -> AppResult<PathBuf> {
    let dir = std::env::temp_dir().join("dictation-print");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.html", sanitize_name(name)));
    std::fs::write(&path, html)?;
    Ok(path)
}

/// 用系统默认程序打开 path（html 会由默认浏览器接管）。
/// 与 system.rs::open_data_dir 同款：explorer / open / xdg-open + spawn，
/// 全程不经 shell（cmd 的 start 会对元字符重解析，有注入面）。
fn open_in_browser(path: &std::path::Path) -> AppResult<()> {
    let opener = if cfg!(target_os = "windows") {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(opener)
        .arg(path)
        .spawn()
        .map_err(|e| AppError::msg(format!("打开浏览器失败：{e}")))?;
    Ok(())
}

/// 打印入口（macOS / Linux WebView 不支持 window.print() 时的兜底）：
/// 前端把「要打印的元素 + 全部样式」拼成独立 HTML 传进来，写临时文件后用默认浏览器打开，
/// 页面内嵌脚本会在加载后自动调起浏览器的打印对话框（打印 / 另存为 PDF 都可用）。
#[tauri::command]
pub fn print_html(name: String, html: String) -> AppResult<String> {
    let path = write_temp_html(&name, &html)?;
    open_in_browser(&path)?;
    Ok(path.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_strips_path_chars_and_falls_back() {
        assert_eq!(sanitize_name("口算练习-题号123"), "口算练习-题号123");
        assert_eq!(sanitize_name("a/b\\c:d"), "abcd");
        assert_eq!(sanitize_name("  默写单  "), "默写单");
        assert_eq!(sanitize_name(""), "打印");
        assert_eq!(sanitize_name("///"), "打印");
    }

    #[test]
    fn write_temp_html_creates_file_with_content() {
        // 调用约定：name 不带扩展名，后缀由这里统一追加
        let path = write_temp_html("单测-打印内容", "<p>hello</p>").unwrap();
        assert!(path.to_string_lossy().ends_with("单测-打印内容.html"));
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "<p>hello</p>");
        std::fs::remove_file(&path).ok();
    }
}
