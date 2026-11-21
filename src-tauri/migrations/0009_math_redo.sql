-- 0009: 口算「重做错题」
-- redo_of 非空 = 这张卷是某张已交卷卷子的「错题重做卷」（把那张卷的错题复制成一张新卷再做一遍）。
-- 重做卷不参与「全对最快」纪录：它的题量可能与正式卷相同（例如重做 5 道错题 vs 正式的 5 道卷），
-- 却只做错题，用时不可比；记录页会把重做卷标出来，让家长分清哪次是正式卷、哪次是重做。
-- 删掉原卷时重做卷保留，只是标记回落成普通卷（ON DELETE SET NULL）。
ALTER TABLE math_sessions ADD COLUMN redo_of INTEGER REFERENCES math_sessions(id) ON DELETE SET NULL;
