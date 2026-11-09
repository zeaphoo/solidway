-- 选号策略重设计：删除 `uptime`（可用性优先），其语义并入 `auto`（w_uptime 默认 0.5）。
--
-- 存量路由改写为 `auto`；数据面另有一层宽松解析（parse_legacy）把遗留的
-- `uptime` 映射为 `auto`，所以本迁移只负责把库里写干净，避免回滚时出现歧义。
UPDATE routes
SET selection_strategy = 'auto'
WHERE selection_strategy = 'uptime';
