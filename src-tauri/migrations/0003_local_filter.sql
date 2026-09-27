-- 超出 tag 上限时留在本地筛选的 tag（空格分隔，-tag 表示排除）；NULL 表示全部交给站点。
ALTER TABLE jobs ADD COLUMN local_filter TEXT;
ALTER TABLE subscriptions ADD COLUMN local_filter TEXT;
