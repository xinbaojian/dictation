// 外部链接统一交给系统浏览器打开。
//
// WebView 里的 <a href> 默认会直接把主窗口导航走（本项目没配 on_navigation，Tauri 不拦），
// 表现为白屏且没有后退可回；window.open / target="_blank" 也开不出浏览器。
// 所以调用方必须 preventDefault，改用官方 opener 插件。这是应用里唯一的「打开外部链接」出口，
// 只由用户点击触发，与自动升级无关（打开数据目录 / 打印的临时文件走各自的后端命令）。
import { openUrl } from "@tauri-apps/plugin-opener";

/** 只放行 http / https：更新说明来自网络，别让 file: / javascript: 之类的协议进系统壳。
 *  注意 ACL 的默认 scope 还含 mailto:/tel:（见 wiki/updater.md 第 4 条），协议白名单靠这一层维护。
 *  校验和小写化共用这一个模式，加协议时别只改一处。 */
const HTTP_SCHEME = /^https?:\/\//i;

/** 返回值表示是否成功唤起浏览器，调用方据此决定要不要提示（失败不抛错，别打断看说明） */
export async function openExternalLink(url: string): Promise<boolean> {
  if (!HTTP_SCHEME.test(url)) return false;
  // 协议头统一转小写：ACL 里 opener 的 glob（`https://*`）大小写敏感，
  // 前端放行 `HTTPS://…` 后端会判 ForbiddenUrl，点了只会弹「打不开链接」
  const target = url.replace(HTTP_SCHEME, (scheme) => scheme.toLowerCase());
  try {
    await openUrl(target);
    return true;
  } catch {
    // 没有默认浏览器，或被系统策略拦下
    return false;
  }
}
