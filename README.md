# 二年级学习小工具（Dictation）

给二年级孩子用的**单机桌面小工具集合**，顶栏切换三个工具：「生词默写」（家长读词、孩子手写、家长批改；程序负责抽题、防偷看、记录错题）、「口算练习」（三数两步逆推口算卷，可打印也可在电脑上做，记成绩与全对最快）、「查字」（家长输字词，逐字笔顺动画 + 部首/笔画/结构/释义）。

数据只留在本机 SQLite，没有账号、没有官方云；默认唯一联网行为是自动升级——只有当用户在设置里显式配置了自有 MySQL（同步）或 OpenAI 兼容接口（AI 讲解）之后，才会多出对应的请求，而且两种情况断网都不影响使用。

默写部分的需求规格见 [REQUIREMENTS.md](./REQUIREMENTS.md)。

## 功能

### 生词默写

- **词表**：多级分组（二年级上册 → 第一单元 → …，任意层级），左侧树可**拖动调整层级与顺序**；单条录入，整段粘贴导入（自动切分、去重、剥离「1.」序号）
- **默写**：多分组 + 范围（全部 / 仅未掌握 / 错题本）+ 题量，随机抽词且一场内不重复，可开「连字抽词」让下一个词优先和上一个词有相同的字；翻到新词先显示、按设置秒数自动遮住（可随时点开重看）
- **断点**：中途退出不丢进度，下次可继续；默写结束未批改也能下次接着批
- **批改**：网格点选错词，还能细化到「哪个字不会写」；错词自动进错题本（仍按整词）
- **错题本**：连续答对 N 次自动出本（默认 2），也可手动判定已掌握、按分组筛选、专项默写；**笔画复习**逐词播放笔顺动画（可先遮住让孩子自己写再对照，笔顺数据随包发出，离线可用）
- **字帖**：错词 / 分组一键出 A4 字帖，每行一个词横向排满（示范字 → 描红 → 空格），可选田字格 / 米字格 / 方格、10-14mm 格、描红 0-2 遍；字形轮廓取自本地笔顺数据，不依赖系统装没装楷体
- **记录**：历史会话 + 抽取顺序 + 只练本次错词，可单条删除（只删记录，不动词条进度）

### 口算练习

- **出题**：三数两步逆推（① 数 *×/÷/+/−* ② 数 *±* ③ 数 = ④ 得数），左边两步、右边一步；题量 5–30，留空位置可固定或每题随机，两步运算符与四位置取值区间可调（默认区间按三段难度递进）
- **打印**：A4 满幅一张卷，附可沿虚线剪下的答案条；选项存本机，下次进来还在
- **上机做题**：一屏一题、48px 算式 + 屏下数字键盘，题号条可跳回任意题检查改答案；**做题过程中不判对错，交卷后统一公布**成绩、用时和逐题答案
- **记录**：每次的题量、做对几道、用时都留着，点开能看那次错在哪；**各题量单独记「全对最快」**，破纪录时成绩页给提示

### 查字

- 家长输入字词，逐字播放笔顺动画（可先遮住让孩子自己写再对照），并给出部首、笔画数、结构、释义
- 字典与笔顺数据随包发出，**完全离线**；AI 单字详解可选，需在设置里显式配置 OpenAI 兼容接口（不配就不联网）

### 通用

- **朗读**：笔画复习页可朗读词语，默认用系统离线语音；想要更好音质可在设置里按需下载本地语音包（一次下载永久离线，缺失时自动落回系统语音）
- **同步**：默认完全离线；在设置里配置自有 MySQL 后，可整库快照跨设备同步（乐观锁 + 启动时替换，冲突会先问再动）
- **自动升级**：新版发布在 GitHub Release，启动时静默检查、设置页下载安装并校验签名；离线时照常使用
- **设置**：遮罩时长、出本连对数、词语字号、朗读引擎、AI 与同步配置、数据目录、手动备份导出/导入

## 文档

- [REQUIREMENTS.md](./REQUIREMENTS.md)：默写部分的需求规格（目标、功能编号、数据模型、里程碑）
- [AGENTS.md](./AGENTS.md)：接手仓库的操作手册（命令、约束、常见任务）
- [wiki/](./wiki/README.md)：项目 wiki —— 架构、数据模型、默写流程、设计决策（ADR）、踩坑
- [wiki/updater.md](./wiki/updater.md)：自动升级、签名密钥保管、发版清单
- [wiki/sync.md](./wiki/sync.md)：跨设备同步的整库快照机制

## 技术栈

