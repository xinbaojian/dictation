-- AI 讲解卡缓存：按 (word_id, kind) 唯一。API Key 不入库（在数据目录 .ai-config.json）。
-- 本表内容随整库快照同步（ADR-17）：一台机器生成、两台可用。
-- 词删除靠外键级联；词文本被编辑时由 repo/words.rs::update 主动清卡（讲解针对旧字形）。
CREATE TABLE ai_cards (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  word_id      INTEGER NOT NULL REFERENCES words(id) ON DELETE CASCADE,
  kind         TEXT NOT NULL DEFAULT 'mnemonic',
  content_json TEXT NOT NULL,
  model        TEXT NOT NULL,
  created_at   TEXT NOT NULL,
  updated_at   TEXT NOT NULL
);
CREATE UNIQUE INDEX ux_ai_cards_word_kind ON ai_cards(word_id, kind);
