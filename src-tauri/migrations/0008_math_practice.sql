-- 0008: 口算上机做题（卷 + 每题作答）
-- 刻意不复用 sessions / session_items：后者的 word_id 是 words 的外键（NOT NULL），
-- 口算题没有对应词条，塞进去会污染错题本与词条进度。
-- 开卷就把 30 道题写进 math_items（题目快照），本题库不依赖出题引擎的当前版本：
-- 以后改出题算法，历史卷与断点续做仍然一模一样。
CREATE TABLE math_sessions (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  seed          INTEGER NOT NULL,
  -- 出题条件快照（pos/op1/op2/count/ranges）：留档备查，便于复现一张卷
  config_json   TEXT NOT NULL,
  total         INTEGER NOT NULL DEFAULT 30 CHECK (total > 0),
  correct_total INTEGER CHECK (correct_total IS NULL OR correct_total >= 0),
  duration_ms   INTEGER CHECK (duration_ms IS NULL OR duration_ms >= 0),
  status        TEXT NOT NULL CHECK (status IN ('running','finished','abandoned')),
  started_at    TEXT NOT NULL,
  finished_at   TEXT
);

CREATE INDEX ix_math_sessions_started ON math_sessions(started_at DESC);

CREATE TABLE math_items (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id  INTEGER NOT NULL REFERENCES math_sessions(id) ON DELETE CASCADE,
  seq         INTEGER NOT NULL,
  a           INTEGER NOT NULL,
  b           INTEGER NOT NULL,
  c           INTEGER NOT NULL,
  d           INTEGER NOT NULL,
  t           INTEGER NOT NULL,
  op1         TEXT NOT NULL CHECK (op1 IN ('m','d','a','s')),
  op2         TEXT NOT NULL CHECK (op2 IN ('m','d','a','s')),
  blank_pos   INTEGER NOT NULL CHECK (blank_pos BETWEEN 1 AND 4),
  -- NULL = 还没做；'' = 按了跳过/交卷留空；其余是原始输入
  input       TEXT,
  -- NULL 未答 / 0 错 / 1 对（交卷统一公布，做题时不回传前端）
  correct     INTEGER CHECK (correct IS NULL OR correct IN (0, 1)),
  elapsed_ms  INTEGER CHECK (elapsed_ms IS NULL OR elapsed_ms >= 0),
  answered_at TEXT
);

CREATE UNIQUE INDEX ux_math_items_seq ON math_items(session_id, seq);
