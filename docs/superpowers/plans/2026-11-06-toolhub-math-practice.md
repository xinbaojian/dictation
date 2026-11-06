# 小工具集合改造 + 口算练习 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把应用改造成「一级顶栏工具切换 + 左侧二级菜单」的工具集合，生词默写成为其中一个工具，新增口算练习工具（Rust 出题引擎 + A4 打印卷）。

**Architecture:** 前端壳层（App.tsx）重构为「顶栏工具 tab + 工具内二级侧栏」，路由仍是手写联合类型（不引路由库）；口算出题逻辑移植为 Rust `domain/math.rs` 纯函数（mulberry32 种子随机、三段难度候选池、四位置唯一解校验、30 题组卷评分），经一个 Tauri 命令暴露，前端只渲染；打印 `@page` 由各打印页自声明（默写 12mm / 口算 0），替换原全局声明。

**Tech Stack:** Tauri 2 + React 18 + TS + Tailwind 4（前端无新依赖）；Rust + serde（后端无新 crate）。

**设计文档:** `docs/superpowers/specs/2026-11-06-toolhub-math-practice-design.md`

## Global Constraints

- 全程中文；提交信息用仓库现有风格（`feat:` / `chore:` / `docs:` 前缀）。
- 遵守 AGENTS.md 硬约束：不改 identifier（`com.dictation.desktop`）；不碰 updater 公钥；不加 `mistakes` 表；学习进度只走 `repo/word_progress.rs`；前端不拼 SQL，一律走 `src/lib/ipc.ts`。
- 前端 camelCase ↔ Rust snake_case 由 Tauri 自动转换；新命令按链路 `repo/domain → commands → lib.rs 注册 → ipc.ts → types.ts`。
- 不新增任何 npm 依赖、不新增 Rust crate（serde/std 已够）。
- 时间一律 SQLite `strftime`（本计划不涉库表，无新 SQL）。
- 每个 Task 结束跑该 Task 的验证命令并提交；全部完成后走 AGENTS.md 审查流程（干净子代理审查）。
- 分支：`feature/toolhub`（基于 main）。

---

### Task 1: 工具注册表 + 壳层重构（顶栏一级 / 侧栏二级）

**Files:**
- Create: `src/tools/registry.ts`
- Create: `src/pages/math/MathSheet.tsx`（占位，Task 5 替换为真页面）
- Modify: `src/App.tsx`（整体重写壳层）
- Modify: `src-tauri/tauri.conf.json`（窗口标题）

**Interfaces:**
- Produces: `Route` 联合类型（`{ tool: "dictation" | "math"; page: … } | { name: "settings" }`）、`TOOLS`、`NAV_BY_TOOL`，供 App.tsx 与后续所有页面跳转使用。

- [x] **Step 1: 新建 `src/tools/registry.ts`**

```ts
/** 工具注册表：顶栏一级菜单与各工具的二级侧栏菜单都登记在这里。
 *  加第 3 个工具：Route 加变体 + TOOLS / NAV_BY_TOOL 各加一项 + App.tsx 挂页面。 */

export type ToolId = "dictation" | "math";

/** 全局路由：tool 嵌套各工具页面；settings 是全局页（顶栏入口，不嵌套） */
export type Route =
  // ── 生词默写 ──
  | { tool: "dictation"; page: "pick" }
  | { tool: "dictation"; page: "wordbook" }
  | { tool: "dictation"; page: "mistakes" }
  | { tool: "dictation"; page: "history" }
  | { tool: "dictation"; page: "quiz"; sessionId: number }
  | { tool: "dictation"; page: "review"; sessionId: number }
  | { tool: "dictation"; page: "print"; mode: "mistakes" | "group"; groupId: number | null; title: string }
  // ── 口算练习 ──
  | { tool: "math"; page: "sheet" }
  // ── 全局 ──
  | { name: "settings" };

export interface ToolDef {
  id: ToolId;
  /** 顶栏 tab 文案 */
  label: string;
  /** 点顶栏 tab 进入的默认路由 */
  home: Route;
}

export const TOOLS: ToolDef[] = [
  { id: "dictation", label: "生词默写", home: { tool: "dictation", page: "pick" } },
  { id: "math", label: "口算练习", home: { tool: "math", page: "sheet" } },
];

/** 各工具的二级菜单（侧栏）；「设置」是全局页，不在任何工具菜单里 */
export const NAV_BY_TOOL: Record<ToolId, readonly { page: string; label: string }[]> = {
  dictation: [
    { page: "pick", label: "默写" },
    { page: "wordbook", label: "词表" },
    { page: "mistakes", label: "错题" },
    { page: "history", label: "记录" },
  ],
  math: [{ page: "sheet", label: "出题打印" }],
};
```

- [x] **Step 2: 新建占位页 `src/pages/math/MathSheet.tsx`**

```tsx
/** 口算练习主页（占位：Task 5 替换为完整出题打印页） */
export default function MathSheetPage() {
  return (
    <div className="flex flex-1 items-center justify-center text-sm text-stone-400">
      口算练习 · 出题打印（建设中）
    </div>
  );
}
```

- [x] **Step 3: 重写 `src/App.tsx`**

要点：顶栏（图标 + 工具 tab + 右侧设置带红点）；横幅移到顶栏下方（逻辑不变）；侧栏只渲染当前工具的二级菜单；所有旧页面 props 零改动，`dr`（dictation route 收窄）承载原有挂载判断。