- Rust + Tauri 2 + SQLite（rusqlite bundled，用户无需安装 SQLite）
- React 18 + TypeScript + Vite + Tailwind CSS 4

## 开发

```bash
npm install
npm run tauri dev
```

## 校验

```bash
npm run typecheck          # 前端类型检查
npm run build              # 前端构建（typecheck + vite build）
cd src-tauri && cargo test # 后端单测：切分 / 抽题 / 出题 / 会话流转 / 同步 / 迁移
```

## 打包与部署

```bash
make                       # 打包 + 部署到 D:/StudyApp/dictation
make build                 # 只打包，产物在 src-tauri/target/release/bundle/nsis/
make test                  # cargo test + tsc --noEmit
make dev                   # 开发模式
make clean                 # 清理 target 与 dist

make install INSTALL_DIR=E:/Apps/dictation   # 换部署目录
```

部署目标默认是 `D:/StudyApp/dictation`，会覆盖其中的 `dictation.exe`；应用正在运行时会拒绝覆盖并提示先关窗口。

打包会生成自动升级所需的签名文件：推荐用 `make`（自动读取 `~/.tauri/dictation.key`）；直接跑 `npm run tauri build` 需自行设置 `TAURI_SIGNING_PRIVATE_KEY`，否则会报 `no private key` 并失败。

在 Git Bash、PowerShell、cmd 里都能跑（Makefile 会自动把 Git 自带的 bash 指给 make，需要装 Git for Windows）。

## 目录

```
src-tauri/
  migrations/            0001…0008（表结构、会话进度、错字标记、分组树、AI 卡、查字卡、口算做题）
  src/db/                连接 + WAL + user_version 递增迁移 + 启动前快照替换
  src/domain/            text 粘贴切分、sampler 抽题、math 出题（纯函数，带单测）、ai_card / char_card 校验
  src/repo/              groups / words / sessions / math_sessions / word_progress / settings / backup / sync / ai_cards / char_cards
  src/commands/          library / quiz / math / system / sync / ai / tts / print（薄封装）
  src/speech/            朗读路由（系统 TTS + 本地语音包，ADR-19/21）
  src/tts/               语音包目录、下载与解压
src/
  pages/                 Pick 抽题 / Quiz 默写 / Review 批改 / Mistakes 错题本 / StrokeReview 笔画复习 /
                         History 记录 / Settings 设置 / Wordbook 词表 / PrintSheet 字帖 / Lookup 查字；
                         math/（MathSheet 出题打印 / MathPractice 上机做题 / MathHistory 记录）
  tools/registry.ts      顶栏工具与各工具侧栏菜单
  lib/                   ipc.ts 后端调用、strokes.ts 笔顺、dict.ts 字典、pinyin.ts、print.ts、updater.ts、external.ts
  components/            Modal / ConfirmModal / Toast / StrokeWord / StrokeGlyph / GridLines / CharInfoCard 等
```

## 数据位置

绿色便携：数据库就在 `dictation.exe` 同级目录的 `dictation.db`（WAL 模式），备份在同目录 `backups/`。整个文件夹拷走即可换电脑。

程序目录不可写时（例如装在 `Program Files`）自动回落到 `%APPDATA%\com.dictation.desktop\`；macOS 一律放 `~/Library/Application Support/com.dictation.desktop\`。实际路径可在「设置 → 数据与备份」查看。

**想清空数据**：关闭应用后删除 `dictation.db`（以及 `-wal`、`-shm`），下次打开就是全新的空库——不会从旧位置恢复。

## 关键设计

- **词条进度只走 `repo::word_progress`**：后续拆多孩子（`word_progress` 表）时是纯搬迁，不用改调用方
- **错题本不单独建表**：由 `words.status='learning'` 派生，避免错题表与词表不同步
- **抽题顺序落库**：`session_items` 上 `UNIQUE(session_id, word_id)` 保证一场内不重复，`seq` 保证顺序可复盘
- **跨组去重**：同一词语在多个分组只抽一次（保留分组顺序靠前的那条）
- **口算做题与词表完全隔离**：`math_sessions` / `math_items` 开卷即快照题目，不碰 `word_progress`、不进错题本；判题只做整数比较，做题时不回传对错（ADR-23）
- **备份导入是合并非覆盖**：同编号同名分组视为同一个，已存在的词跳过，不破坏现有进度
- **同步只搬整库快照**：本地 SQLite 永远是唯一主库，云端只存一行快照 + 版本，拉取后下次启动替换（ADR-17）
- **默认离线**：笔顺与字典数据随包发出，不连 CDN；AI 与同步都是显式配置后才联网（ADR-18/20/22）
