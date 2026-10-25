# 生词默写提问小工具 · 需求规格说明（V1）

> 定位：家长在家给小学二年级孩子做生词默写的**单机离线**桌面小工具。家长读词、孩子手写、家长批改，工具负责抽题、防偷看、记录与错题沉淀。

- 技术约束：Rust + Tauri 2.x + SQLite（rusqlite bundled）
- 前端：React 18 + TypeScript + Vite + Tailwind CSS
- 使用范围：单孩子、多词表分组（无账号、无网络、无云端）

---

## 1. 目标与非目标

### 1.1 目标
| # | 目标 | 衡量方式 |
|---|---|---|
| G1 | 录入一本课本的词表（约 8~20 组、每组 10~30 词）不超过 10 分钟 | 批量粘贴为主路径 |
| G2 | 一次默写 20 词，家长全程只需「点显示 → 读 → 点下一个」 | 3 次点击内完成一个词 |
| G3 | 每个词在一次默写中严格只出现一次，顺序可追溯 | 数据库唯一约束保证 |
| G4 | 错词自动进入错题本，学会后自动出本，无需人工维护 | 连续答对 2 次出本 |
| G5 | 中途退出不丢进度，可继续未完成的默写 | 会话可恢复 |

### 1.2 非目标（V1 不做）
- 不做语音识别、不做孩子自己在电脑上打字答题
- 不做多设备同步、账号、云备份
- 不做排名、积分、动画等游戏化激励

---

## 2. 角色与核心场景

**唯一角色：家长**（操作者）。孩子只在纸上书写，不看屏幕。

主流程（Happy Path）：
1. 首次使用：新建分组「第 1 单元」→ 粘贴词表 → 保存
2. 默写前：勾选「第 1 单元」「第 2 单元」→ 设置题量 20 → 开始默写
3. 默写中：屏幕默认遮住词语 → 点「显示」→ 家长读 → 孩子写 → 点「下一个」
4. 孩子不会：点「不会」，直接跳到下一词
5. 结束后：批改页全览本次 20 个词 → 点出错词 → 完成
6. 下次：可直接从「错题本」专项默写

---

## 3. 功能需求

### F1 词语表管理

| 编号 | 需求 | 优先级 |
|---|---|---|
| F1.1 | 分组的增删改：编号（如「1」「第3单元」）、名称、排序、归档 | P0 |
| F1.2 | 分组内词条的增删改（词语文本、可选备注） | P0 |
| F1.3 | **批量粘贴导入**：把整段文字粘进来，自动切分成词条 | P0 |
| F1.4 | 分组列表显示每组词数、未掌握数、最近练习时间 | P1 |
| F1.5 | 删除分组需二次确认，提示将同时删除 N 个词条（若涉及错题本中的词，需额外警告） | P0 |
| F1.6 | 同组内词语不可重复 | P0 |
| F1.7 | 全量导出/导入 JSON 备份 | P1 |

**F1.3 切分规则**（确定性逻辑，需单测覆盖）：
- 分隔符：换行、中文逗号「，」、英文逗号「,」、顿号「、」、制表符、空格（连续空白视作一个）
- 全角/半角归一化；去除首尾空白与空项
- 自动去重，并在预览中提示「已自动去重 N 个」
- 导入前展示**预览列表**：解析出 M 个词，可逐个删改后确认入库

### F2 抽题配置

| 编号 | 需求 | 优先级 |
|---|---|---|
| F2.1 | 多选分组（复选框，支持全选/反选） | P0 |
| F2.2 | 抽题范围：全部词 / 仅未掌握（新词+错题）/ 错题本 | P0 |
| F2.3 | 题量：10 / 20 / 30 / 全部；候选不足时按实际数量并提示 | P0 |
| F2.4 | 实时预览「本次可抽 X 词」；为 0 时禁用开始按钮并说明原因 | P0 |
| F2.5 | 顺序策略：随机（默认）/ 按词表顺序 | P1 |

**抽题算法**（纯函数，可单测）：
1. 取候选集：选中分组内所有词 → 按 F2.2 过滤 `status`
2. **跨组去重**：同一词语文本只保留一条（保留所属分组 `sort_order` 最小者），避免一场默写重复写同一个词
3. Fisher–Yates 洗牌（随机源来自 Rust `rand`）
4. 截取前 N 条
5. 按顺序写入 `session_items.seq`（1-based）；唯一索引保证 `(session_id, word_id)` 与 `(session_id, seq)` 双重唯一

### F3 默写进行中

