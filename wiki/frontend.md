---
title: 前端结构
updated: 2026-11-14
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
| { name: "dictation"; page: "stroke"; groupId: number | null; title: string }
| { name: "dictation"; page: "print"; mode: "mistakes" | "group"; groupId: number | null; title: string }
| { name: "math"; page: "sheet" }
| { name: "lookup"; page: "lookup" }
| { name: "settings" }
```

- 默写中 / 批改中，侧栏高亮仍停在「默写」；笔画复习（`stroke`）挂在「错题」下
- Quiz / Review 用 `key={sessionId}` 强制重建，避免上一场的状态残留

## 页面职责

| 页面 | 文件 | 关键点 |
|---|---|---|
| 抽题配置 | `pages/Pick.tsx` | 分组多选（默认全不选，未选时不可开始）、范围、题量（5 / 10 / 15 / 20 / 25 / 30 / 全部）、抽词偏好多选（反复错字 / 错过字 / 新词 / 上次写过的排后 / 同批不重字 / 连字，默认除连字外全选，全不选 = 纯随机）、可抽数量（120ms 防抖）；顶部「继续上次」横幅 |
| 默写 | `pages/Quiz.tsx` | 进入即自动显示、到点遮罩、键盘快捷键、进度落库、`advancing` 锁 |
| 批改 | `pages/Review.tsx` | 网格点错、`skip_marked` 预置、提交后 0.9s 跳转 |
| 错题本 | `pages/Mistakes.tsx` | 分组筛选、连对进度条（读设置）、专项默写（**点击后先确认再开**，防误点）、打印、**手工加词并点字标错**（`AddMistakeModal`）、**进笔画复习（点词语直接从该词开始，路由带 `wordId`）** |
| 笔画复习 | `pages/StrokeReview.tsx` | 逐词看笔顺动画（`components/StrokeWord.tsx` + `lib/strokes.ts`）、每字一个田字格、田字格上方逐字拼音（`lib/pinyin.ts`，多音字按词消歧，仅此页）、点格子只重播该字、「先自己写」遮住笔画后再对照、系统离线语音朗读（自动+手动，偏好记忆）、AI 巧记讲解卡（`components/MnemonicCard.tsx`，配置 AI 后可按需生成，遮蔽模式整卡隐藏，展开可自动读口诀）、空格/←/R 快捷键、可从指定词开始（错题本点词语；找不到时从头开始） |
| 记录 | `pages/History.tsx` | 50 条列表 + 详情弹窗 + 只练本次错词 + 单条删除 |
| 设置 | `pages/Settings.tsx` | 遮罩/连对/字号、备份导出导入、打开数据目录（连字已挪到抽题页的抽词偏好）、AI 讲解配置区块、朗读语音区块（`components/TtsSettings.tsx`：引擎切换 + 语音包下载/试听/删除，ADR-21） |
| 词表 | `pages/Wordbook.tsx` | 分组树（可折叠、默认全部折叠、**可拖动调整层级与顺序**）、词数/错题/最近练习、行内编辑、粘贴导入、标记错题 |
| 打印 / 字帖 | `pages/PrintSheet.tsx` | A4 字帖：每行一个词横向排满（示范字 → 描红 → 空格），可选方格（田 / 米 / 方）、大小（10 / 12 / 14mm）、描红遍数（0 / 1 / 2）、抄写单 ↔ 默写单；选项存 localStorage（`printSheet.options`） |
| 口算练习 | `pages/math/MathSheet.tsx` | 左侧条件面板（留空位置 / 两步运算符 / 四位置取值区间 / 答案显隐，选项存 localStorage）、A4 满幅卷面（3×10 格 + 可裁剪答案条 + 口诀示范条 `example.ts`）、出题走 `generate_math_sheet` 命令（区间默认 = 三段难度，自定义 = 单池平铺，见 `domain/math.rs`） |
| 查字 | `pages/Lookup.tsx` | 输入字词逐字出卡（`components/CharInfoCard.tsx`）：田字格笔顺动画（复用 `StrokeWord`）+ 拼音 + 部首/笔画数/结构/释义（`lib/dict.ts` 离线字典）；AI 未配置显示提示行，配置后可生成字卡并缓存秒开；见下「查字工具」 |

## 打印

`@page` 不再全局声明：各打印页自己渲染 `<style>` 声明（默写单 `margin: 12mm`，口算卷满幅 `margin: 0`；`@page` 无法按 class 作用域隔离，同一时刻只挂一个打印页所以不冲突）。工具条 / 横幅 / 侧栏挂 `.no-print`，正文容器挂 `.print-root`。

打印动作统一走 `src/lib/print.ts` 的 `printElement/printWithToast`，不要直接调 `window.print()`：Windows WebView2 支持原生打印；**macOS WKWebView / Linux WebKitGTK 的 `window.print()` 是静默无操作**（Tauri issue #3066），兜底路径是把「打印元素 outerHTML + 收集到的全量 CSS（含 @page 与 @media print）」拼成独立 HTML，经 `print_html` 命令（`commands/print.rs`）写进临时目录 `dictation-print/`，用系统默认浏览器打开，页面加载后自动调起浏览器的打印对话框（打印 / 存为 PDF 都可用）。打印元素靠 class 携带样式，所以克隆 DOM + 全量 CSS 即可复刻排版。

### 字帖的「示范字 / 描红」怎么画

`components/StrokeGlyph.tsx` 用**笔顺数据的轮廓路径**（`lib/strokes.ts::loadStrokePaths`）把字画成 `<svg>` 里的实心 path：

- 好处：不依赖系统装没装楷体（Linux / 精简 Windows 上 `font-family: KaiTi` 会掉成黑体），而且和复习页的动画字形是同一份数据，写到纸上的形状和屏幕上看的完全一致
- 坐标换算是 `HanziWriter.getScalingTransform(100, 100, 10)`，**别手写这个矩阵**：数据是 1024 见方、y 轴朝上，需要 `translate(x, h-y) scale(s, -s)`
- 打印是同步的：页面在加载阶段就把整页用到的字 `Promise.all` 取齐，不能在点「打印」时还有字在路上
- 没有数据的字（字母、数字、生僻字）退回普通文本，用 `fallbackSize` 控制字号

方格：`.ge`（`index.css`）只负责外框——虚线边、`box-sizing: border-box`、`flex: none`（排不下时折行，而不是把格子压扁）。**十字与对角线由 `components/GridLines.tsx` 用 SVG 描边画**，不用 CSS 背景渐变：打印对话框默认关掉「背景图形」，渐变线在纸上会消失，而田 / 米 / 方的区别全在这几条线上（屏幕和打印预览都看不出来，只有出纸才发现）。方格模式（`fang`）不渲染辅助线。相邻格的边框靠 `margin-left: -1px` 叠成一条线。

## 笔画数据（笔顺动画）

错题本的「复习笔画」页用 [hanzi-writer](https://github.com/chanind/hanzi-writer) 播放笔顺，数据来自
`hanzi-writer-data`（Make Me a Hanzi，Arphic Public License）。**不连 CDN**——本应用默认纯离线，见 ADR-18。

同一页的**拼音**与**朗读**也是离线的：拼音用 `pinyin-pro` 纯前端转换（`lib/pinyin.ts`，整词一次转换
做多音字消歧、再与字符对齐，仅此页显示，见 ADR-19）；朗读走后端 `speech/` 的系统语音
（`speak_text` / `stop_speak` 命令，macOS say / Windows SAPI / Linux spd-say）。
自动朗读默认关，偏好存后端设置 `strokeAutoSpeak`（见 ADR-19 修订）：设置页开关与复习页内
「自动朗读」按钮读写同一个值，页内切换是「乐观生效 + 写回 + 失败回滚」。

- `scripts/sync-hanzi-data.mjs` 把 9574 个字打成 **256 个分片**写进 `public/hanzi/`（这是生成物，`.gitignore` 掉了），
  `package.json` 的 `predev` / `prebuild` 会自动跑；缺依赖会直接失败，不会打出「没有笔画」的包
- 分片规则：**码点 % 256** 决定文件名 `s??.json`（`??` 是分片号的十六进制），片内 key 是码点的十六进制
  （`"6625"` 而不是 `"春"`）。前后端各算一次，不需要清单文件；纯 ASCII 路径省掉 Windows / percent-encoding 的一类坑
- 每片约 37 字 / 130KB；`lib/strokes.ts` 缓存已加载的分片，失败不缓存（翻回来能重试）。整包约 33MB，是「离线可看笔顺」付的代价
- **URL 必须相对站点根**（`import.meta.env.BASE_URL` + `hanzi/s??.json`），不要用 `document.baseURI`：
  后者会挂到当前页面所在目录下，换个入口就 404，而 dev server 的 SPA 兜底返回的是 HTML + 200，表现成
  「JSON 解析失败」这种看不出根因的错误
- `components/StrokeWord.tsx`：**初始 `showCharacter: false`**，否则还没轮到的字会先整字显示、轮到再被擦掉重描；
  淡灰轮廓（`showOutline`）当描红底，遮住模式下连轮廓也不画
- 非汉字、没有数据的字**退回静态字形**（框内直接画这个字），不会留白

## 查字工具

第三个顶栏工具「查字」（ADR-22）：家长输入字词，页面逐字出信息卡——田字格笔顺动画（复用 `components/StrokeWord.tsx` 与 `lib/strokes.ts`）+ 逐字拼音（`lib/pinyin.ts`，与笔画复习页同一套）+ 部首 / 笔画数 / 结构 / 释义。`components/CharInfoCard.tsx` 负责单字信息卡；AI 字卡走 `get_char_card` / `generate_char_card` 命令（参数 `character`），未配置 AI 时只显示提示行，不弹窗；长释义默认折叠、点击展开。

字典数据（部首/结构/释义）来自 `lib/dict.ts` 的分片懒加载，规则与笔顺数据同思路（ADR-18）：

- `scripts/sync-dict-data.mjs` 把 vendor 源（`scripts/dict-src/xinhua.json` 精简新华字典 + node_modules 里的 CCD 结构数据）打成 **64 个分片**写进 `public/dict/`（生成物，`.gitignore` 掉了），`predev` / `prebuild` 自动跑；约 21153 字 / 实测约 5.5MB
- 分片规则：**码点 % 64** 决定文件名 `s??.json`，片内 key 是码点十六进制——与 `public/hanzi/` 完全同构，只是片数不同
- `.stamp` 绑定源内容哈希（`xinhua-<sha8>+ccd-<版本>-s64a`），换源 / 升级包自动重刷；URL 同样必须相对站点根（`import.meta.env.BASE_URL`）
- 结构标签来自 CCD 的部件规则表转教学标签（CCD 是几何拆分，须看部件再定包围结构），约 21094 条；罕见复合（叕 / 弼 / \*）不产出，显示「—」
- 笔画数取笔顺数据 `strokes.length`（精确），不重复打包；查不到的字段一律显示「—」，不引第二数据源

### 验收路径

1. 顶栏出现「查字」，进入后侧栏高亮「查字」
2. 查「碧绿」：逐字笔顺动画 + 拼音 + 两张信息卡（碧：石 / 14 画 / 上下结构；绿：纟 / 11 画 / 左右结构）
3. 点田字格只重播该字；「重播」重播整词；「朗读」读整词；离开页面声音停止
4. 输入「爸爸」只出一张卡（同字去重）
5. 纯非汉字输入（字母 / 数字 / 标点）toast「请输入汉字」
6. AI 未配置显示提示行；配置后生成字卡、再次进入秒开（读 `char_cards` 缓存）；生成中按钮防连点
7. 长释义折叠，点击展开
8. 生僻字（字典无条目）退静态字形，不白屏

## 通用组件

| 组件 | 用途 |
|---|---|
| `components/Modal.tsx` | 遮罩弹窗骨架，Esc 关闭 |
| `components/ConfirmModal.tsx` | 危险操作确认，支持左下的第三个动作（如「放弃这次」） |
| `components/Toast.tsx` | `useNotice()` 返回 `{notice, notify}`，4 秒自动消失 |
| `components/GroupModal.tsx` / `PasteImportModal.tsx` | 分组表单 / 粘贴解析预览（预览走后端 `preview_words`） |
| `components/MnemonicCard.tsx` | AI 巧记讲解卡（ADR-20）：内容层（积木/口诀/例句）与辨析层分开呈现，展开可自动读口诀，遮蔽模式整卡隐藏 |
| `components/AiSettings.tsx` | AI 讲解配置区块：OpenAI 兼容服务地址/模型/key（key 只回掩码）、最大输出 tokens（留空 = 不传 `max_tokens`，思考型模型建议 100000+）、测试连接、清除确认；配置存数据目录 `.ai-config.json` |

## 约定

- 后端调用全部走 `src/lib/ipc.ts`；错误统一 `errorMessage(e)`
- 时间展示用 `formatTime`（MM-DD HH:mm）与 `practiceLabel`（今天/昨天/N 天前练过）
- 破坏性操作一律自建弹窗，不用 `window.confirm`
- 状态机与业务流程放页面内，不引状态库（无 redux/zustand）
- 打印相关类名：`no-print`（打印时隐藏）、`print-page`（去阴影去宽度）、`print-row`（避免跨页断开）、`ge`（方格外框，辅助线见 `GridLines`）

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
