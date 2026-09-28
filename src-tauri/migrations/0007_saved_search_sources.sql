-- 收藏的搜索可以勾选几个站点一起搜：source 改存按固定顺序、逗号分隔的站点，例如 danbooru,gelbooru。
-- 上一版把「全部平台」存成 all，当时只有这两个站点，换成同样的写法；已经有同样的收藏时去掉这条重复的。
UPDATE OR IGNORE saved_searches SET source = 'danbooru,gelbooru' WHERE source = 'all';
DELETE FROM saved_searches WHERE source = 'all';