| 编号 | 需求 | 优先级 |
|---|---|---|
| F3.1 | **防偷看遮罩**：默认隐藏词语，只显示「第 3 / 20 题」大字 + 「点击显示」 | P0 |
| F3.2 | 点击（或按空格）显示词语，默认 3 秒后自动重新遮罩；时长可配 2~10 秒 / 不自动 | P0 |
| F3.3 | 「下一个」推进；「不会」标记 `skip_marked=1` 并推进 | P0 |
| F3.4 | 「上一个」可回看（只读，不重复计数） | P1 |
| F3.5 | 顶部进度条 + 当前题号；底部「结束并批改」 | P0 |
| F3.6 | 词语字号大（默认 72~96px，可调），方便家长侧身读数 | P1 |
| F3.7 | 键盘快捷键：空格 = 显示/下一个，Enter = 不会，← = 上一个，Esc = 退出 | P1 |
| F3.8 | 中途关闭 App：会话保持 `running`，下次启动提示「继续上次默写 / 放弃」 | P0 |
| F3.9 | 同一场会话内每个词不重复出现（数据库约束兜底） | P0 |

### F4 批改与记录

| 编号 | 需求 | 优先级 |
|---|---|---|
| F4.1 | 结束后进入批改页：网格展示本次全部词 + 顺序号，默认全部正确 | P0 |
| F4.2 | 点击词条切换「错」标记（红）；`skip_marked` 的词预置为错 | P0 |
| F4.3 | 「全对，完成」一键确认；「保存进度」可稍后继续批改（`reviewing` 状态） | P0 |
| F4.4 | 提交时按 §4 状态机更新每个词的状态 | P0 |
| F4.5 | 历史记录：时间、模式、题量、错题数；可看详情并可「只练本次错词」 | P1 |
| F4.6 | 会话详情保留抽取顺序，可复盘「第几个词卡住了」 | P1 |

### F5 错题本

| 编号 | 需求 | 优先级 |
|---|---|---|
| F5.1 | 错题本 = 所有 `status='learning'` 的词，展示词语、所属分组、错误次数、最近出错时间 | P0 |
| F5.2 | 排序：错误次数降序（默认）/ 最近出错 / 分组 | P1 |
| F5.3 | 「错题专项默写」：跳转 F2，范围锁定错题本，可再按分组筛选 | P0 |
| F5.4 | 出本规则：**连续答对 2 次**自动出本（全局可配，默认 2） | P0 |
| F5.5 | 手动「已掌握」移出；误操作可「重新加入错题本」 | P1 |
| F5.6 | 显示「已连续答对 1/2」进度提示 | P1 |
| F5.7 | 分组被删除时其词条一并删除并从错题本移除（需 F1.5 警告） | P0 |

### F6 设置

| 编号 | 需求 | 优先级 |
|---|---|---|
| F6.1 | 遮罩自动隐藏时长（2~10 秒 / 不自动） | P1 |
| F6.2 | 出本所需连续答对数（1~5） | P1 |
| F6.3 | 词语字号档位（大 / 更大 / 超大） | P1 |
| F6.4 | 数据目录展示、一键打开、备份导出 | P1 |

---

## 4. 词条状态机

~~~
                 批改=错
    new ───────────────────────► learning ──┐
     │                             ▲        │ 连续答对 N 次 (N=2)
     │  批改=对                     │ 批改=错 │
     └────────► mastered ◄─────────┴────────┘
                  ▲
                  └── 手动移出错题本 / 错题本内连续答对 N 次
~~~

| 事件 | 字段变更 |
|---|---|
| 批改为「错」 | `status='learning'`，`wrong_count+1`，`correct_streak=0` |
| 批改为「对」 | `correct_streak+1`；若 `status='new'` → `mastered`；若 `status='learning'` 且 `correct_streak>=N` → `mastered` 且 `correct_streak=0` |
| 会话中「不会」 | 等价于批改「错」 |
| 手动移出 | `status='mastered'`，`correct_streak=0` |
| 任何练习 | 更新 `last_practiced_at` |

说明：`mastered` 不等于永不出现——用户选「范围 = 全部词」时仍会被抽到（用于期末总复习）。

---

## 5. 数据模型（SQLite）

~~~sql
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE groups (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  code         TEXT NOT NULL,                 -- 编号，如 "1" / "第3单元"
  name         TEXT NOT NULL,
  sort_order   INTEGER NOT NULL DEFAULT 0,
  is_archived  INTEGER NOT NULL DEFAULT 0,
  created_at   TEXT NOT NULL                  -- ISO8601 UTC
);

