---
title: 前端结构
updated: 2026-11-06
tags: [frontend, react]
related: [architecture.md, decisions.md]
---

# 前端结构

技术：React 18 + TypeScript（strict, noUnusedLocals）+ Vite + Tailwind 4（```tailwindcss/vite`，无 config 文件，主题在 `src/index.css` 的 ```theme`）。

## 路由

没有 react-router。`App.tsx` 里一个联合类型 `Route` 驱动整屏切换；工具集合壳层：**顶栏一级菜单切换工具（`src/tools/registry.ts` 的 `TOOLS`），左侧侧栏是当前工具的二级菜单（`NAV_BY_TOOL`），设置是全局页挂在顶栏**。加第 3 个工具 = Route 加变体 + 注册表加一项 + App.tsx 挂页面。

```ts
// name 是唯一判别字段（工具 id 或 "settings"）
{ name: "dictation"; page: "pick" | "wordbook" | "mistakes" | "history" }
| { name: "dictation"; page: "quiz" | "review"; sessionId: number }
| { name: "dictation"; page: "print"; mode: "mistakes" | "group"; groupId: number | null; title: string }
| { name: "math"; page: "sheet" }
| { name: "settings" }
```

- 默写中 / 批改中，侧栏高亮仍停在「默写」
- Quiz / Review 用 `key={sessionId}` 强制重建，避免上一场的状态残留

## 页面职责

| 页面 | 文件 | 关键点 |
|---|---|---|
| 抽题配置 | `pages/Pick.tsx` | 分组多选（默认全不选，未选时不可开始）、范围、题量（5 / 10 / 15 / 20 / 25 / 30 / 全部）、抽词偏好多选（反复错字 / 错过字 / 新词 / 上次写过的排后 / 同批不重字 / 连字，默认除连字外全选，全不选 = 纯随机）、可抽数量（120ms 防抖）；顶部「继续上次」横幅 |
| 默写 | `pages/Quiz.tsx` | 进入即自动显示、到点遮罩、键盘快捷键、进度落库、`advancing` 锁 |
| 批改 | `pages/Review.tsx` | 网格点错、`skip_marked` 预置、提交后 0.9s 跳转 |
| 错题本 | `pages/Mistakes.tsx` | 分组筛选、连对进度条（读设置）、专项默写、打印、**手工加词并点字标错**（`AddMistakeModal`） |
| 记录 | `pages/History.tsx` | 50 条列表 + 详情弹窗 + 只练本次错词 + 单条删除 |
| 设置 | `pages/Settings.tsx` | 遮罩/连对/字号、备份导出导入、打开数据目录（连字已挪到抽题页的抽词偏好） |
| 词表 | `pages/Wordbook.tsx` | 分组树（可折叠、默认全部折叠、**可拖动调整层级与顺序**）、词数/错题/最近练习、行内编辑、粘贴导入、标记错题 |
| 打印 | `pages/PrintSheet.tsx` | A4 排版 + 田字格，`window.print()` |
| 口算练习 | `pages/math/MathSheet.tsx` | 左侧条件面板（留空位置 / 两步运算符 / 四位置取值区间 / 答案显隐，选项存 localStorage）、A4 满幅卷面（3×10 格 + 可裁剪答案条 + 口诀示范条 `example.ts`）、出题走 `generate_math_sheet` 命令（区间默认 = 三段难度，自定义 = 单池平铺，见 `domain/math.rs`） |

## 打印

`@page` 不再全局声明：各打印页自己渲染 `<style>` 声明（默写单 `margin: 12mm`，口算卷满幅 `margin: 0`；`@page` 无法按 class 作用域隔离，同一时刻只挂一个打印页所以不冲突）。工具条 / 横幅 / 侧栏挂 `.no-print`，正文容器挂 `.print-root`。

打印动作统一走 `src/lib/print.ts` 的 `printElement/printWithToast`，不要直接调 `window.print()`：Windows WebView2 支持原生打印；**macOS WKWebView / Linux WebKitGTK 的 `window.print()` 是静默无操作**（Tauri issue #3066），兜底路径是把「打印元素 outerHTML + 收集到的全量 CSS（含 @page 与 @media print）」拼成独立 HTML，经 `print_html` 命令（`commands/print.rs`）写进临时目录 `dictation-print/`，用系统默认浏览器打开，页面加载后自动调起浏览器的打印对话框（打印 / 存为 PDF 都可用）。打印元素靠 class 携带样式，所以克隆 DOM + 全量 CSS 即可复刻排版。