```tsx
import { useEffect, useState } from "react";
import appIconUrl from "../app-icon.svg?url";
import { api } from "./lib/ipc";
import { useAutoUpdate } from "./lib/updater";
import History from "./pages/History";
import MathSheetPage from "./pages/math/MathSheet";
import Mistakes from "./pages/Mistakes";
import Pick from "./pages/Pick";
import PrintSheet from "./pages/PrintSheet";
import Quiz from "./pages/Quiz";
import Review from "./pages/Review";
import SettingsPage from "./pages/Settings";
import Wordbook from "./pages/Wordbook";
import { NAV_BY_TOOL, TOOLS, type Route, type ToolId } from "./tools/registry";

export default function App() {
  const [route, setRoute] = useState<Route>(TOOLS[0].home);
  // 设置是全局页：进了设置后，顶栏仍保持「从哪个工具进来的」高亮
  const [lastTool, setLastTool] = useState<ToolId>("dictation");
  const updater = useAutoUpdate();
  // 启动时看一眼同步状态：拉取后重启失败、或有快照待生效时，用户不该只能靠「自己想起来去设置页看」
  const [syncNotice, setSyncNotice] = useState<{ title: string; message: string } | null>(null);
  // 「知道了」只消音当前这条内容；内容变了（新事件）仍要重新提示
  const [dismissedNotice, setDismissedNotice] = useState<string | null>(null);

  // 各工具 page 取值全局唯一（pick/wordbook/…/sheet），可当路由 key 用
  const routeKey = route.name === "settings" ? "settings" : route.page;

  useEffect(() => {
    // 每次离开设置页都重取一次：用户可能刚在那里「放弃快照 / 清除错误提示」，
    // 只在挂载时读一次会让横幅与实际状态相反
    if (route.name === "settings") return;
    let alive = true;
    (async () => {
      try {
        const s = await api.syncStatus();
        if (!alive) return;
        let next: { title: string; message: string } | null = null;
        if (s.warning) next = { title: "同步配置需要处理", message: s.warning };
        else if (s.lastError?.includes("替换成功"))
          // 库已经换成本次快照（坏库恢复 / 状态没写成）：结果通报，不是失败告警
          next = { title: "云端快照已生效", message: s.lastError };
        else if (s.lastError)
          next = {
            title: s.pending ? "同步快照待生效" : "上次同步没有完成",
            message: s.lastError,
          };
        else if (s.pending)
          next = { title: "同步快照待生效", message: "已下载云端快照，重启应用后生效" };
        // 用户点过「知道了」的那条内容不再弹；新内容（提示变了）照常弹
        setSyncNotice(next && next.message === dismissedNotice ? null : next);
      } catch {
        // 读不到同步状态不影响使用，设置页里有完整提示
      }
    })();
    return () => {
      alive = false;
    };
  }, [routeKey, dismissedNotice]);

  const navigate = (next: Route) => {
    if (next.name !== "settings") setLastTool(next.tool);
    setRoute(next);
  };
  const startSession = (sessionId: number) => navigate({ tool: "dictation", page: "quiz", sessionId });
  const openReview = (sessionId: number) => navigate({ tool: "dictation", page: "review", sessionId });

  const activeTool: ToolId = route.name === "settings" ? lastTool : route.tool;

  // 侧栏高亮：quiz/review 归「默写」，打印页按来源归「词表 / 错题」
  let activeNav: string;
  if (route.name === "settings") activeNav = "";
  else if (route.tool === "math") activeNav = "sheet";
  else if (route.page === "quiz" || route.page === "review") activeNav = "pick";
  else if (route.page === "print") activeNav = route.mode === "mistakes" ? "mistakes" : "wordbook";
  else activeNav = route.page;

  // 生词默写路由收窄（其它工具 / 设置页时为 null），页面挂载判断都从它取
  const dr = route.name === "settings" || route.tool === "math" ? null : route;

  const updateAvailable = updater.state.phase === "available" && !updater.state.dismissed;
  // 默写、批改、打印、设置页不弹横幅：前者不能打断孩子，设置页自己有完整卡片
  const quietRoute = ["quiz", "review", "print", "settings"].includes(routeKey);
  const showUpdateBanner = updateAvailable && !quietRoute;
  const showSyncBanner = !!syncNotice && !quietRoute;
  const settingsDot = updateAvailable || !!syncNotice;

  return (
    <div className="flex h-full flex-col">
      <header className="no-print flex h-11 shrink-0 items-center gap-1 border-b border-stone-200 bg-white px-3">
        <img src={appIconUrl} alt="" draggable={false} className="mr-2 h-6 w-6 select-none" />
        {TOOLS.map((t) => (
          <button
            key={t.id}
            onClick={() => navigate(t.home)}
            className={
              "rounded-md px-3 py-1.5 text-xs transition " +
              (activeTool === t.id
                ? "bg-stone-900 font-medium text-white"
                : "text-stone-500 hover:bg-stone-100")
            }
          >
            {t.label}
          </button>
        ))}
        <button
          onClick={() => setRoute({ name: "settings" })}
          title="设置"
          className={
            "relative ml-auto rounded-md px-3 py-1.5 text-xs transition " +
            (route.name === "settings"
              ? "bg-stone-900 font-medium text-white"
              : "text-stone-400 hover:bg-stone-100")
          }
        >
          设置
          {settingsDot && (
            <span className="absolute right-1 top-1 h-2 w-2 rounded-full bg-amber-500" />
          )}
        </button>
      </header>

      {showUpdateBanner && (
        <div className="no-print flex shrink-0 items-center gap-3 border-b border-amber-200 bg-amber-50 px-6 py-2 text-xs text-amber-900">
          <span className="shrink-0 font-medium">发现新版本 v{updater.state.version}</span>
          <span className="hidden min-w-0 flex-1 truncate text-amber-700 sm:block">
            更新包已发布到 GitHub Release，可在设置中下载安装
          </span>
          <button
            onClick={() => setRoute({ name: "settings" })}
            className="shrink-0 rounded bg-amber-600 px-3 py-1 font-medium text-white hover:bg-amber-700"
          >
            去更新
          </button>
          <button
            onClick={updater.dismiss}
            className="shrink-0 rounded px-3 py-1 text-amber-700 hover:bg-amber-100"
          >
            稍后
          </button>
        </div>
      )}

      {showSyncBanner && (
        <div className="no-print flex shrink-0 items-center gap-3 border-b border-amber-200 bg-amber-50 px-6 py-2 text-xs text-amber-900">
          <span className="shrink-0 font-medium">{syncNotice!.title}</span>
          <span className="hidden min-w-0 flex-1 truncate text-amber-700 sm:block">
            {syncNotice!.message}
          </span>
          <button
            onClick={() => setRoute({ name: "settings" })}
            className="shrink-0 rounded bg-amber-600 px-3 py-1 font-medium text-white hover:bg-amber-700"
          >
            去设置
          </button>
          <button
            onClick={() => {
              setDismissedNotice(syncNotice!.message);
              setSyncNotice(null);
            }}
            className="shrink-0 rounded px-3 py-1 text-amber-700 hover:bg-amber-100"
          >
            知道了
          </button>
        </div>
      )}

      <div className="flex min-h-0 flex-1">
        <nav className="no-print flex w-16 shrink-0 flex-col items-center gap-1 border-r border-stone-200 bg-white py-3">
          {NAV_BY_TOOL[activeTool].map((item) => (
            <button
              key={item.page}
              onClick={() => navigate(
                activeTool === "math"
                  ? { tool: "math", page: "sheet" }
                  : ({ tool: "dictation", page: item.page } as Route),
              )}
              className={
                "w-11 rounded-md py-2 text-xs transition " +
                (activeNav === item.page
                  ? "bg-stone-900 font-medium text-white"
                  : "text-stone-500 hover:bg-stone-100")
              }
            >
              {item.label}
            </button>
          ))}
        </nav>

        <main className="flex min-w-0 flex-1 flex-col">
          {dr?.page === "wordbook" && (
            <Wordbook
              onPrint={(groupId, title) =>
                navigate({ tool: "dictation", page: "print", mode: "group", groupId, title })
              }
            />
          )}
          {dr?.page === "mistakes" && (
            <Mistakes
              onStart={startSession}
              onGoWordbook={() => navigate({ tool: "dictation", page: "wordbook" })}
              onPrint={(groupId, title) =>
                navigate({ tool: "dictation", page: "print", mode: "mistakes", groupId, title })
              }
            />
          )}
          {dr?.page === "print" && (
            <PrintSheet
              mode={dr.mode}
              groupId={dr.groupId}
              title={dr.title}
              onBack={() =>
                navigate(
                  dr.mode === "mistakes"
                    ? { tool: "dictation", page: "mistakes" }
                    : { tool: "dictation", page: "wordbook" },
                )
              }
            />
          )}
          {dr?.page === "history" && <History onStart={startSession} onReview={openReview} />}
          {route.name === "settings" && <SettingsPage updater={updater} />}
          {dr?.page === "quiz" && (
            <Quiz
              key={dr.sessionId}
              sessionId={dr.sessionId}
              onFinish={openReview}
              onExit={() => navigate({ tool: "dictation", page: "pick" })}
            />
          )}
          {dr?.page === "review" && (
            <Review
              key={dr.sessionId}
              sessionId={dr.sessionId}
              onDone={() => navigate({ tool: "dictation", page: "pick" })}
              onBack={() => navigate({ tool: "dictation", page: "pick" })}
            />
          )}
          {dr?.page === "pick" && (
            <Pick
              onStart={startSession}
              onReview={openReview}
              onGoWordbook={() => navigate({ tool: "dictation", page: "wordbook" })}
            />
          )}
          {route.name !== "settings" && route.tool === "math" && <MathSheetPage />}
        </main>
      </div>
    </div>
  );
}
```

- [x] **Step 4: 改窗口标题**：`src-tauri/tauri.conf.json` 里 `"title": "生词默写"` → `"title": "学习小工具"`（productName 与 identifier 不动，硬约束 8）。

- [x] **Step 5: 验证**：`npm run typecheck` 通过；`npm run build` 通过。

- [x] **Step 6: 提交**

```bash
git add src/tools/registry.ts src/pages/math/MathSheet.tsx src/App.tsx src-tauri/tauri.conf.json
git commit -m "feat: 壳层改造为工具集合（顶栏一级工具 + 侧栏二级菜单）"
```

---

### Task 2: 打印 @page 归一（各打印页自声明边距）

**背景**：原全局 `@page { size: A4; margin: 12mm }` 对「每一页」生效；口算卷要满幅 0 边距。全局改 0 会让默写单第 2 页起内容顶到纸边（页容器 padding 只在整体首尾生效）。因此删全局声明，由各打印页自渲染 `<style>` 声明自己的 `@page`。

**Files:**
- Modify: `src/index.css`（删除全局 `@page` 块）
- Modify: `src/pages/PrintSheet.tsx`（渲染 12mm 声明）

**Interfaces:**
- Produces: 打印页自声明 `@page` 的模式（Task 5 的口算页用同样的方式声明 `margin: 0`）。

- [x] **Step 1: `src/index.css` 删除这个块**（其余打印规则全部保留不动）：

```css
@page {
  size: A4;
  margin: 12mm;
}
```

- [x] **Step 2: `src/pages/PrintSheet.tsx`** 在组件返回的根 `<div>` 内第一个子元素位置加：

```tsx
{/* 本页打印边距自声明：@page 无法按 class 作用域隔离，由各打印页自己渲染（口算卷是 margin:0 满幅） */}
<style>{"@page { size: A4; margin: 12mm }"}</style>
```

