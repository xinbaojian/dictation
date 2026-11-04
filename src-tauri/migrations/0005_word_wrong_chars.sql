-- 手工加入错题本时标记的错字（字符下标，逗号分隔，空串 = 没标具体字）
-- 与批改记录 session_items.wrong_chars 分开存：这里没有会话，是家长直接记录的错
ALTER TABLE words ADD COLUMN wrong_chars TEXT NOT NULL DEFAULT '';