## 通用组件

| 组件 | 用途 |
|---|---|
| `components/Modal.tsx` | 遮罩弹窗骨架，Esc 关闭 |
| `components/ConfirmModal.tsx` | 危险操作确认，支持左下的第三个动作（如「放弃这次」） |
| `components/Toast.tsx` | `useNotice()` 返回 `{notice, notify}`，4 秒自动消失 |
| `components/GroupModal.tsx` / `PasteImportModal.tsx` | 分组表单 / 粘贴解析预览（预览走后端 `preview_words`） |

## 约定

- 后端调用全部走 `src/lib/ipc.ts`；错误统一 `errorMessage(e)`
- 时间展示用 `formatTime`（MM-DD HH:mm）与 `practiceLabel`（今天/昨天/N 天前练过）
- 破坏性操作一律自建弹窗，不用 `window.confirm`
- 状态机与业务流程放页面内，不引状态库（无 redux/zustand）
- 打印相关类名：`no-print`（打印时隐藏）、`print-page`（去阴影去宽度）、`print-row`（避免跨页断开）、`tian`（田字格）

## 容易忘的

Quiz 进入每个词会 `setRevealed(true)`，再由 `[revealed, settings, index]` 的定时器到点遮罩；`maskSeconds = 0` 表示一直显示。卡片下方用 `items.slice(0, index)` 小字列出已默写的词，纯展示，不落库。批改页的错字标记是 `Map<itemId, Set<字下标>>`，空集合 = 整词错。

词表分组树的拖放（`Wordbook.tsx`）：行上中下三段分别是「变子分组 / 排到前面 / 排到后面」，`zoneOf` 按 `clientY` 与行高的比例切；`targetFor` 负责把落点翻译成 `(parentId, beforeId)` 并挡掉「拖进自己的子孙」，所以非法落点连 `preventDefault` 都不做——不给假的可落点反馈。落点只是渲染态，真正的结构变更全在 `move_group` 里，前端拿到返回的新树直接替换 `groups`。

分组树默认全部折叠：`collapsed` 只在首次加载后按 `hasChildren` 初始化一次（`Wordbook.tsx` 的初始 `useEffect`）。`refreshGroups` 在新建/编辑/删除后也会调用，把初始化挪进去会重置用户的展开状态。新建子分组、拖进折叠分组仍会自动展开父级。

## 应用图标与 favicon

**唯一设计源是仓库根的 `app-icon.svg`**（田字格 + 生字「人」+ 铅笔，暖橙圆角底）。`src-tauri/icons/` 下所有 png / `icon.ico` / `icon.icns` / android / ios 图标都由 `npm run icons`（`tauri icon app-icon.svg`）覆盖生成，**不要手改生成物**。

- 圆角（rx=228）与四角透明由 SVG 自己负责；tauri icon 只做缩放，不会替你裁圆角。别把源图换成不透明方图，否则 macOS / Windows 上会出现直角白边
- `index.html` 用 `./app-icon.svg` 作 favicon，Vite 会当作 asset 处理并带 hash 复制进 `dist/assets/`；不要改成 `/favicon.ico`（没有 public 目录）
- 侧边栏左上角的小图标（`src/App.tsx`）`import appIconUrl from "../app-icon.svg?url"` 显示同一份 SVG，**旧版那个 CSS 画的橙底白「默」字方块已删**。`tsconfig.json` 的 `types` 必须含 `vite/client`，否则 `?url` 模块过不了 `tsc`
- 所以应用图标 / favicon / 侧边栏三处共用一个源：改 `app-icon.svg` → `npm run icons` → 重新构建，三处一起换；只跑 `tauri icon` 而没重新打包，已安装程序里的侧边栏不会变
- `tauri.conf.json` 的 `bundle.icon` 里 `icon.png`（512）是给 Linux 用的，其余四个是 Windows / macOS 常规项
- `app-icon.png` 只是 1024 预览图，可再生成：`npx tauri icon app-icon.svg -p 1024 -o <临时目录>`；源文件必须是正方形且带透明度
- 改完至少验一次 32x32 小图：田字格虚线在 32px 会糊成淡十字，属正常，但生字与铅笔仍要能辨认