- [x] **Step 3: 验证**：`npm run typecheck` 通过。行为等价性说明：原 `@page 12mm` + `.print-page` 打印时 padding 清零 ≡ 现在 `@page`（本页声明 12mm）+ 同样的打印规则。

- [x] **Step 4: 提交**

```bash
git add src/index.css src/pages/PrintSheet.tsx
git commit -m "refactor: 打印边距改为各打印页自声明，为满幅口算卷让路"
```

---

### Task 3: Rust 出题引擎 `domain/math.rs`（TDD）

**Files:**
- Create: `src-tauri/src/domain/math.rs`（实现 + 单测同文件，仓库惯例）
- Modify: `src-tauri/src/domain/mod.rs`（加 `pub mod math;`）

**Interfaces:**
- Produces: `math::generate(seed: u32, pos: BlankSel, op1: Op1Sel, op2: Op2Sel) -> Option<MathSheet>`；`BlankSel/Op1Sel/Op2Sel::parse(&str) -> AppResult<Self>`；`MathSheet { questions: Vec<MathQuestion>, blanks: Vec<u8> }`；`MathQuestion { a, b, c, op1: Op, op2: Op, t, d }`（Op 序列化为 `"m"|"d"|"a"|"s"`）。Task 4 的命令与 Task 5 的前端消费这些签名。

**移植来源**：`生成口算练习.py` 页内 JS 出题器。**确定性约定**：同 seed + 同选项 ⇒ 同一张卷（含留空位置）。随机数调用次序必须与 JS 一致（阈值判断短路、索引取值各一次）。

- [x] **Step 1: 写类型骨架 + 全部测试（红）**

`src-tauri/src/domain/math.rs` 先放类型、`todo!()` 实现与 `#[cfg(test)] mod tests`：

```rust
//! 口算练习出题引擎：三数两步逆推（① op1 ② op2 ③ = ④，从左到右两步计算）。
//! 移植自口算练习 HTML 出题器；纯函数、无 IO。
//! 确定性约定：同 seed + 同选项 ⇒ 完全同一张卷（题目与留空位置）。

use crate::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// 运算符（序列化与前端约定：m 乘 / d 除 / a 加 / s 减）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Op {
    #[serde(rename = "m")]
    Mul,
    #[serde(rename = "d")]
    Div,
    #[serde(rename = "a")]
    Add,
    #[serde(rename = "s")]
    Sub,
}

/// 留空位置选择
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlankSel {
    One,
    Two,
    Three,
    Four,
    Random,
}

/// 第一步运算符选择
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op1Sel {
    Fixed(Op),
    Random,
}

/// 第二步运算符选择（只有加减；「除/减 配 减」无解，由候选池过滤兜底）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op2Sel {
    Add,
    Sub,
    Random,
}

/// 一道题：a op1 b = t，t op2 c = d（t 为中间结果）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct MathQuestion {
    pub a: i32,
    pub b: i32,
    pub c: i32,
    pub op1: Op,
    pub op2: Op,
    pub t: i32,
    pub d: i32,
}

/// 一张卷：30 题与每题留空位置（1-4）
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MathSheet {
    pub questions: Vec<MathQuestion>,
    pub blanks: Vec<u8>,
}

impl BlankSel {
    pub fn parse(raw: &str) -> AppResult<Self> {
        match raw {
            "1" => Ok(BlankSel::One),
            "2" => Ok(BlankSel::Two),
            "3" => Ok(BlankSel::Three),
            "4" => Ok(BlankSel::Four),
            "random" => Ok(BlankSel::Random),
            other => Err(AppError::msg(format!("未知的留空位置：{other}"))),
        }
    }
}

impl Op1Sel {
    pub fn parse(raw: &str) -> AppResult<Self> {
        match raw {
            "m" => Ok(Op1Sel::Fixed(Op::Mul)),
            "d" => Ok(Op1Sel::Fixed(Op::Div)),
            "a" => Ok(Op1Sel::Fixed(Op::Add)),
            "s" => Ok(Op1Sel::Fixed(Op::Sub)),
            "random" => Ok(Op1Sel::Random),
            other => Err(AppError::msg(format!("未知的运算符：{other}"))),
        }
    }
}

impl Op2Sel {
    pub fn parse(raw: &str) -> AppResult<Self> {
        match raw {
            "a" => Ok(Op2Sel::Add),
            "s" => Ok(Op2Sel::Sub),
            "random" => Ok(Op2Sel::Random),
            other => Err(AppError::msg(format!("未知的第二步运算符：{other}"))),
        }
    }
}

/// 出题。返回 None 表示当前符号组合无解（前端提示换运算符）。
pub fn generate(_seed: u32, _pos: BlankSel, _op1: Op1Sel, _op2: Op2Sel) -> Option<MathSheet> {
    todo!()
}
```

测试模块（同文件末尾）：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const SEED: u32 = 20261105;

    fn sheet(pos: BlankSel, op1: Op1Sel, op2: Op2Sel) -> MathSheet {
        generate(SEED, pos, op1, op2).expect("默认组合应能出满 30 题")
    }

    /// 测试内独立重算：留空位置 p 是否唯一解（不复用实现里的 unique_at）
    fn solution_count(q: &MathQuestion, p: u8) -> i32 {
        let (lo, hi) = match p {
            1 | 2 => (1, 9),
            3 => (10, 50),
            _ => (1, 100),
        };
        let mut sols = 0;
        for v in lo..=hi {
            let (a, b, c) = (q.a, q.b, q.c);
            let (a, b, c, d) = match p {
                1 => (v, b, c, q.d),
                2 => (a, v, c, q.d),
                3 => (a, b, v, q.d),
                _ => (a, b, c, v),
            };
            let t = match q.op1 {
                Op::Add => a + b,
                Op::Sub => a - b,
                Op::Mul => a * b,
                Op::Div => {
                    if b != 0 && a % b == 0 { a / b } else { continue }
                }
            };
            if t < 1 { continue; }
            let d2 = match q.op2 {
                Op::Add => t + c,
                _ => t - c,
            };
            if d2 == d { sols += 1; }
        }
        sols
    }

    #[test]
    fn same_seed_same_sheet() {
        let s1 = sheet(BlankSel::Two, Op1Sel::Fixed(Op::Mul), Op2Sel::Random);
        let s2 = sheet(BlankSel::Two, Op1Sel::Fixed(Op::Mul), Op2Sel::Random);
        assert_eq!(s1, s2);
    }

    #[test]
    fn thirty_unique_questions() {
        let s = sheet(BlankSel::Two, Op1Sel::Fixed(Op::Mul), Op2Sel::Random);
        assert_eq!(s.questions.len(), 30);
        assert_eq!(s.blanks.len(), 30);
        let keys: HashSet<_> = s.questions.iter().map(|q| (q.a, q.b, q.c, q.op1, q.op2, q.d)).collect();
        assert_eq!(keys.len(), 30, "同一张卷里不应有重复题");
    }

    #[test]
    fn three_difficulty_stages() {
        let s = sheet(BlankSel::Two, Op1Sel::Fixed(Op::Mul), Op2Sel::Random);
        let ranges = [(1, 4), (3, 6), (5, 9)];
        for (i, q) in s.questions.iter().enumerate() {
            let (lo, hi) = ranges[i / 10];
            assert!((lo..=hi).contains(&q.a), "第 {} 题不在难度段内", i + 1);
        }
    }

    #[test]
    fn unique_solution_at_every_blank() {
        let s = sheet(BlankSel::Random, Op1Sel::Random, Op2Sel::Random);
        for q in &s.questions {
            for p in 1..=4u8 {
                assert_eq!(solution_count(q, p), 1, "{:?} 在位置 {} 不是唯一解", q, p);
            }
        }
    }

    #[test]
    fn value_ranges_hold() {
        let s = sheet(BlankSel::Two, Op1Sel::Random, Op2Sel::Random);
        for q in &s.questions {
            assert!((1..=9).contains(&q.a) && (1..=9).contains(&q.b));
            assert!((10..=50).contains(&q.c));
            assert!((1..=100).contains(&q.d));
            assert!(q.t >= 1);
            if q.op1 == Op::Div {
                assert!(q.b != 0 && q.a % q.b == 0, "除法必须表内整除");
            }
        }
    }

    #[test]
    fn div_then_sub_is_unsolvable() {
        assert!(generate(SEED, BlankSel::Two, Op1Sel::Fixed(Op::Div), Op2Sel::Sub).is_none());
        assert!(generate(SEED, BlankSel::Two, Op1Sel::Fixed(Op::Sub), Op2Sel::Sub).is_none());
    }

    #[test]
    fn random_mode_skips_unsolvable_combos() {
        // 第一步随机 + 第二步减：应跳过（除,减）与（减,减）仍出满 30 题
        let s = sheet(BlankSel::Two, Op1Sel::Random, Op2Sel::Sub);
        for q in &s.questions {
            assert!(q.op2 == Op::Sub);
            assert!(q.op1 == Op::Mul || q.op1 == Op::Add);
        }
    }

    #[test]
    fn random_blanks_cover_all_positions() {
        let s = sheet(BlankSel::Random, Op1Sel::Fixed(Op::Mul), Op2Sel::Random);
        let mut counts = [0usize; 5];
        for &b in &s.blanks {
            assert!((1..=4).contains(&b));
            counts[b as usize] += 1;
        }
        for p in 1..=4 {
            assert!((7..=8).contains(&counts[p]), "位置 {} 配额应为 7-8 题", p);
        }
    }

    #[test]
    fn fixed_blank_position() {
        let s = sheet(BlankSel::Three, Op1Sel::Fixed(Op::Mul), Op2Sel::Random);
        assert!(s.blanks.iter().all(|&b| b == 3));
    }

    #[test]
    fn different_seed_different_sheet() {
        let s1 = sheet(BlankSel::Two, Op1Sel::Fixed(Op::Mul), Op2Sel::Random);
        let s2 = generate(SEED + 1, BlankSel::Two, Op1Sel::Fixed(Op::Mul), Op2Sel::Random)
            .expect("相邻 seed 也应能出卷");
        assert_ne!(s1.questions, s2.questions);
    }

    #[test]
    fn parse_rejects_unknown_values() {
        assert!(BlankSel::parse("5").is_err());
        assert!(Op1Sel::parse("x").is_err());
        assert!(Op2Sel::parse("m").is_err());
    }
}
```

`src-tauri/src/domain/mod.rs` 加一行：`pub mod math;`

- [x] **Step 2: 跑测试确认红**

Run: `cd src-tauri && cargo test math`
Expected: 编译通过、运行时 panic（todo!()）——`same_seed_same_sheet` 等 10 个用例失败。

- [x] **Step 3: 实现（替换 todo!()，加入以下私有实现）**

```rust
/// mulberry32：与原 JS 出题器逐位一致（wrapping 运算、无符号右移）
struct Rng(u32);

