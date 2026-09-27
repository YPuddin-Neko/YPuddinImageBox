-- 图库排序和筛选用的索引。十万张时按分数、收藏、文件大小、分辨率排序，以及按分级、来源筛选，都不用整表扫描再排序。
CREATE INDEX posts_score ON posts (score);
CREATE INDEX posts_fav_count ON posts (fav_count);
CREATE INDEX posts_file_size ON posts (file_size);
CREATE INDEX posts_pixels ON posts (width * height);
CREATE INDEX posts_rating ON posts (rating);
CREATE INDEX posts_source ON posts (source);
