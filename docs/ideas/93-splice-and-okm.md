# splice 与 okm：查询语言与内置存储

**状态**：候选（未实现）

**一句话**：splice 是 okm 的计划与协调层，okm-query 是有序流的算子层；okm 做内置存储的价值不在方便，而在节点的写与它的日志 entry 能落同一个 batch。

**动机**：

- 两者都是图，容易被误当成同一张图；程序图（节点是算子）与数据图（节点是实体）是两回事，查询是两者之间的映射。
- 行级算法已经在 okm-query 里，splice 重写没有意义；splice 该给的是计划、下推与审计。
- 内置存储真正的理由只有一个：恰好一次。

**形态**：

## 作为查询语言

- **图是逻辑/物理计划**：节点是算子，list 进 list 出。行级工作留在节点内——容器不是给表的，百万行逐元素调度不可行（[88](88-containers-and-joins.md) 的逐元素模式用于几十到几百个元素）。
- **okm-query 已有的**：有序 scan 流上的 merge（外连接的一般形式）/ intersect / difference / union、group_by、galloping 的 intersect_seek；一跳 = 一次 scan。set 语义不是 multiset：配对展开留给调用方，避免 k² 物化。
- **计划负责**：前缀与索引选择、limit / top-k 下推（okm-query 明说 LIMIT 切不断 k² 物化）、set join 的顺序。这些是优化器的事；图本身不做优化，别指望它自动重排。
- **splice 白送的两样**：可渲染（查询为什么这么跑）、可审计（每次查询一份 run 文档）。
- **两种图别混**：程序图节点是算子，数据图节点是实体；形状相似是视觉收益，不是表示收益。
- **模式查询（Cypher 式）另算**：需要 matcher 节点（子图模板 + 绑定），没有合一与回溯；「一跳一 scan」只能拼定长路径，变长路径要环或 matcher。

## 作为内置存储

- **源与汇**：scan（前缀/区间/索引）、get、put / del（走 Batch）。
- **主理由**：Batch + commit_batch 在 fjall（`db.batch()`）、redb（写事务）、ephemeral（锁）下都是原子的；远程路径一帧提交（ADR-0010）。若 run 文档与数据同在一个 okm store，节点的写与日志 entry 落同一个 batch → 恰好一次，不需要幂等键。
- **约束四条**：
  - 日志在 k10r、数据在 okm，收益消失，退回 (run, step, pass, attempt) 幂等键。
  - BatchStore 拒绝 scan（`okm-core/src/error.rs:160`）：读-改-写必须先读、再开 batch 写；中间没有隔离保证，除非 okm 有条件写（未核实）。
  - 每次 scan 是快照（fjall snapshot / redb 读事务），但多次 scan 之间不是同一快照；全查询一致性要版本或事务。
  - 远程一帧提交，网络失败后状态未知，仍要幂等。

**代价**：

- **两种图同名不同物**：程序图与数据图在文档与 UI 里都要区分，否则「图上连一条边」会被读成改数据。
- **绑定 okm 的能力**：有序 scan、快照粒度、是否支持条件写。换存储要重做节点。
- **原子提交只在同 store 内成立**：跨 store 永远有一条幂等缝。
- **查询优化器是新组件**：splice 不提供；没有它，计划靠人（或 LLM）写。

**冲突**：[88](88-containers-and-joins.md)（容器与 combine）、[90](90-types.md)（值类型与布局）、[85](85-graph-linearization.md)（编译与压实）、[92](92-streams-and-batches.md)（批 = list，不是流）、[87](87-binary-graph-is-the-artifact.md)（产物格式）。

**若改掉会动到**：源/汇节点、run 文档的存放位置、恰好一次的实现、查询前端。