impl Rng {
    fn new(seed: u32) -> Self {
        Rng(seed)
    }

    fn next_f64(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x6D2B79F5);
        let mut t = self.0;
        t = (t ^ (t >> 15)).wrapping_mul(1 | t);
        t = t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t));
        ((t ^ (t >> 14)) as f64) / 4294967296.0
    }
}

/// Fisher-Yates 洗牌（与 JS shuffle 逐调用一致）
fn shuffle<T>(arr: &mut [T], rnd: &mut Rng) {
    for i in (1..arr.len()).rev() {
        let j = ((rnd.next_f64() * (i as f64 + 1.0)) as usize).min(i);
        arr.swap(i, j);
    }
}

fn calc(x: i32, op: Op, y: i32) -> Option<i32> {
    match op {
        Op::Add => Some(x + y),
        Op::Sub => Some(x - y),
        Op::Mul => Some(x * y),
        // 表内除法：只保留整除
        Op::Div => {
            if y != 0 && x % y == 0 {
                Some(x / y)
            } else {
                None
            }
        }
    }
}

/// 从左到右两步：t = a op1 b（≥1），d = t op2 c（1..=100）
fn eval_q(a: i32, b: i32, c: i32, o1: Op, o2: Op) -> Option<(i32, i32)> {
    let t = calc(a, o1, b)?;
    if t < 1 {
        return None;
    }
    let d = calc(t, o2, c)?;
    if !(1..=100).contains(&d) {
        return None;
    }
    Some((t, d))
}

/// 三段难度：(a 范围, c 范围)，段内 a,b ∈ 1-9
const STAGES: [(i32, i32, i32, i32); 3] = [(1, 4, 10, 30), (3, 6, 10, 50), (5, 9, 10, 50)];

fn pool(stage: usize, o1: Op, o2: Op) -> Vec<MathQuestion> {
    let (a0, a1, c0, c1) = STAGES[stage];
    let mut v = Vec::new();
    for a in a0..=a1 {
        for b in 1..=9 {
            for c in c0..=c1 {
                if let Some((t, d)) = eval_q(a, b, c, o1, o2) {
                    v.push(MathQuestion { a, b, c, op1: o1, op2: o2, t, d });
                }
            }
        }
    }
    v
}

/// 只保留三段都有足够候选的符号组合（例如「除后减」中间结果 ≤9，减不去 ≥10 的数）
fn combos_for(o1s: Op1Sel, o2s: Op2Sel) -> Vec<(Op, Op)> {
    const O1: [Op; 4] = [Op::Mul, Op::Div, Op::Add, Op::Sub];
    const O2: [Op; 2] = [Op::Add, Op::Sub];
    let l1: &[Op] = match o1s {
        Op1Sel::Random => &O1,
        Op1Sel::Fixed(op) => {
            if op == Op::Add || op == Op::Sub || op == Op::Mul || op == Op::Div {
                // 单元素切片：直接按匹配返回
                match op {
                    Op::Mul => &[Op::Mul],
                    Op::Div => &[Op::Div],
                    Op::Add => &[Op::Add],
                    Op::Sub => &[Op::Sub],
                }
            } else {
                &O1
            }
        }
    };
    let l2: &[Op] = match o2s {
        Op2Sel::Random => &O2,
        Op2Sel::Add => &[Op::Add],
        Op2Sel::Sub => &[Op::Sub],
    };
    let mut out = Vec::new();
    for &o1 in l1 {
        for &o2 in l2 {
            if (0..3).all(|s| pool(s, o1, o2).len() >= 8) {
                out.push((o1, o2));
            }
        }
    }
    out
}

/// 留空位置 p 是否唯一解（暴力枚举该位置的取值域）
fn unique_at(q: &MathQuestion, p: u8) -> bool {
    let (lo, hi) = match p {
        1 | 2 => (1, 9),
        3 => (10, 50),
        _ => (1, 100),
    };
    let mut sols = 0;
    for v in lo..=hi {
        let (a, b, c, d) = (q.a, q.b, q.c, q.d);
        let (a, b, c, d) = match p {
            1 => (v, b, c, d),
            2 => (a, v, c, d),
            3 => (a, b, v, d),
            _ => (a, b, c, v),
        };
        if let Some((_, dd)) = eval_q(a, b, c, q.op1, q.op2) {
            if dd == d {
                sols += 1;
            }
        }
    }
    sols == 1
}

fn unique_all(q: &MathQuestion) -> bool {
    (1..=4u8).all(|p| unique_at(q, p))
}

/// 组卷：符号按配额打乱、题目去重、相邻题 b/d 错开；失败返回 None
fn build(seed: u32, combos: &[(Op, Op)]) -> Option<Vec<MathQuestion>> {
    let mut plan: Vec<(Op, Op)> = (0..30).map(|i| combos[i % combos.len()]).collect();
    shuffle(&mut plan, &mut Rng::new(seed ^ 0x9E3779B9));

    // 预计算每个组合的三段池与总池，避免 1500 次重试里反复枚举
    let mut stage_pools: HashMap<(Op, Op), [Vec<MathQuestion>; 3]> = HashMap::new();
    let mut all_pools: HashMap<(Op, Op), Vec<MathQuestion>> = HashMap::new();
    for &(o1, o2) in combos {
        let stages = [pool(0, o1, o2), pool(1, o1, o2), pool(2, o1, o2)];
        let all = [stages[0].clone(), stages[1].clone(), stages[2].clone()].concat();
        stage_pools.insert((o1, o2), stages);
        all_pools.insert((o1, o2), all);
    }

    let mut rnd = Rng::new(seed);
    let mut qs: Vec<MathQuestion> = Vec::new();
    let mut seen: HashSet<(i32, i32, i32, Op, Op, i32)> = HashSet::new();
    for i in 0..30 {
        let s = i / 10;
        let cb = plan[i];
        let p = &stage_pools[&cb][s];
        let ap = &all_pools[&cb];
        let mut got: Option<MathQuestion> = None;
        for _ in 0..1500 {
            // 与 JS 随机数调用次序一致：先阈值（段池足够大才掷点，短路），再索引
            let src = if p.len() >= 30 && rnd.next_f64() < 0.8 { p } else { ap };
            let q = match src.get((rnd.next_f64() * src.len() as f64) as usize) {
                Some(q) => *q,
                None => continue,
            };
            let key = (q.a, q.b, q.c, q.op1, q.op2, q.d);
            if seen.contains(&key) {
                continue;
            }
            if let Some(last) = qs.last() {
                if q.b == last.b || q.d == last.d {
                    continue;
                }
            }
            got = Some(q);
            break;
        }
        let q = got?;
        if !unique_all(&q) {
            return None;
        }
        seen.insert((q.a, q.b, q.c, q.op1, q.op2, q.d));
        qs.push(q);
    }
    Some(qs)
}

