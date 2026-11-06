import { invoke } from "@tauri-apps/api/core";
import type {
  BackupInfo,
  Group,
  GroupDeleteResult,
  ImportResult,
  ImportSummary,
  MathRanges,
  MathSheet,
  Mistake,
  ReviewResult,
  SampleOptions,
  Session,
  SessionSummary,
  Settings,
  SplitResult,
  SyncConfig,
  SyncOutcome,
  SyncStatus,
  Word,
  WrongMark,
} from "../types";

// 所有后端调用集中在此，命令名与 Rust commands/ 一一对应
export const api = {
  // 词表
  listGroups: () => invoke<Group[]>("list_groups"),
  createGroup: (parentId: number | null, code: string, name: string) =>
    invoke<Group>("create_group", { parentId, code, name }),
  updateGroup: (id: number, code: string, name: string) =>
    invoke<Group>("update_group", { id, code, name }),
  deleteGroup: (id: number) => invoke<GroupDeleteResult>("delete_group", { id }),
  /** 拖动调整结构：移到 parentId 下、插在 beforeId 之前（null = 排到末尾），返回新的整棵树 */
  moveGroup: (id: number, parentId: number | null, beforeId: number | null) =>
    invoke<Group[]>("move_group", { id, parentId, beforeId }),
  listWords: (groupId: number, deep = false) => invoke<Word[]>("list_words", { groupId, deep }),
  addWord: (groupId: number, text: string, note: string | null) =>
    invoke<Word>("add_word", { groupId, text, note }),
  updateWord: (id: number, text: string, note: string | null) =>
    invoke<Word>("update_word", { id, text, note }),
  deleteWord: (id: number) => invoke<void>("delete_word", { id }),
  previewWords: (text: string) => invoke<SplitResult>("preview_words", { text }),
  importWords: (groupId: number, items: string[]) =>
    invoke<ImportResult>("import_words", { groupId, items }),

  // 默写
  countCandidates: (groupIds: number[], scope: string) =>
    invoke<number>("count_candidates", { groupIds, scope }),
  createSession: (groupIds: number[], scope: string, limit: number, shuffle: boolean, options: SampleOptions) =>
    invoke<Session>("create_session", { groupIds, scope, limit, shuffle, options }),
  getSession: (id: number) => invoke<Session>("get_session", { id }),
  activeSession: () => invoke<Session | null>("active_session"),
  updateSessionProgress: (sessionId: number, currentSeq: number, skipItemId: number | null) =>
    invoke<void>("update_session_progress", { sessionId, currentSeq, skipItemId }),
  finishSession: (id: number) => invoke<Session>("finish_session", { id }),
  abandonSession: (id: number) => invoke<void>("abandon_session", { id }),
  submitReview: (sessionId: number, wrongItems: WrongMark[]) =>
    invoke<ReviewResult>("submit_review", { sessionId, wrongItems }),
  retrySession: (id: number) => invoke<Session>("retry_session", { id }),
  listSessions: (limit: number) => invoke<SessionSummary[]>("list_sessions", { limit }),
  /** 删记录不动词条进度：错题本与连对次数是学习结果，不跟着记录走 */
  deleteSession: (id: number) => invoke<void>("delete_session", { id }),

  // 错题本
  listMistakes: (groupId: number | null) => invoke<Mistake[]>("list_mistakes", { groupId }),
  /** 手工把词语加进错题本；wrongChars 是「不会写的字」下标（越界由后端丢弃） */
  addMistake: (groupId: number, text: string, wrongChars: number[]) =>
    invoke<Word>("add_mistake", { groupId, text, wrongChars }),
  setWordMastered: (wordId: number, mastered: boolean) =>
    invoke<void>("set_word_mastered", { wordId, mastered }),

  // 口算练习
  /** 出一张 30 题的口算卷；ranges 缺省用默认区间；无解符号组合返回 null */
  generateMathSheet: (
    seed: number,
    pos: string,
    op1: string,
    op2: string,
    ranges?: MathRanges,
  ) =>
    invoke<MathSheet | null>("generate_math_sheet", {
      seed,
      pos,
      op1,
      op2,
      ranges: ranges ?? null,
    }),

  // 设置与备份
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (value: Settings) => invoke<Settings>("save_settings", { value }),
  createBackup: (destDir?: string | null) =>
    invoke<string>("create_backup", { destDir: destDir ?? null }),
  listBackups: () => invoke<BackupInfo[]>("list_backups"),
  importBackup: (path: string) => invoke<ImportSummary>("import_backup", { path }),
  openDataDir: () => invoke<void>("open_data_dir"),
  dataDirPath: () => invoke<string>("data_dir_path"),

  // 自动升级：更新检查/下载走 @tauri-apps/plugin-updater（见 lib/updater.ts），
  // 这两个是本项目自己的命令
  appVersion: () => invoke<string>("app_version"),
  /** 重启后当前进程立刻退出，invoke 会抛错，调用方需忽略 */
  restartApp: () => invoke<void>("restart_app"),

  // 跨设备同步：默认完全离线，只有配置了自有 MySQL 才会有网络请求
  syncStatus: () => invoke<SyncStatus>("sync_status"),
  /** password 传 null / 空串表示沿用已保存的密码；状态返回值永远不含密码 */
  saveSyncConfig: (config: SyncConfig, password: string | null) =>
    invoke<SyncStatus>("save_sync_config", { config, password }),
  /** 测连接，不落盘：传当前表单即可，避免为了试一个错端口覆盖掉能用的凭据 */
  testSyncConnection: (config: SyncConfig | null, password: string | null) =>
    invoke<string>("test_sync_connection", { config, password }),
  syncPush: (force: boolean) => invoke<SyncOutcome>("sync_push", { force }),
  syncPull: (force: boolean) => invoke<SyncOutcome>("sync_pull", { force }),
  /** 放弃已下载但还没应用的 pending 快照 / 清除失败提示；不改动主库与主状态 */
  discardSyncPending: () => invoke<SyncStatus>("sync_discard_pending"),
  /** 隔离损坏的 .sync-state.json 并重建；仅在 status.stateCorrupt 为 true 时调用 */
  repairSyncState: () => invoke<SyncStatus>("sync_repair_state"),
};

export function errorMessage(err: unknown): string {
  if (typeof err === "string") return err;
  if (err instanceof Error) return err.message;
  return String(err);
}

/** 练习时间的相对描述，用于分组列表 */
export function practiceLabel(iso: string | null): string {
  if (!iso) return "还没练过";
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return "";
  const now = new Date();
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
  const that = new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();
  const days = Math.round((today - that) / 86400000);
  if (days <= 0) return "今天练过";
  if (days === 1) return "昨天练过";
  if (days < 7) return days + " 天前练过";
  return date.getMonth() + 1 + " 月 " + date.getDate() + " 日练过";
}

export function formatTime(iso: string | null): string {
  if (!iso) return "—";
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  return date.toLocaleString("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}
