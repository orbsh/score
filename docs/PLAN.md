# PLAN：从语法到第一个能跑的 run

设计在 [ideas/](ideas/README.md)；这个文件只管实施顺序。第一目标是一个纵向切片：`score run x.score` 能解析、校验、执行一张小图，打印结果与轨迹。不追求完整语言。

## 原则

- 先让一条端到端路径成立，再扩语言。语法与模型以 `ideas/96` 为准，实现落在真实代码里之前，文档不改。
- 每个阶段有验收命令；红了不往下走。
- 不接 schematic。它的模型是 steps + 控制边，没有端口；score 的端口图与它不是同构。第一个切片自带 runtime，等模型稳定后再决定谁承担执行（`ideas/87`）。
- 类型先只做 Int / Text / Bool / Unit。

## Phase 0 骨架

- workspace：`crates/score-core`（值、语法前端、模型、validate、执行）、`crates/score-cli`（二进制 `score`）。
- edition 2024，`unsafe_code = "forbid"`，clippy `all` 为 warn。
- 验收：`cargo test` 与 `cargo clippy --all-targets -- -D warnings` 绿（空跑）。

## Phase 1 文本 → 图

- lexer：标识符、整数、字符串、`->`、`-->`、`-[`、`]->`、`<`、`|`、`>`、`(`、`)`、`{`、`}`、`,`、`:`、`=`、`@`、算术与逻辑符号、`#` 注释。
- parser：按行分组；组状态（头，当前）；行首 `->` 接续、`-->` 从头分叉、`-[expr]->` 条件分叉；调用 = 名字 + `<in|out>` + `(args)`；`const`。
- 模型：`Node { name, op, args, out_port }`、`Edge { from, to, guard }`。
- `def`：签名加体，调用点内联。参数只能由实参供给（不支持从链上进入 def），输出用 `<|name>` 选，不支持递归。
- 验收：`examples/*.score` 解析出的节点数、边数、端口名与预期一致（测试钉住）。

## Phase 2 validate

- 节点名唯一、算子存在、参数是合法端口且类型匹配。
- 一条入边一个端口；每个输入要么有参数要么有入边，不能两者都有。
- 条件里的裸名字必须是源节点的出口。
- 同一源端口有 `When` 时不能混用无条件边，且必须有 `[*]` 兜底。
- 验收：每条错误一个测试，措辞钉死。

## Phase 3 执行

- 值：`Int / Text / Bool / Unit`。
- 算子（v0 最小集）：`lit`、`add`、`concat`、`upper`、`len`、`print`。
- 调度：单线程工作表；节点输入齐了就发火；出口值按守卫规则投递；卡住时报告未完成的节点。
- 验收：`examples/sum.score` 跑出 `5`；`examples/branch.score` 只有一支发火；轨迹顺序稳定。

## Phase 4 CLI

- `score validate <file>`、`score run <file>`（`--trace` 打印每个节点的值）。
- 退出码：0 成功，1 解析/校验/执行错误。
- README 补「运行」一节。

## Phase 5 之后（不在第一切片）

- 子图的高级形态：递归、从链上进入 def、多输出解构、定义在调用之后。
- 容器 `map` / `fold`；环与 seed / budget；记录与和类型；okm 源汇；压实。

## 已知缺口

- 环只有预算一种形态：`-[budget: n]->` 的回边最多回灌 n 次；初值来自同一个端口的普通边（没有单独的 seed 节点）。无预算的环被 validate 拒绝，`unbounded` 靠运行期 100 万次发火上限兜底。
- resume 不保存环的预算进度：从日志恢复会把环的预算重新计一遍。
- `def` 只能由实参供给参数，不能从链上进入，也不能递归（定义必须先于调用）。
- `List` 没有元素类型参数：`List` 表示任意列表，`List<Row>` 还不存在；`map` / `fold` 已能对 def 逐元素求值，但不能捕获外层值（body 只看得见自己的参数）。
- 记录与和类型待 Phase 5；okm 查询层只是想法，先不做。

## 验收总览

```
cargo test
cargo clippy --all-targets -- -D warnings
cargo run -p score-cli -- run examples/sum.score
```