/// 分布评分：c/d 覆盖越宽、b 与 d 取值越多越好
fn score(qs: &[MathQuestion]) -> f64 {
    let mut cmin = i32::MAX;
    let mut cmax = i32::MIN;
    let mut dmin = i32::MAX;
    let mut dmax = i32::MIN;
    let mut bs: HashSet<i32> = HashSet::new();
    let mut ds: HashSet<i32> = HashSet::new();
    for q in qs {
        cmin = cmin.min(q.c);
        cmax = cmax.max(q.c);
        dmin = dmin.min(q.d);
        dmax = dmax.max(q.d);
        bs.insert(q.b);
        ds.insert(q.d);
    }
    (cmax - cmin) as f64
        + (dmax - dmin) as f64 * 0.6
        + bs.len() as f64 * 3.0
        + ds.len() as f64 * 0.4
}

/// 24 个相邻 seed 各出一卷，取分布最好的
fn build_best(seed: u32, combos: &[(Op, Op)]) -> Option<Vec<MathQuestion>> {
    let mut best: Option<(f64, Vec<MathQuestion>)> = None;
    for k in 0..24u32 {
        if let Some(qs) = build(seed.wrapping_add(k), combos) {
            let sc = score(&qs);
            if best.as_ref().map_or(true, |b| sc > b.0) {
                best = Some((sc, qs));
            }
        }
    }
    best.map(|(_, qs)| qs)
}

pub fn generate(seed: u32, pos: BlankSel, op1: Op1Sel, op2: Op2Sel) -> Option<MathSheet> {
    let combos = combos_for(op1, op2);
    let questions = build_best(seed, &combos)?;
    // 留空位置与题目无关（同 seed 换位置不换题）；随机模式四位置配额打乱
    let blanks = match pos {
        BlankSel::Random => {
            let mut arr: Vec<u8> = (0..30).map(|i| (i % 4 + 1) as u8).collect();
            shuffle(&mut arr, &mut Rng::new(seed ^ 0x5BF03635));
            arr
        }
        BlankSel::One => vec![1; 30],
        BlankSel::Two => vec![2; 30],
        BlankSel::Three => vec![3; 30],
        BlankSel::Four => vec![4; 30],
    };
    Some(MathSheet { questions, blanks })
}
```

（`combos_for` 中 `Op1Sel::Fixed(op)` 的分支可简化为直接 `match op { … }` 四臂返回单元素切片，上面的写法保守；实现时用简洁版：）

```rust
let l1: &[Op] = match o1s {
    Op1Sel::Random => &O1,
    Op1Sel::Fixed(Op::Mul) => &[Op::Mul],
    Op1Sel::Fixed(Op::Div) => &[Op::Div],
    Op1Sel::Fixed(Op::Add) => &[Op::Add],
    Op1Sel::Fixed(Op::Sub) => &[Op::Sub],
};
```

- [x] **Step 4: 跑测试确认绿**

Run: `cd src-tauri && cargo test math`
Expected: 11 个用例全部 PASS。

- [x] **Step 5: clippy**

Run: `cd src-tauri && cargo clippy --all-targets`
Expected: 无新增警告。

- [x] **Step 6: 提交**

```bash
git add src-tauri/src/domain/math.rs src-tauri/src/domain/mod.rs
git commit -m "feat: 口算出题引擎（三数两步逆推，seed 可复现，四位置唯一解）"
```

---

### Task 4: 命令注册 + IPC + 类型

**Files:**
- Create: `src-tauri/src/commands/math.rs`
- Modify: `src-tauri/src/commands/mod.rs`（`pub mod math;`）
- Modify: `src-tauri/src/lib.rs`（invoke_handler 加 `commands::math::generate_math_sheet,`，放 quiz 组之后）
- Modify: `src/lib/ipc.ts`（口算分组）
- Modify: `src/types.ts`（`MathQuestion` / `MathSheet`）

**Interfaces:**
- Consumes: Task 3 的 `math::{generate, BlankSel, Op1Sel, Op2Sel, MathSheet}`。
- Produces: `api.generateMathSheet(seed, pos, op1, op2) → Promise<MathSheet | null>`（Task 5 消费）；TS 类型 `MathQuestion { a,b,c,op1,op2,t,d }`、`MathSheet { questions, blanks }`。

- [x] **Step 1: `src-tauri/src/commands/math.rs`**

```rust
use crate::domain::math::{self, BlankSel, Op1Sel, Op2Sel};
use crate::error::AppResult;

/// 出一张口算卷（30 题 + 每题留空位置）。
/// pos: "1"|"2"|"3"|"4"|"random"；op1: "m"|"d"|"a"|"s"|"random"；op2: "a"|"s"|"random"。
/// 当前符号组合无解时返回 null，由前端引导换运算符。
#[tauri::command]
pub fn generate_math_sheet(seed: u32, pos: String, op1: String, op2: String) -> AppResult<Option<math::MathSheet>> {
    let pos = BlankSel::parse(&pos)?;
    let op1 = Op1Sel::parse(&op1)?;
    let op2 = Op2Sel::parse(&op2)?;
    Ok(math::generate(seed, pos, op1, op2))
}
```

- [x] **Step 2: 注册**：`commands/mod.rs` 按字母序加 `pub mod math;`（在 library 前）；`lib.rs` 的 `generate_handler![...]` 里 `commands::quiz::add_mistake,` 之后加一行 `commands::math::generate_math_sheet,`。

- [x] **Step 3: `src/types.ts` 末尾追加**

```ts
/** 口算练习：一道三数两步逆推题（① op1 ② op2 ③ = ④，从左到右计算） */
export interface MathQuestion {
  a: number;
  b: number;
  c: number;
  op1: "m" | "d" | "a" | "s";
  op2: "m" | "d" | "a" | "s";
  /** 第一步中间结果 a op1 b */
  t: number;
  /** 得数 */
  d: number;
}

/** 一张口算卷：30 题与每题留空位置（1-4） */
export interface MathSheet {
  questions: MathQuestion[];
  blanks: number[];
}
```

- [x] **Step 4: `src/lib/ipc.ts`** 导入类型加 `MathSheet`，`api` 对象在「错题本」组后加：

```ts
  // 口算练习
  /** 出一张 30 题的口算卷；当前符号组合无解时返回 null */
  generateMathSheet: (seed: number, pos: string, op1: string, op2: string) =>
    invoke<MathSheet | null>("generate_math_sheet", { seed, pos, op1, op2 }),
```

- [x] **Step 5: 验证**：`cd src-tauri && cargo build`；`npm run typecheck`。

- [x] **Step 6: 提交**

```bash
git add src-tauri/src/commands/math.rs src-tauri/src/commands/mod.rs src-tauri/src/lib.rs src/lib/ipc.ts src/types.ts
git commit -m "feat: generate_math_sheet 命令与前端 IPC 接入"
```

---

### Task 5: 口算练习页面（工具条 + A4 卷 + 打印）

**Files:**
- Create: `src/pages/math/example.ts`（示范条：中文数字 / 乘法口诀 / 逆推步骤）
- Create: `src/pages/math/math-sheet.css`（卷面打印样式，`ms-` 前缀，移植自脚本）
- Modify: `src/pages/math/MathSheet.tsx`（替换 Task 1 占位）

**Interfaces:**
- Consumes: `api.generateMathSheet`、`MathSheet/MathQuestion` 类型、Task 2 的自声明 `@page` 模式。

- [x] **Step 1: `src/pages/math/example.ts`**

```ts
/** 口算示范条：中文数字、乘法口诀与「先算能算的一步，再倒着找缺的数」的示范。
 *  移植自口算练习 HTML 出题器；纯前端展示逻辑。 */

export type MathOp = "m" | "d" | "a" | "s";

const SYM: Record<MathOp, string> = { m: "×", d: "÷", a: "+", s: "−" };

