export type WordStatus = "new" | "learning" | "mastered";
export type Scope = "all" | "unlearned" | "mistake";
export type SessionStatus = "running" | "reviewing" | "finished" | "abandoned";

export interface Group {
  id: number;
  code: string;
  name: string;
  sortOrder: number;
  wordCount: number;
  learningCount: number;
  lastPracticedAt: string | null;
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
