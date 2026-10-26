import { invoke } from "@tauri-apps/api/core";
import type {
  BackupInfo,
  Group,
  GroupDeleteResult,
  ImportResult,
  ImportSummary,
  Mistake,
  ReviewResult,
  Session,
  SessionSummary,
  Settings,
  SplitResult,
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
  createSession: (groupIds: number[], scope: string, limit: number, shuffle: boolean) =>
    invoke<Session>("create_session", { groupIds, scope, limit, shuffle }),
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
  setWordMastered: (wordId: number, mastered: boolean) =>
    invoke<void>("set_word_mastered", { wordId, mastered }),

  // 设置与备份
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (value: Settings) => invoke<Settings>("save_settings", { value }),
  createBackup: (destDir?: string | null) =>
    invoke<string>("create_backup", { destDir: destDir ?? null }),
  listBackups: () => invoke<BackupInfo[]>("list_backups"),
  importBackup: (path: string) => invoke<ImportSummary>("import_backup", { path }),
  openDataDir: () => invoke<void>("open_data_dir"),
  dataDirPath: () => invoke<string>("data_dir_path"),
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
