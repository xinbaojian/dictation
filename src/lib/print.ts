import { api, errorMessage } from "./ipc";

/** Windows WebView2 实现了 window.print()（原生打印对话框）；
 *  macOS WKWebView / Linux WebKitGTK 的 window.print() 是静默无操作，
 *  兜底走「导出独立 HTML → Rust 写临时文件 → 默认浏览器打开 → 自动调起打印」。 */
export function isWebViewPrintSupported(): boolean {
  return navigator.userAgent.includes("Windows");
}

/** 收集当前文档的全部 CSS 规则（Tailwind + 页面 <style>，含 @page 与 @media print）。
 *  打印元素靠 class 携带样式，克隆 DOM + 全量 CSS 即可完整复刻排版。 */
function collectCss(): string {
  const parts: string[] = [];
  for (const sheet of Array.from(document.styleSheets)) {
    try {
      for (const rule of Array.from(sheet.cssRules)) {
        parts.push(rule.cssText);
      }
    } catch {
      // 跨域样式表读不到规则；本项目样式全部同源，忽略即可
    }
  }
  return parts.join("\n");
}

/** HTML 转义：name 含用户可控的分组标题，拼进 <title> 前必须转义（防提前闭合注入） */
function escapeHtml(text: string): string {
  return text.replace(/[&<>"']/g, (c) =>
    ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c] ?? c,
  );
}

/** 打印页面上某个元素（selector 选中它的 outerHTML）。
 *  name 不带扩展名，用于临时文件名（口算卷用题号，默写单用标题）。
 *  返回提示文案；失败抛错由调用方 toast。 */
export async function printElement(selector: string, name: string): Promise<string> {
  // 校验先于平台分流：Windows 上元素缺失同样报错，而不是把空态卡片整页打出去
  const el = document.querySelector(selector);
  if (!el) {
    throw new Error("找不到要打印的内容");
  }
  if (isWebViewPrintSupported()) {
    window.print();
    return "";
  }
  const html = `<!doctype html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<title>${escapeHtml(name)}</title>
<style>${collectCss()}</style>
<style>html,body{margin:0;background:#fff}</style>
</head>
<body>
${el.outerHTML}
<script>window.addEventListener("load",function(){setTimeout(function(){window.print()},300)});</script>
</body>
</html>`;
  await api.printHtml(name, html);
  return "已在默认浏览器打开，用打印或「存储为 PDF」即可";
}

/** 页面按钮统一入口：成功给提示，失败把中文错误抛给调用方的 notify */
export async function printWithToast(
  selector: string,
  name: string,
  notify: (kind: "ok" | "error", text: string) => void,
): Promise<void> {
  try {
    const tip = await printElement(selector, name);
    if (tip) notify("ok", tip);
  } catch (e) {
    notify("error", errorMessage(e));
  }
}