const CN = ["零", "一", "二", "三", "四", "五", "六", "七", "八", "九"];

function cnNum(n: number): string {
  if (n < 10) return CN[n];
  if (n === 10) return "一十"; // 「二五一十」，不是「二五十」
  if (n < 20) return "十" + (n % 10 ? CN[n % 10] : "");
  return CN[Math.floor(n / 10)] + "十" + (n % 10 ? CN[n % 10] : "");
}

/** 乘法口诀：小的数在前，积 <10 用「得」 */
function verse(x: number, y: number): string {
  const lo = Math.min(x, y);
  const hi = Math.max(x, y);
  const p = lo * hi;
  return cnNum(lo) + cnNum(hi) + (p < 10 ? "得" : "") + cnNum(p);
}

function evalQ(a: number, b: number, c: number, o1: MathOp, o2: MathOp): { t: number; d: number } | null {
  let t: number;
  if (o1 === "m") t = a * b;
  else if (o1 === "d") {
    if (b === 0 || a % b !== 0) return null;
    t = a / b;
  } else if (o1 === "a") t = a + b;
  else t = a - b;
  if (t < 1) return null;
  const d = o2 === "s" ? t - c : t + c;
  if (d < 1 || d > 100) return null;
  return { t, d };
}

export interface MathExample {
  expr: string;
  /** 示范步骤（结尾「填 X」由调用方渲染强调） */
  step: string;
  /** 最终要填的数 */
  answer: string;
}

export function buildExample(pos: number, o1: MathOp, o2: MathOp): MathExample {
  let A = 8;
  let B = 4;
  let C = 10;
  let e = evalQ(A, B, C, o1, o2);
  if (!e) {
    A = 6;
    B = 3;
    C = 20;
    e = evalQ(A, B, C, o1, o2);
  }
  if (!e) {
    return { expr: "", step: "先看清楚空在哪一位，能算的那一步先算出来，再倒着想缺的数。", answer: "" };
  }
  const { t, d } = e;
  const nums = [A, B, C, d];
  const ans = nums[pos - 1];
  const parts = nums.map((v, k) => (k + 1 === pos ? "___" : String(v)));
  const expr = `${parts[0]} ${SYM[o1]} ${parts[1]} ${SYM[o2]} ${parts[2]} = ${parts[3]}`;
  let step: string;
  if (pos === 4) {
    step = `${A} ${SYM[o1]} ${B} = ${t}，${t} ${SYM[o2]} ${C} = ${d}`;
  } else if (pos === 3) {
    const s1 = `${A} ${SYM[o1]} ${B} = ${t}`;
    const s2 = o2 === "a" ? `${d} − ${t} = ${C}` : `${t} − ${d} = ${C}`;
    step = `${s1}，${s2}`;
  } else {
    const backT = o2 === "a" ? `${d} − ${C} = ${t}` : `${d} + ${C} = ${t}`;
    let back: string;
    if (o1 === "m") back = `想「${pos === 1 ? `几 × ${B}` : `${A} × 几`} = ${t}」→ ${verse(A, B)}`;
    else if (o1 === "d") back = `想 ${pos === 1 ? `${B} × ${t}` : `${t} × ${B}`} = ${A} → ${verse(B, t)}`;
    else if (o1 === "a") back = pos === 1 ? `${t} − ${B} = ${A}` : `${t} − ${A} = ${B}`;
    else back = pos === 1 ? `${t} + ${B} = ${A}` : `${A} − ${t} = ${B}`;
    step = `${backT}，${back}`;
  }
  return { expr, step, answer: String(ans) };
}
```

- [x] **Step 2: `src/pages/math/math-sheet.css`**（移植脚本 CSS，全部加 `ms-` 前缀；屏幕工具条 + A4 卷面 + 打印规则；`@page` 不放这里，由组件渲染）

```css
/* 口算练习卷面样式：移植自口算练习 HTML（mm 尺寸链勿乱动：
   12 + 27 + 3 + 15 + 4 + 200 + 4 + 18 + 10 = 293mm < 296mm） */

.ms-toolbar {
  position: sticky;
  top: 0;
  z-index: 9;
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 8px 12px;
  background: rgba(255, 255, 255, 0.96);
  border-bottom: 1px solid #dcd3c2;
  font-size: 12.5px;
  color: #6b5c49;
}
.ms-tb-line {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: center;
  gap: 8px;
}
.ms-tb-title {
  font-size: 13px;
  font-weight: 600;
  color: #3e342a;
}
.ms-hint {
  color: #8a7a66;
}
.ms-hint em {
  font-style: normal;
  font-weight: 700;
  color: #b4673a;
}
.ms-seg {
  display: flex;
  align-items: center;
  gap: 2px;
  background: #f4efe6;
  border-radius: 999px;
  padding: 3px 6px;
}
.ms-seg-label {
  margin: 0 7px;
  font-weight: 600;
  white-space: nowrap;
}
.ms-seg label {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 4px 8px;
  border-radius: 999px;
  cursor: pointer;
  white-space: nowrap;
}
.ms-seg label:has(input:checked) {
  background: #d9793a;
  color: #fff;
}
.ms-seg input {
  margin: 0;
  accent-color: #d9793a;
}
.ms-btn {
  border: 0;
  border-radius: 999px;
  background: #d9793a;
  color: #fff;
  font-size: 12.5px;
  padding: 8px 18px;
  cursor: pointer;
  font-family: inherit;
  letter-spacing: 0.03em;
  white-space: nowrap;
}
.ms-btn:hover {
  background: #c4692c;
}
.ms-btn-ghost {
  background: #fff;
  color: #a05a2c;
  border: 1px solid #d9793a;
  padding: 7px 15px;
}
.ms-btn-ghost:hover {
  background: #fbf0e7;
}
.ms-btn-ghost[aria-pressed="true"] {
  background: #f1e7da;
  color: #6b5c49;
  border-color: #cbb79f;
}
.ms-btn-alt {
  background: #4f7a66;
}
.ms-btn-alt:hover {
  background: #416856;
}

.ms-stack {
  padding: 20px 0 50px;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 12px;
}
.ms-cap {
  margin: 0;
  font-size: 12.5px;
  color: #8a7a66;
  letter-spacing: 0.04em;
  text-align: center;
}

/* A4 竖版 210×296mm，留 1mm 余量防分页 */
.ms-sheet {
  position: relative;
  width: 210mm;
  height: 296mm;
  background: #fff;
  padding: 12mm 12mm 10mm;
  overflow: hidden;
  box-shadow: 0 10px 30px rgba(72, 58, 40, 0.16);
  -webkit-print-color-adjust: exact;
  print-color-adjust: exact;
}
.ms-sheet::after {
  content: "";
  position: absolute;
  inset: 5mm;
  border: 1px dashed #dcd2c0;
  pointer-events: none;
}
.ms-hd {
  height: 27mm;
  display: flex;
  flex-direction: column;
  justify-content: space-between;
}
.ms-hd-top {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  border-bottom: 1.5px solid #26221d;
  padding-bottom: 2.2mm;
}
.ms-eyebrow {
  display: flex;
  align-items: center;
  gap: 5px;
  font-size: 3mm;
  font-weight: 600;
  letter-spacing: 0.2em;
  color: #b4673a;
}
.ms-eyebrow i {
  display: block;
  width: 12px;
  height: 2px;
  background: #d9793a;
  border-radius: 2px;
}
.ms-h1 {
  margin: 2mm 0 0;
  font-size: 8mm;
  line-height: 1;
  font-weight: 800;
  letter-spacing: 0.05em;
}
.ms-tag {
  font-size: 3.1mm;
  letter-spacing: 0.12em;
  color: #9b8f7d;
  padding-bottom: 1mm;
}
.ms-meta {
  display: flex;
  align-items: center;
  gap: 5mm;
  font-size: 3.5mm;
  color: #5a5248;
}
.ms-fill {
  display: inline-block;
  width: 19mm;
  height: 4mm;
  border-bottom: 1px solid #9a9186;
  margin-left: 1mm;
  vertical-align: -0.5mm;
}
.ms-fill.w-s {
  width: 12mm;
}
.ms-tip {
  margin-top: 3mm;
  height: 15mm;
  background: #f8f4ec;
  border-left: 3px solid #d9793a;
  padding: 1.8mm 3mm;
  display: flex;
  flex-direction: column;
  justify-content: center;
  gap: 1mm;
  font-size: 3.4mm;
  line-height: 1.45;
  color: #4a4137;
}
.ms-tip .lead {
  font-weight: 700;
  color: #b4673a;
  letter-spacing: 0.04em;
}
.ms-tip .ex {
  font-variant-numeric: tabular-nums;
  letter-spacing: 0.02em;
}
.ms-tip em {
  font-style: normal;
  font-weight: 700;
  color: #b4673a;
}

