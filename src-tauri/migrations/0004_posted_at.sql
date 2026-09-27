-- 帖子的发布时间（Unix 毫秒），图库按上传先后排序用。两个站点 created_at 的格式不同，由程序解析后写入；
-- 这之前下载的图在打开图库时补上。
ALTER TABLE posts ADD COLUMN posted_at INTEGER;
CREATE INDEX posts_posted_at ON posts (posted_at);
