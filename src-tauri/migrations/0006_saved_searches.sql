-- 收藏的搜索条件：发现页里点一下就能重新搜，不会自动下载（自动下载用订阅）。
-- ratings 是按固定顺序排好、逗号分隔的分级，全选时为空字符串；同样的条件只存一份。
CREATE TABLE saved_searches (
    id         INTEGER PRIMARY KEY,
    source     TEXT    NOT NULL,
    tags       TEXT    NOT NULL,
    ratings    TEXT    NOT NULL,
    sort       TEXT    NOT NULL,
    created_at INTEGER NOT NULL,
    UNIQUE (source, tags, ratings, sort)
);