/* 30 题：3 列 × 10 行，每格 62×20mm */
.ms-grid {
  margin-top: 4mm;
  height: 200mm;
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  grid-template-rows: repeat(10, 20mm);
  border-top: 1px solid #e2dccd;
  border-left: 1px solid #e2dccd;
}
.ms-q {
  position: relative;
  display: flex;
  align-items: center;
  justify-content: center;
  border-right: 1px solid #e2dccd;
  border-bottom: 1px solid #e2dccd;
}
.ms-no {
  position: absolute;
  left: 2mm;
  top: 1.1mm;
  font-size: 9px;
  color: #b8ae9d;
  font-family: Arial, sans-serif;
}
.ms-expr {
  font-family: Arial, "Microsoft YaHei", sans-serif;
  font-size: 5.6mm;
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
  letter-spacing: 0.01em;
}
.ms-op {
  margin: 0 1.1mm;
}
.ms-blank {
  display: inline-block;
  width: 13mm;
  height: 5mm;
  border-bottom: 1.6px solid #26221d;
  vertical-align: -0.9mm;
  margin: 0 0.4mm;
}

/* 可裁剪答案条：10 列 × 3 行 */
.ms-ansbar {
  margin-top: 4mm;
  height: 18mm;
  border: 1px dashed #b9ae9a;
  padding: 1.2mm 2.5mm;
  display: flex;
  flex-direction: column;
  justify-content: space-between;
}
.ms-ans-h {
  font-size: 3mm;
  font-weight: 700;
  letter-spacing: 0.1em;
  color: #8a7a66;
}
.ms-ans-h em {
  font-style: normal;
  font-weight: 400;
  margin-left: 2mm;
  letter-spacing: 0;
  color: #a79a87;
}
.ms-ans-grid {
  display: grid;
  grid-template-columns: repeat(10, 1fr);
  row-gap: 0.2mm;
  font-family: Arial, "Microsoft YaHei", sans-serif;
  font-size: 3.3mm;
  line-height: 3.8mm;
  font-variant-numeric: tabular-nums;
  color: #6b6155;
}
.ms-ans-grid b {
  color: #26221d;
  font-weight: 700;
  margin-left: 0.3mm;
}

/* 答案开关：屏幕与打印同步；visibility 保占位 */
.ms-hide-ans .ms-ansbar {
  visibility: hidden;
}

@media print {
  .ms-stack {
    padding: 0;
    gap: 0;
    display: block;
  }
  .ms-sheet {
    box-shadow: none;
    margin: 0;
  }
}
```

- [x] **Step 3: `src/pages/math/MathSheet.tsx` 完整实现**

```tsx
import { useEffect, useState } from "react";
import { Toast, useNotice } from "../../components/Toast";
import { api, errorMessage } from "../../lib/ipc";
import type { MathSheet } from "../../types";
import { buildExample, type MathOp } from "./example";
import "./math-sheet.css";

/** 选项与脚本 URL 参数一一对应；桌面版存 localStorage（seed 不存，每次进来换新卷） */
interface MathOptions {
  pos: string; // "1" | "2" | "3" | "4" | "random"
  op1: string; // "m" | "d" | "a" | "s" | "random"
  op2: string; // "a" | "s" | "random"
  showAns: boolean;
}

const DEFAULT_OPTIONS: MathOptions = { pos: "2", op1: "m", op2: "random", showAns: false };
const STORAGE_KEY = "mathSheet.options";

function loadOptions(): MathOptions {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) return { ...DEFAULT_OPTIONS, ...(JSON.parse(raw) as Partial<MathOptions>) };
  } catch {
    // 坏数据一律回默认
  }
  return DEFAULT_OPTIONS;
}

const POS_OPTIONS = [
  { value: "1", label: "① 第 1 个" },
  { value: "2", label: "② 第 2 个" },
  { value: "3", label: "③ 第 3 个" },
  { value: "4", label: "④ 第 4 个" },
  { value: "random", label: "随机（每题不同）" },
];
const OP1_OPTIONS = [
  { value: "m", label: "× 乘" },
  { value: "d", label: "÷ 除" },
  { value: "a", label: "+ 加" },
  { value: "s", label: "− 减" },
  { value: "random", label: "随机" },
];
const OP2_OPTIONS = [
  { value: "a", label: "+ 加" },
  { value: "s", label: "− 减" },
  { value: "random", label: "随机" },
];

const OP_SYM: Record<string, string> = { m: "×", d: "÷", a: "+", s: "−" };
const OP1_NAME: Record<string, string> = { m: "乘", d: "除", a: "加", s: "减" };

const posLabel = (p: string) => (p === "random" ? "每题随机留空" : `填第 ${p} 个数`);
const op1Label = (s: string) => (s === "random" ? "随机" : OP1_NAME[s]);
const op2Label = (s: string) => (s === "random" ? "加减" : s === "a" ? "加" : "减");

