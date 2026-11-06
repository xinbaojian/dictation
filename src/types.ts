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
  /** 历次批改中标记过「不会写」的字下标，对应 text 的字符位置 */
  wrongChars: number[];
  /** 整词算错（批改时没标具体字）的次数 */
  wholeWordWrong: number;
}

export interface Settings {
  maskSeconds: number;
  streakToMaster: number;
  wordFontSize: number;
  /** 启动时是否自动检查新版本；关闭后只能手动检查（默认开启） */
  autoCheckUpdates: boolean;
}

/** 抽词偏好：开一场默写时勾选，全不选 = 纯随机 */
export interface SampleOptions {
  /** 连字：下一个词优先和上一个词有相同的字 */
  chain: boolean;
  /** 同一个字在多个词里都写错过时优先 */
  repeatWrong: boolean;
  /** 含批改标过的字优先 */
  weakFirst: boolean;
  /** 没默写过的新词优先 */
  newFirst: boolean;
  /** 最近一场写过的词排后 */
  lastBack: boolean;
  /** 同一批尽量不重复用字 */
  diverse: boolean;
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

/** 自有 MySQL 快照同步配置；注意这里永远不含密码 */
export interface SyncConfig {
  enabled: boolean;
  host: string;
  port: number;
  database: string;
  user: string;
  deviceName: string;
}

export interface SyncStatus {
  configured: boolean;
  enabled: boolean;
  hasPassword: boolean;
  config: SyncConfig;
  deviceId: string;
  lastSyncAt: string;
  lastSyncFrom: string;
  /** 有下载好的快照等待下次启动替换 */
  pending: boolean;
  /** 本机同步文件异常时的告警；非空表示推送/拉取已被暂停 */
  warning: string;
  /** 上次启动替换快照失败的原因，可手动清除；不禁用同步 */
  lastError: string;
  /** .sync-state.json 是否损坏；为 true 才能重置同步状态 */
  stateCorrupt: boolean;
}

export interface SyncOutcome {
  kind: "applied" | "conflict" | "unchanged" | "pending" | "empty";
  message: string;
  remoteVersion: number;
  /** 需要用户在确认框里决定是否强制覆盖 */
  needsConfirm: boolean;
}

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

/** 口算四位置的取值区间；字段名与后端 camelCase 对应（aMin/aMax…）。
 *  全部为默认值（1-9 / 1-9 / 10-50 / 1-100）时走三段难度；自定义则单池平铺。 */
export interface MathRanges {
  aMin: number;
  aMax: number;
  bMin: number;
  bMax: number;
  cMin: number;
  cMax: number;
  dMin: number;
  dMax: number;
}

export const DEFAULT_MATH_RANGES: MathRanges = {
  aMin: 1,
  aMax: 9,
  bMin: 1,
  bMax: 9,
  cMin: 10,
  cMax: 50,
  dMin: 1,
  dMax: 100,
};
