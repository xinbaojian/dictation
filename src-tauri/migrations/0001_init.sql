-- 0001_init: 初始表结构
CREATE TABLE groups (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  code         TEXT NOT NULL,
  name         TEXT NOT NULL,
  sort_order   INTEGER NOT NULL DEFAULT 0,
  is_archived  INTEGER NOT NULL DEFAULT 0,
  created_at   TEXT NOT NULL
);

CREATE TABLE words (
  id                INTEGER PRIMARY KEY AUTOINCREMENT,
  group_id          INTEGER NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
  text              TEXT NOT NULL,
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
  config_json  TEXT NOT NULL,
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
  seq          INTEGER NOT NULL,
  skip_marked  INTEGER NOT NULL DEFAULT 0,
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