CREATE TABLE words (
  id                INTEGER PRIMARY KEY AUTOINCREMENT,
  group_id          INTEGER NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
  text              TEXT NOT NULL,            -- 归一化后的词语（NFKC + 去空白）
  note              TEXT,
  status            TEXT NOT NULL DEFAULT 'new'
                    CHECK (status IN ('new','learning','mastered')),
  wrong_count       INTEGER NOT NULL DEFAULT 0,
  correct_streak    INTEGER NOT NULL DEFAULT 0,
  last_practiced_at TEXT,
  created_at        TEXT NOT NULL
);
CREATE UNIQUE INDEX ux_words_group_text ON words(group_id, text);
CREATE INDEX ix_words_status ON words(status);

CREATE TABLE sessions (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  mode         TEXT NOT NULL CHECK (mode IN ('group','mistake')),
  status       TEXT NOT NULL CHECK (status IN ('running','reviewing','finished','abandoned')),
  config_json  TEXT NOT NULL,                 -- 抽题参数快照（分组id、范围、题量、策略）
  total        INTEGER NOT NULL DEFAULT 0,
  wrong_total  INTEGER NOT NULL DEFAULT 0,
  started_at   TEXT NOT NULL,
  finished_at  TEXT
);
CREATE INDEX ix_sessions_status ON sessions(status);

CREATE TABLE session_items (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id   INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  word_id      INTEGER NOT NULL REFERENCES words(id) ON DELETE CASCADE,
  seq          INTEGER NOT NULL,              -- 1-based 抽取顺序
  skip_marked  INTEGER NOT NULL DEFAULT 0,    -- 默写中点了「不会」
  result       TEXT NOT NULL DEFAULT 'pending'
               CHECK (result IN ('pending','correct','wrong')),
  created_at   TEXT NOT NULL
);
CREATE UNIQUE INDEX ux_items_seq  ON session_items(session_id, seq);
CREATE UNIQUE INDEX ux_items_word ON session_items(session_id, word_id);

CREATE TABLE settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
~~~

**设计取舍**
- 不设 `books` 表：V1 用分组的编号/名称承载「单元/课」维度，需要分册时再引入（见 §10）
- 错题本不单独建表：由 `words.status='learning'` 派生，避免「错题表与词表不同步」的经典 bug；历史错误次数由 `wrong_count` 与 `session_items` 共同支撑
- 会话快照 `config_json`：即使用户事后改了分组或删了词，历史记录仍可解释

---

## 6. 界面清单

| 页面 | 关键元素 |
|---|---|
| **P1 词语表** | 左侧分组列表（编号/名称/词数/未掌握数），右侧词条表格；「+ 分组」「粘贴导入」 |
| **P1b 批量导入弹窗** | 大文本框 → 实时解析预览（词条 chips）→ 去重提示 → 确认导入 |
| **P2 抽题配置** | 分组多选卡片、范围单选、题量选择、「本次可抽 X 词」+「开始默写」 |
| **P3 默写** | 全屏无干扰：进度条 + 题号 + 遮罩区 +「显示 / 不会 / 下一个」 |
| **P4 批改** | 词条网格（序号 + 词语），点击标红；底部「错 N 个 · 完成」 |
| **P5 错题本** | 表格（词语/分组/错次/连对进度/最近出错），「专项默写」「已掌握」 |
| **P6 历史** | 会话列表 → 详情（顺序 + 对错）→「只练本次错词」 |
| **P7 设置** | 遮罩时长、出本连对次数、字号、数据目录与备份 |

导航：4 个主入口 —— 词语表 / 开始默写 / 错题本 / 历史（设置入口在角落）。默认首页 =「开始默写」。

---

## 7. 非功能需求

| 类别 | 要求 |
|---|---|
| 离线 | 完全离线，无网络请求、无遥测、无自动更新（V1） |
| 数据位置 | Tauri `app_data_dir` 下 `dictation.db`（Windows: %APPDATA%\com.dictation.app\） |
| 数据安全 | 启动时若存在 `running` / `reviewing` 会话则提示恢复；备份为**手动**导出 JSON（V1 不做自动备份） |
| 性能 | 5000 词内：启动 < 1s，抽题 < 50ms，页面切换无卡顿 |
| 可靠性 | 写操作走事务；迁移用 `PRAGMA user_version` 递增，升级不丢数据 |
| 界面 | 中文字体优先（微软雅黑回退）；最小窗口 900×640，默认 1200×800；词语区字号 ≥ 72px |
| 可用性 | 默写页可纯键盘操作；破坏性操作二次确认 |
| 可测试 | 抽题、切分、状态机为纯函数，`cargo test` 覆盖 |

