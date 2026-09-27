-- 图库：已下载到本地的帖子。时间戳都是 Unix 毫秒。
CREATE TABLE posts (
    id            INTEGER PRIMARY KEY,
    source        TEXT    NOT NULL,
    post_id       INTEGER NOT NULL,
    md5           TEXT,
    width         INTEGER NOT NULL,
    height        INTEGER NOT NULL,
    rating        TEXT,
    score         INTEGER NOT NULL DEFAULT 0,
    fav_count     INTEGER,
    file_ext      TEXT    NOT NULL,
    file_size     INTEGER,
    file_url      TEXT,
    created_at    TEXT,
    post_url      TEXT    NOT NULL,
    -- 绝对路径。图片位置整体移动时一起改写；选择「已有图片留在原处」时保持不变。
    local_path    TEXT    NOT NULL,
    downloaded_at INTEGER NOT NULL,
    UNIQUE (source, post_id)
);
CREATE INDEX posts_md5 ON posts (md5);
CREATE INDEX posts_downloaded_at ON posts (downloaded_at);

-- tag 按名称合并，两个站点共用。分类：artist / copyright / character / general / meta。
CREATE TABLE tags (
    id       INTEGER PRIMARY KEY,
    name     TEXT NOT NULL UNIQUE,
    category TEXT NOT NULL
);

CREATE TABLE post_tags (
    post_id INTEGER NOT NULL REFERENCES posts (id) ON DELETE CASCADE,
    tag_id  INTEGER NOT NULL REFERENCES tags (id) ON DELETE CASCADE,
    PRIMARY KEY (post_id, tag_id)
) WITHOUT ROWID;
CREATE INDEX post_tags_tag ON post_tags (tag_id, post_id);

-- 下载任务。kind：posts（选中的帖子）/ query（按条件下载全部结果）。
-- status：queued / running / paused / done / failed / canceled。
CREATE TABLE jobs (
    id          INTEGER PRIMARY KEY,
    kind        TEXT    NOT NULL,
    source      TEXT    NOT NULL,
    title       TEXT    NOT NULL,
    query       TEXT,
    -- query 任务最多下载前多少张，NULL 表示不限。
    max_posts   INTEGER,
    status      TEXT    NOT NULL,
    total       INTEGER,
    saved       INTEGER NOT NULL DEFAULT 0,
    skipped     INTEGER NOT NULL DEFAULT 0,
    failed      INTEGER NOT NULL DEFAULT 0,
    -- query 任务下一页的页码参数；NULL 表示已经翻完。
    cursor      TEXT,
    error       TEXT,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);

-- 任务里的每张图。status：pending / saved / skipped / failed。
CREATE TABLE job_items (
    job_id  INTEGER NOT NULL REFERENCES jobs (id) ON DELETE CASCADE,
    seq     INTEGER NOT NULL,
    post_id INTEGER NOT NULL,
    data    TEXT    NOT NULL,
    status  TEXT    NOT NULL,
    note    TEXT,
    PRIMARY KEY (job_id, seq)
) WITHOUT ROWID;
CREATE INDEX job_items_status ON job_items (job_id, status, seq);
