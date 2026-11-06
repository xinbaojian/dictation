# 设计：应用改造为小工具集合 + 新增「口算练习」工具

- 日期：2026-11-06
- 状态：待用户评审
- 范围：仅设计，不含实施

## 1. 背景与目标

当前应用是单一功能的生词默写桌面应用（Tauri 2 + React 18 + SQLite）。目标：

1. 把应用改造成「小工具集合」：一级菜单切换工具，二级菜单是工具内功能；
2. 生词默写整体降级为其中一个工具（功能不变）；
3. 新增第二个工具「口算练习」，移植自 `~/Downloads/生成口算练习.py`（二年级三数两步逆推口算 A4 打印卷）。

### 已拍板的决策（2026-11-06 与用户确认）

| 决策点 | 结论 |
|---|---|
| 界面形态 | 顶部一级工具栏 + 左侧二级功能菜单 |
| 口算出题逻辑位置 | Rust `domain` 纯函数 + 命令封装（符合仓库约定，cargo 单测兜底） |
| 应用改名 | 只改窗口标题（建议「学习小工具」，实施时可定）；**不改 productName / identifier** |
| 口算 V1 范围 | 纯出题 + 打印，不做历史记录、不建表 |

### 非目标（V1 明确不做）

- 口算做题记录 / 统计（不建任何新表，同步零影响）
- productName、identifier 改名（identifier 绝不改，硬约束 8；productName 改名牵动升级产物命名，如做需单独立项）
- `src/pages/` 目录物理重组为 `src/tools/{dictation,math}/`（等第 3 个工具出现再做，控制本次 diff）
- 多孩子支持

## 2. 壳层改造：顶部工具栏 + 二级侧栏

### 2.1 布局

```
┌────────────────────────────────────────────────┐
│ [icon] 生词默写 | 口算练习          [设置●]      │ ← 顶栏 ~44px：一级工具切换 + 全局设置
├────────┬───────────────────────────────────────┤
│ （升级/同步横幅，全局，位置不变逻辑不变）            │
├────────┼───────────────────────────────────────┤
│ 默写    │                                       │
│ 词表    │           主内容区                     │
│ 错题    │                                       │
│ 记录    │                                       │
└────────┴───────────────────────────────────────┘
```

- 顶栏：工具 tab 按注册表渲染；右侧「设置」按钮（保留红点提示逻辑）。
- 侧栏：仅展示当前工具的二级菜单（默写工具下为现有的 默写/词表/错题/记录；口算 V1 只有一项「出题打印」）。
- 升级 / 同步横幅：从侧栏顶部方案移到顶栏下方，`quietRoute`（quiz/review/print/settings 不打横幅）逻辑原样保留。
- quiz / review 沉浸态：与现状一致，导航仍可见，activeNav 归到所属工具的默认二级项。

### 2.2 路由模型

`App.tsx` 的 `Route` 联合类型改为工具嵌套：

```ts
type ToolId = "dictation" | "math";
type Route =
  | { tool: ToolId; page: ToolPage }             // dictation: pick/wordbook/mistakes/history/quiz/review/print
                                                 // math: sheet
  | { name: "settings" };                        // 设置是全局页，挂在顶栏，不嵌套在任何工具下
```

- 现有 8 个页面的 props 接口**零改动**：跳转回调仍在 `App.tsx` 内部转换。
- 「设置」从默写侧栏底部提级为顶栏全局入口；`Settings.tsx` 内容不动（其中「默写」一节本就是工具专属，其余数据备份/同步/版本更新本就是全局）。

### 2.3 工具注册表

新增 `src/tools/registry.ts`：

```ts
interface ToolDef {
  id: ToolId;
  label: string;
  icon: string;                 // 顶栏 tab 图标
  navItems: { page: string; label: string }[]; // 二级菜单
  defaultRoute: Route;
}
export const TOOLS: ToolDef[] = [/* dictation, math */];
```