---

## 8. 技术方案

~~~
src-tauri/
  src/
    lib.rs                  # tauri::Builder，注册 command
    db/                     # 连接(Mutex<Connection>) + migrations
    domain/                 # 纯逻辑：抽题 sampler、文本切分 parser、状态机 transitions
    speech/                 # 【预留】TTS 抽象：trait Speaker + NullSpeaker，V1 不实现
    repo/                   # groups / words / sessions 存取
    commands/               # #[tauri::command] 薄层，DTO 为 serde struct
    error.rs                # 统一 AppError -> 前端 message
  migrations/0001_init.sql
src/                        # React + TS
  pages/{Wordbook,Pick,Quiz,Review,Mistakes,History,Settings}
  lib/ipc.ts                # invoke 封装 + 与 Rust DTO 对齐的类型
  store/                    # zustand（当前会话状态）
~~~

- SQLite：`rusqlite` 开启 `bundled` feature（免用户安装 SQLite）；单连接 + `Mutex`，WAL 足够单机场景
- IPC：按用例划分命令（`create_session`、`next_item`、`mark_item`、`submit_review`），不在前端拼 SQL
- 状态机与抽题算法放 `domain/`，不依赖 Tauri，便于单测
- 打包：Windows NSIS + MSI，体积 < 15MB

### 8.1 预留扩展点（V1 只留接口，不实现）

| 预留项 | 预留方式 | 落地时的改动 |
|---|---|---|
| TTS 朗读 | `speech::Speaker` trait（`fn speak(&self, text: &str) -> Result<()>`）+ `NullSpeaker` 空实现，随 AppState 注入；默写页预留「朗读」按钮位（禁用态） | 新增 `SapiSpeaker` 实现 + 前端启用按钮；**不改表结构** |
| 多孩子 | 进度字段暂放 `words`，但所有读写统一走 `repo::word_progress` 接口，禁止命令层直接碰状态字段 | 拆出 `word_progress(child_id, word_id, status, wrong_count, correct_streak, last_practiced_at)`，repo 换实现 |
| 多课本/学期 | 分组已带 `code`（编号）与 `sort_order`，UI 完全按此排序 | 新增 `books` 表与 `groups.book_id`，顶部加学期切换器 |
| 打印复习单 | 错题本查询已是独立 command `list_mistakes`，自带筛选参数 | 只加前端打印视图，后端不动 |

> 关键约束：`words` 表只放「词条本身」的字段，任何学习进度都以 `repo::word_progress` 收口；这样后续拆表是纯搬迁，不改调用方。

---

## 9. 里程碑

| 阶段 | 交付 | 验收 |
|---|---|---|
| M1 骨架 | Tauri 工程、SQLite 迁移、分组/词条 CRUD、批量粘贴 | 能录入一本词表并持久化 |
| M2 默写 | 抽题配置 + 默写页（遮罩/推进/顺序落库/中断恢复） | 20 词默写不重复、重启可续 |
| M3 闭环 | 批改页 + 状态机 + 错题本 + 历史 | 错词入本、连对 2 次出本 |
| M4 打磨 | 设置项、备份导出、快捷键、安装包 | 在干净 Windows 上可直接安装使用 |

---

## 10. 后续版本（V2 候选）

1. **TTS 朗读**（已确认排期）：系统 SAPI 或 edge-tts 读词，家长可只听不看屏，届时遮罩可取消；接口已按 §8.1 预留
2. **多课本/多学期**：新增 `books` 表、`groups.book_id`，顶部切换学期
3. **多孩子**：拆出 `word_progress` 表
4. **打印复习单**：按错题本生成 A4 田字格
5. **数据看板**：每周错词趋势、分组掌握率

---

## 11. 已确认的范围裁剪（2026-10-25）

| 项 | 结论 |
|---|---|
| 拼音字段 | **不做**：`words` 表不含 `pinyin`，粘贴解析不处理拼音，界面无拼音显示 |
| TTS 朗读 | **V2 再做**：V1 只在架构上预留（§8.1），不引入任何语音依赖，不写 TTS 代码 |
| 备份 | **仅手动**：设置页提供「导出备份 / 导入备份」，不做自动滚动备份、不扫描备份目录 |

### 11.1 仍待确认
- 导入备份时若遇到同编号分组，冲突策略选「覆盖 / 合并 / 新建副本」——M4 前定即可
