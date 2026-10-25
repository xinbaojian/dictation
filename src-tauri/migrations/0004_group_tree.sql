-- 0004: 分组支持多级（册 -> 单元 -> ...），删父级时级联删子级
ALTER TABLE groups ADD COLUMN parent_id INTEGER REFERENCES groups(id) ON DELETE CASCADE;

CREATE INDEX ix_groups_parent ON groups(parent_id);