export default function MathSheetPage() {
  const [options, setOptions] = useState<MathOptions>(loadOptions);
  const [seed, setSeed] = useState(() => (Date.now() ^ Math.floor(Math.random() * 2147483647)) >>> 0);
  const [sheet, setSheet] = useState<MathSheet | null>(null);
  const [loading, setLoading] = useState(true);
  /** 第一步是除/减时第二步自动切加的提示 */
  const [autoNote, setAutoNote] = useState(false);
  const { notice, notify } = useNotice();

  // 第一步为 ÷/− 时中间结果 ≤9，减不动 ≥10 的第三数：第二步强制用加（与脚本 normalize 一致）
  const effective = (): { pos: string; op1: string; op2: string } => {
    if ((options.op1 === "d" || options.op1 === "s") && options.op2 === "s") {
      return { pos: options.pos, op1: options.op1, op2: "a" };
    }
    return { pos: options.pos, op1: options.op1, op2: options.op2 };
  };

  const eff = effective();
  useEffect(() => {
    setAutoNote((options.op1 === "d" || options.op1 === "s") && options.op2 === "s");
  }, [options.op1, options.op2]);

  useEffect(() => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(options));
  }, [options]);

  useEffect(() => {
    let alive = true;
    setLoading(true);
    (async () => {
      try {
        const s = await api.generateMathSheet(seed, eff.pos, eff.op1, eff.op2);
        if (alive) setSheet(s);
      } catch (e) {
        if (alive) {
          setSheet(null);
          notify("error", errorMessage(e));
        }
      } finally {
        if (alive) setLoading(false);
      }
    })();
    return () => {
      alive = false;
    };
    // eff 由 options 派生，这里故意只用三个原始值
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [seed, options.pos, options.op1, options.op2]);

  const regen = () => setSeed((Date.now() ^ Math.floor(Math.random() * 2147483647)) >>> 0);

  const first = sheet?.questions[0];
  const example =
    first && options.pos !== "random"
      ? buildExample(Number(options.pos), first.op1 as MathOp, first.op2 as MathOp)
      : first
        ? buildExample(3, first.op1 as MathOp, first.op2 as MathOp)
        : null;

  return (
    <div className="print-root min-h-0 flex-1 overflow-y-auto bg-stone-200/60">
      {/* 本页满幅打印；@page 无法按 class 隔离，由各打印页自声明（默写单是 12mm） */}
      <style>{"@page { size: A4 portrait; margin: 0 }"}</style>

      <div className="no-print">
        <div className="ms-toolbar">
          <div className="ms-tb-line">
            <span className="ms-tb-title">二年级口算 · A4 竖版 1 页 30 题</span>
            <span className="ms-hint">
              从左到右两步算：① 数 <em>×/÷/+ /−</em> ② 数 <em>±</em> ③ 数 = ④ 得数
            </span>
            <span className="ms-hint">打印：边距「无」、缩放 100%</span>
          </div>
          <div className="ms-tb-line">
            {[
              { id: "pos", label: "让第几个数留空", items: POS_OPTIONS },
              { id: "op1", label: "第一步", items: OP1_OPTIONS },
              { id: "op2", label: "第二步", items: OP2_OPTIONS },
            ].map((g) => (
              <div className="ms-seg" key={g.id}>
                <span className="ms-seg-label">{g.label}</span>
                {g.items.map((it) => (
                  <label key={it.value}>
                    <input
                      type="radio"
                      name={g.id}
                      value={it.value}
                      checked={options[g.id as "pos" | "op1" | "op2"] === it.value}
                      onChange={() => setOptions((o) => ({ ...o, [g.id]: it.value }))}
                    />
                    {it.label}
                  </label>
                ))}
              </div>
            ))}
          </div>
          <div className="ms-tb-line">
            <button
              className="ms-btn ms-btn-ghost"
              type="button"
              aria-pressed={!options.showAns}
              onClick={() => setOptions((o) => ({ ...o, showAns: !o.showAns }))}
            >
              {options.showAns ? "隐藏答案 · 给孩子" : "显示答案 · 家长版"}
            </button>
            <button className="ms-btn ms-btn-alt" type="button" onClick={regen}>
              重新生成算式
            </button>
            <button className="ms-btn" type="button" onClick={() => window.print()}>
              打印 / 存为 PDF
            </button>
          </div>
        </div>
      </div>

      <div className="ms-stack">
        <p className="ms-cap no-print">
          二年级口算 · {posLabel(options.pos)} ·{" "}
          {eff.op1 === "random" ? "混合" : OP_SYM[eff.op1]} → {eff.op2 === "random" ? "±" : OP_SYM[eff.op2]} · 题目 #
          {seed} ·{" "}
          {options.showAns ? "答案条可沿虚线剪下" : "已隐藏答案，打印出来直接给孩子做"}
          {autoNote ? " · 第一步是除或减时第二步只能用加，已自动切换" : ""}
        </p>

        {loading ? (
          <div className="flex w-full items-center justify-center py-20 text-sm text-stone-400">
            正在出题…
          </div>
        ) : !sheet ? (
          <div className="flex w-[210mm] max-w-full flex-col items-center gap-2 rounded bg-white px-6 py-20 shadow-md">
            <p className="text-sm text-stone-500">这组运算符暂时出不出 30 道题</p>
            <p className="text-xs text-stone-400">请换一个第二步运算符，或把第一步改成「随机」</p>
          </div>
        ) : (
          <div className={"ms-sheet" + (options.showAns ? "" : " ms-hide-ans")}>
            <div className="ms-hd">
              <div className="ms-hd-top">
                <div>
                  <div className="ms-eyebrow">
                    <i /> 每日口算
                  </div>
                  <h1 className="ms-h1">想一想，横线上填几？</h1>
                </div>
                <div className="ms-tag">
                  {posLabel(eff.pos)} · 先{op1Label(eff.op1)}，再{op2Label(eff.op2)}
                </div>
              </div>
              <div className="ms-meta">
                <span>
                  姓名<span className="ms-fill" />
                </span>
                <span>
                  日期<span className="ms-fill" />
                </span>
                <span>
                  用时<span className="ms-fill w-s" />
                  分<span className="ms-fill w-s" />
                  秒
                </span>
                <span>
                  做对<span className="ms-fill w-s" />
                  / 30 题
                </span>
              </div>
            </div>

            <div className="ms-tip">
              <div>
                <span className="lead">
                  {options.pos === "random"
                    ? "先看清空在哪一位、两步各是什么运算："
                    : eff.op2 === "random"
                      ? "先看第二步是 + 还是 −，再倒着把缺的数找出来："
                      : "先算能算的那一步，再倒着把缺的数找出来："}
                </span>
              </div>
              {example && (
                <div className="ex">
                  {options.pos === "random" ? "举例：" : "例："}
                  {example.expr} → {example.step}
                  {example.answer !== "" && (
                    <>
                      ，填 <em>{example.answer}</em>
                    </>
                  )}
                  {options.pos === "random" ? "。" : "。"}
                </div>
              )}
            </div>

            <div className="ms-grid">
              {sheet.questions.map((q, i) => {
                const pos = sheet.blanks[i];
                const nums = [q.a, q.b, q.c, q.d];
                const cells = nums.map((v, k) =>
                  k + 1 === pos ? (
                    <span className="ms-blank" key={k} />
                  ) : (
                    <span key={k}>{v}</span>
                  ),
                );
                return (
                  <div className="ms-q" key={i}>
                    <span className="ms-no">{String(i + 1).padStart(2, "0")}</span>
                    <span className="ms-expr">
                      {cells[0]}
                      <span className="ms-op">{OP_SYM[q.op1]}</span>
                      {cells[1]}
                      <span className="ms-op">{OP_SYM[q.op2]}</span>
                      {cells[2]}
                      <span className="ms-op">=</span>
                      {cells[3]}
                    </span>
                  </div>
                );
              })}
            </div>

            <div className="ms-ansbar">
              <div className="ms-ans-h">
                参考答案<em>沿虚线剪下，做完再核对</em>
              </div>
              <div className="ms-ans-grid">
                {sheet.questions.map((q, i) => {
                  const nums = [q.a, q.b, q.c, q.d];
                  return (
                    <span key={i}>
                      {i + 1}.&nbsp;<b>{nums[sheet.blanks[i] - 1]}</b>
                    </span>
                  );
                })}
              </div>
            </div>
          </div>
        )}
      </div>

      <Toast notice={notice} />
    </div>
  );
}
```

- [x] **Step 4: 验证**：`npm run typecheck`；`npm run build`。

- [x] **Step 5: 提交**

```bash
git add src/pages/math/
git commit -m "feat: 口算练习页面（选项工具条 + A4 卷面 + 答案条 + 打印）"
```

---

### Task 6: 文档登记（wiki + AGENTS.md）

**Files:**
- Modify: `wiki/architecture.md`（domain 模块列表加 math.rs）
- Modify: `wiki/frontend.md`（工具集合壳层 + pages/math）
- Modify: `AGENTS.md`（「这是什么」与目录段反映工具集合）

- [x] **Step 1** 先读三个文件相关段落，再最小增量修改：
  - architecture.md：domain 一节 `sampler.rs` 旁加一行 `math.rs 口算出题引擎（三数两步逆推，seed 可复现，纯函数带单测）`。
  - frontend.md：加小节「工具集合壳层」：顶栏一级工具（`src/tools/registry.ts` 注册）+ 侧栏二级菜单；打印 `@page` 由各打印页自声明（PrintSheet 12mm / MathSheet 0）；`src/pages/math/` 说明。
  - AGENTS.md：「这是什么」改为「给孩子用的**学习小工具集合**（单机离线）：当前含生词默写与口算练习两个工具」，目录树 `src/pages/` 行补 `math/`，加 `src/tools/registry.ts` 一行。

- [x] **Step 2: 提交**

```bash
git add wiki/architecture.md wiki/frontend.md AGENTS.md
git commit -m "docs: 登记工具集合壳层与口算练习模块"
```

---

### Task 7: 全量验证 + 干净子代理审查（AGENTS.md 流程）

- [x] **Step 1: 全量验证**

```bash
cd src-tauri && cargo test          # 136 + 新增 11 个左右全部通过
cd src-tauri && cargo clippy --all-targets
npm run typecheck
npm run build
```

- [x] **Step 2: 起全新子代理审查**（general-purpose，只读不改）：给它 `git status --short` / `git diff main...HEAD --stat`、变更文件清单（要求读全文）、AGENTS.md 硬约束、已跑通的验证结果；要求输出 `文件:行号 + 触发路径 + P1/P2 + 最小修法`。
- [x] **Step 3: 主会话逐条判定**：真问题修 + 补单测；误报记理由。
- [x] **Step 4: 修复后重跑 Step 1，再起新子代理复审，直到无 P1 且无新引入 P2。**
- [x] **Step 5: 记录 `.workbuddy/memory/2026-11-06.md`**：各轮发现、判定、修复、误报理由。

---

## Self-Review 记录

- 规格覆盖：设计文档 §2 壳层（Task 1）、§3 @page（Task 2，改为更安全的「各页自声明」方案并在 Task 2 说明理由）、§4 出题引擎（Task 3/4）、§5 前端页（Task 5）、§8 顺序（分支内按 Task 顺序提交）、文档（Task 6）、审查（Task 7）。规格的「全局 margin:0」被 Task 2 的等价更安全方案替代——已在本计划标注。
- 占位符扫描：无 TBD/TODO；所有代码步骤均给出完整代码或精确编辑。
- 类型一致性：`Route/TOOLS/NAV_BY_TOOL`（Task 1）与 App.tsx 引用一致；`generate(seed,pos,op1,op2)` 签名（Task 3）与命令（Task 4）及 `generateMathSheet`（Task 4/5）一致；`MathSheet{questions,blanks}` 字段名前后端一致（Op 序列化为 "m/d/a/s"）。
