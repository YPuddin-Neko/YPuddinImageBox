-- 订阅：按设定的间隔检查条件下有没有新图，有就自动下载。时间戳都是 Unix 毫秒。
CREATE TABLE subscriptions (
    id               INTEGER PRIMARY KEY,
    source           TEXT    NOT NULL,
    -- 用户填写的 tag（不含分级）和选中的分级（逗号分隔），界面显示用。
    tags             TEXT    NOT NULL,
    ratings          TEXT    NOT NULL,
    -- 发给站点的查询串。
    query            TEXT    NOT NULL,
    enabled          INTEGER NOT NULL DEFAULT 1,
    interval_minutes INTEGER NOT NULL,
    -- 已经处理到的最大帖子 id，比它新的才算新图。
    last_seen_id     INTEGER NOT NULL DEFAULT 0,
    last_checked_at  INTEGER,
    -- 最近一次检查找到的新图张数。
    last_new         INTEGER NOT NULL DEFAULT 0,
    last_error       TEXT,
    created_at       INTEGER NOT NULL,
    updated_at       INTEGER NOT NULL
);

-- 订阅检查生成的下载任务。订阅删掉后任务保留，只断开关联。
ALTER TABLE jobs ADD COLUMN subscription_id INTEGER REFERENCES subscriptions (id) ON DELETE SET NULL;
CREATE INDEX jobs_subscription ON jobs (subscription_id, status);