加第 3 个工具 = 注册表加一项 + 新建页面目录，壳层不再改。

## 3. 打印基建归一（唯一必须处理的冲突）

现状：全局 `@page { size: A4; margin: 12mm }`；口算卷要求满幅 `margin: 0`（卷面 210×296mm 自带 12mm 内边距，尺寸链 12+27+3+15+4+200+4+18+10=293mm 不可压）。

方案：**全局 `@page` 改为 `margin: 0`，各打印页自管内边距。**

- `PrintSheet.tsx`（默写卷）：`.print-page` 在 `@media print` 下不再把 padding 重置为 0，改为保留页内 `padding: 14mm 12mm`（把原 @page 边距转为页内 padding，视觉等价）。
- 口算卷：卷面自带 padding，满幅直接可用。
- 验证项：两种卷各打印一次，确认仍为单页、无内容截断、田字格与答案条位置不变（macOS 与 Windows WebView2 各验一遍）。

## 4. 口算练习：出题引擎（Rust domain）

### 4.1 模块与接口

新增 `src-tauri/src/domain/math.rs`（纯函数、不依赖 Tauri、可单测）：

```rust
pub enum Op { Mul, Div, Add, Sub }            // 对应脚本 m/d/a/s
pub enum OpChoice { Fixed(Op), Random }
pub enum Op2Choice { Add, Sub, Random }       // 第二步只有加减
pub enum BlankPos { One, Two, Three, Four, Random }

pub struct MathSheetRequest {
    pub seed: u32,
    pub pos: BlankPos,
    pub op1: OpChoice,
    pub op2: Op2Choice,
}

pub struct MathQuestion { pub a: i32, pub b: i32, pub c: i32, pub d: i32,
                          pub op1: Op, pub op2: Op, pub t: i32 }  // t = 第一步中间结果

pub struct MathSheet {
    pub questions: Vec<MathQuestion>,   // 恒为 30 题
    pub blanks: Vec<u8>,                // 每题留空位置 1-4（随机模式下配额打乱）
}

pub fn generate_sheet(req: &MathSheetRequest) -> Option<MathSheet>;
```

移植自脚本 JS 出题器（约 200 行核心）：mulberry32 种子随机（`Math.imul` → `wrapping_mul`，`>>> 0` → `as u32`）、`evalQ` 两步求值（表内整除、结果 1–100）、三段难度候选池（a,b∈1-9 分三段；c∈10-30/10-50）、无解符号组合过滤（除/减 配 减）、四位置唯一解暴力校验、30 题组卷（符号配额 + 去重 + 相邻答案错开 + 24 个 seed 候选按分布评分选优）、随机留空的四位置配额打乱。

**确定性约定**：同 `seed` + 同选项 ⇒ 完全同一张卷（含留空位置），因此 `blanks` 也在 Rust 生成一并返回。

### 4.2 命令与 IPC

按仓库标准链路：`src-tauri/src/commands/math.rs` 加 `#[tauri::command] generate_math_sheet`（参数转换 + 锁，无 DB 访问）→ `lib.rs` 注册 → `src/lib/ipc.ts` 加 `api.generateMathSheet` → `src/types.ts` 加类型（camelCase 自动转换）。

- 无解组合返回 `None` → 命令层映射为空卷 DTO，前端展示引导文案（与脚本一致：「请换一个第二步运算符，或把第一步改成随机」）。
- 输入修正规则（第一步为 ÷/− 时第二步自动切 +，并提示「已自动切换」）在前端选项层实现（UI 规则）；Rust 端对该组合数学上自然无解，双保险。

### 4.3 单测（cargo，预计 +15~20 用例）

- 同 seed 完全复现（questions + blanks 逐项相等）；
- 30 题、无重复、三段难度各 10 题；
- 每题四位置唯一解（测试内暴力重验，不信任实现自证）；
- 值域断言：a,b∈1-9、c∈10-50、d∈1-100、除法必整除、中间结果 ≥1；
- 无解组合（÷−、−−）返回 None；Random 模式跳过无解组合仍能出满 30 题。

