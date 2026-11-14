-- 查字工具的 AI 字卡缓存：按字文本唯一（与 ai_cards 按 word_id 不同，
-- 查字工具的字不要求在词表里）。内容随整库快照同步（ADR-17 / ADR-22）；
-- 词表增删改不影响本表，所以没有外键、也没有清卡联动。
CREATE TABLE char_cards (
  char         TEXT PRIMARY KEY,
  content_json TEXT NOT NULL,
  model        TEXT NOT NULL,
  created_at   TEXT NOT NULL,
  updated_at   TEXT NOT NULL
);
