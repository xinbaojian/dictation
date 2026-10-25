# 生词默写（Dictation）

给二年级孩子做生词默写的单机桌面小工具：家长读词、孩子手写、家长批改，工具负责抽题、防偷看、记录错题。

详细需求见 [REQUIREMENTS.md](./REQUIREMENTS.md)。

## 功能

- **词表**：多级分组（二年级上册 → 第一单元 → …，任意层级），左侧树可**拖动调整层级与顺序**；单条录入，整段粘贴导入（自动切分、去重、剥离「1.」序号）
- **默写**：多分组 + 范围（全部 / 仅未掌握 / 错题本）+ 题量，随机抽词且一场内不重复，可开「连字抽词」让下一个词优先和上一个词有相同的字；翻到新词先显示、按设置秒数自动遮住（可随时点开重看）
- **断点**：中途退出不丢进度，下次可继续；默写结束未批改也能下次接着批
- **批改**：网格点选错词，还能细化到「哪个字不会写」；错词自动进错题本（仍按整词）
- **错题本**：连续答对 N 次自动出本（默认 2），也可手动判定已掌握、按分组筛选、专项默写
- **记录**：历史会话 + 抽取顺序 + 只练本次错词
- **设置**：遮罩时长、出本连对数、词语字号、数据目录、手动备份导出/导入

## 文档

- [REQUIREMENTS.md](./REQUIREMENTS.md)：需求规格（目标、功能编号、数据模型、里程碑）
- [AGENTS.md](./AGENTS.md)：接手仓库的操作手册（命令、约束、常见任务）
- [wiki/](./wiki/README.md)：项目 wiki —— 架构、数据模型、默写流程、设计决策、踩坑

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
cd src-tauri && cargo test # 后端单测：切分 / 抽题 / 状态机 / 会话流转
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

## 目录

```
src-tauri/
  migrations/            0001 表结构，0002 会话进度
  src/db/                连接 + user_version 递增迁移
  src/domain/            text.rs 粘贴切分、sampler.rs 抽题（纯函数，带单测）
  src/repo/              groups / words / sessions / word_progress / settings / backup
  src/commands/          library / quiz / system 三组 #[tauri::command]
  src/speech/            TTS 抽象 trait Speaker（V1 空实现，V2 接入）
src/
  pages/                 Pick 抽题 / Quiz 默写 / Review 批改 / Mistakes 错题本 / History 记录 / Settings 设置 / Wordbook 词表
  lib/ipc.ts             所有后端调用集中于此
  components/            Modal / ConfirmModal / Toast / 分组弹窗 / 粘贴导入弹窗
```

## 数据位置

绿色便携：数据库就在 `dictation.exe` 同级目录的 `dictation.db`（WAL 模式），备份在同目录 `backups/`。整个文件夹拷走即可换电脑。

程序目录不可写时（例如装在 `Program Files`）自动回落到 `%APPDATA%\com.dictation.app\`，实际路径可在「设置 → 数据与备份」查看。

**想清空数据**：关闭应用后删除 `dictation.db`（以及 `-wal`、`-shm`），下次打开就是全新的空库——不会从旧位置恢复。

## 关键设计

- **词条进度只走 `repo::word_progress`**：后续拆多孩子（`word_progress` 表）时是纯搬迁，不用改调用方
- **错题本不单独建表**：由 `words.status='learning'` 派生，避免错题表与词表不同步
- **抽题顺序落库**：`session_items` 上 `UNIQUE(session_id, word_id)` 保证一场内不重复，`seq` 保证顺序可复盘
- **跨组去重**：同一词语在多个分组只抽一次（保留分组顺序靠前的那条）
- **备份导入是合并非覆盖**：同编号同名分组视为同一个，已存在的词跳过，不破坏现有进度