## 5. 口算练习：前端页面 `src/pages/math/MathSheet.tsx`

- **工具条（`no-print`，对齐应用主题）**：留空位置 5 选、第一步 5 选、第二步 3 选、答案显隐切换、重新生成（新 seed = 时间戳扰动）、打印按钮。
- **A4 卷面（210×296mm）**：表头（标题「想一想，横线上填几？」+ 姓名/日期/用时/做对 填写栏）、示范条（**口诀与示范例生成保留在前端 TS**：`cnNum / verse / example` 约 40 行，含「二五一十」式中文口诀）、3 列 × 10 行题目格、可裁剪答案条（`visibility` 切换保占位）。
- 卷面样式沿用脚本的成熟打印设计（mm 级尺寸链、`tabular-nums`、`@page` 满幅），不重新发明。
- **状态**：选项与 seed 存 `localStorage`（记住上次配置，替代脚本的 URL 参数方案）；seed 显示在卷面脚注（题号 #），可复现同卷。
- 出题走 IPC；loading 态为毫秒级，失败按 `AppError` 中文提示统一走 `Toast`。

## 6. 错误处理汇总

| 场景 | 行为 |
|---|---|
| 无解符号组合 | Rust 返回空卷；前端显示引导文案（自动切换第二步为「加」并提示） |
| IPC 失败 | `AppError` → `Toast` 中文提示，卷面显示空态 |
| 打印 | 沿用 `window.print()` + `@media print`；工具条/横幅均挂 `no-print` |

## 7. 硬约束对照

| 约束 | 影响 |
|---|---|
| 1/2/3/5 进度只走 word_progress、错题本不建表、会话唯一性、分组树 | 不涉及，口算零 DB |
| 4 无拼音字段 | 不涉及 |
| 8 identifier 不改 | 遵守；只改 `tauri.conf.json` 的窗口 `title` |
| 9 updater 公钥/私钥 | 不碰 |
| 10 同步整库快照 | 不碰（口算无表无数据） |
| 11 单实例插件第一个注册 | 不碰 |

## 8. 实施顺序与工作量

两个独立 PR，串行，各自走完整审查流程（干净子 agent 审查 + `cargo test` / `cargo clippy --all-targets` / `npm run typecheck` + `.workbuddy/memory` 记录）：

1. **PR1 壳改造**：注册表 + 顶栏/二级侧栏 + 设置提级 + 横幅移位 + `@page` 归一 + 窗口标题改「学习小工具」；回归验证默写全流程、两种打印卷、同步横幅。约 1 天。
2. **PR2 口算练习**：`domain/math.rs` + 命令 + 单测 + ipc/类型 + `MathSheet.tsx` + 打印实测。约 1.5~2 天。

合计约 3 个工作日当量。

## 9. 已知取舍

- 口算 V1 无前端单测（仓库无前端测试设施，不为单一页面引入 vitest）；核心逻辑全部在 Rust domain 有 cargo 单测兜底，前端仅剩渲染与 ~40 行口诀示范（低风险）。
- 卷面沿用脚本原版「暖橙纸张」审美，与应用 stone/amber 主题不完全统一——打印件优先保证成熟排版，工具条才对齐应用主题。
- `src/pages/` 平铺暂不重组，等第 3 个工具出现再物理搬家。
- **seed 只显示不可输入**（审查轮 1 P2-3 判定）：「同 seed 同卷」的复现性由 Rust 引擎与单测保证；重打同一张卷走打印对话框的份数，孩子重做换一张新卷反而更好，不为此加工具条输入口。
- **示范条用第一题的实际符号**（审查轮 1 P2-4 判定，偏离原脚本的固定 combos[0]）：跟随卷面真实题型，示范更贴题。
