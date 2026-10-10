-- 已有数字搜索保留 tag 语义，新搜索可显式保存作品/画师编号输入方式。
ALTER TABLE saved_searches RENAME TO saved_searches_previous;
CREATE TABLE saved_searches (
    id INTEGER PRIMARY KEY,
    source TEXT NOT NULL,
    tags TEXT NOT NULL,
    ratings TEXT NOT NULL,
    sort TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    pixiv_input INTEGER NOT NULL DEFAULT 0,
    UNIQUE (source, tags, ratings, sort, pixiv_input)
);
INSERT INTO saved_searches (id, source, tags, ratings, sort, created_at)
SELECT id, source, tags, ratings, sort, created_at FROM saved_searches_previous;
DROP TABLE saved_searches_previous;

-- 修复数字 tag 订阅被写成单作品查询的记录，保留创建时的时间边界。
ALTER TABLE subscriptions ADD COLUMN min_posted_at INTEGER;
UPDATE subscriptions
SET query = trim(tags) || substr(query, length('id:' || CAST(CAST(trim(tags) AS INTEGER) AS TEXT)) + 1),
    min_posted_at = created_at
WHERE source = 'pixiv' AND trim(tags) <> '' AND trim(tags) NOT GLOB '*[^0-9]*'
  AND (query = 'id:' || CAST(CAST(trim(tags) AS INTEGER) AS TEXT)
       OR query LIKE 'id:' || CAST(CAST(trim(tags) AS INTEGER) AS TEXT) || ' %');
UPDATE jobs SET query = (SELECT query FROM subscriptions WHERE id = jobs.subscription_id)
WHERE subscription_id IN (SELECT id FROM subscriptions WHERE min_posted_at IS NOT NULL)
  AND status IN ('queued', 'running', 'paused', 'failed');
