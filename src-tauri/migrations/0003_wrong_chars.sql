-- 0003: 批改时可标记词组里具体哪个字不会写（按逗号分隔的 0-based 下标，空串表示整词错）
ALTER TABLE session_items ADD COLUMN wrong_chars TEXT NOT NULL DEFAULT '';
