export type WordStatus = "new" | "learning" | "mastered";
export type Scope = "all" | "unlearned" | "mistake";
export type SessionStatus = "running" | "reviewing" | "finished" | "abandoned";

export interface Group {
  id: number;
  parentId: number | null;
  code: string;
  name: string;
  /** 层级，根为 0 */
  depth: number;
  /** 完整路径，如 "二年级上册 / 第一单元" */
  path: string;
  pathSegments: string[];
  sortOrder: number;
  /** 本节点直属词条数 */
  wordCount: number;
  /** 含所有子分组 */
  totalWordCount: number;
  learningCount: number;
  totalLearningCount: number;
  hasChildren: boolean;
  lastPracticedAt: string | null;
}

export interface GroupDeleteResult {
  groups: number;
  words: number;
}

export interface Word {
  id: number;
  groupId: number;
  text: string;
  note: string | null;
  status: WordStatus;
  wrongCount: number;
  correctStreak: number;
  lastPracticedAt: string | null;
}

export interface SplitResult {
  words: string[];
  duplicates: number;
  dropped: number;
}

export interface ImportResult {
  inserted: number;
  skipped: number;
}

export interface SessionItem {
  id: number;
  seq: number;
  wordId: number;
  text: string;
  skipMarked: boolean;
  result: "pending" | "correct" | "wrong";
  /** 批改时标出的「不会写的字」下标，空数组表示整词算错 */
  wrongChars: number[];
}

export interface WrongMark {
  itemId: number;
  chars: number[];
}

export interface Session {
  id: number;
  mode: "group" | "mistake";
  status: SessionStatus;
  total: number;
  wrongTotal: number;
  currentSeq: number;
  startedAt: string;
  finishedAt: string | null;
  groupLabel: string;
  scope: Scope;
  items: SessionItem[];
}

export interface SessionSummary {
  id: number;
  mode: "group" | "mistake";
  status: SessionStatus;
  total: number;
  wrongTotal: number;
  startedAt: string;
  finishedAt: string | null;
  groupLabel: string;
  scope: Scope;
}

export interface Mistake {
  id: number;
  text: string;
  groupId: number;
  groupCode: string;
  groupName: string;
  wrongCount: number;
  correctStreak: number;
  lastPracticedAt: string | null;
}

export interface Settings {
  maskSeconds: number;
  streakToMaster: number;
  wordFontSize: number;
}

export interface ReviewResult {
  wrong: number;
  correct: number;
  graduated: number;
}

export interface BackupInfo {
  name: string;
  path: string;
  size: number;
}

export interface ImportSummary {
  groupsAdded: number;
  groupsMerged: number;
  wordsAdded: number;
  wordsSkipped: number;
}
