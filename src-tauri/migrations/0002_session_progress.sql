-- 0002: 记录默写进行到第几个词，支持中断恢复
ALTER TABLE sessions ADD COLUMN current_seq INTEGER NOT NULL DEFAULT 0;

CREATE INDEX ix_sessions_started ON sessions(started_at DESC);
