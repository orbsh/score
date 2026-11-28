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
- resume 保存环的预算：回边每花一次预算就往日志写一条 `kind: "budget"` 行，恢复时从剩余值继续，不再重发一份满预算。真正剩下的限制是：发过火的节点被 seed 之后不再发火，所以恢复出来的环停在那里，不会再转。
- 调用是节点：`m = double(x: n)` 造一个 `op = double` 的节点，def 体只存一份（`Graph.defs`），运行期按帧跑。签名在体之前注册，所以体里可以调用自己——递归的**机制**有了（帧各有 values/stamps/budgets/环进度，栈在堆上，超 1000 帧报「递归不终止」）。
- 递归还停不下来是因为缺一个合并点：节点的输出只有一个生产者，分支只能沿守卫边分叉到不同消费者，没法在「基值」和「递归结果」之间选一个。任何自调用都会跑到帧上限。补这个需要条件节点（merge），是语言决定，不是管道活。
- 多参数 def 从链上进入时要显式写入口端口 `<p|>`。
- `map` / `fold` 已能对 def 逐元素求值，但不能捕获外层值（body 只看得见自己的参数）；`map` 的输出是 `List<Any>`，因为不会去推 body 的元素类型。
- `Ty` 是 `Int` / `Text` / `Bool` / `Unit` / `List<T>` / `Record` / `Any` 加 `Named`：端口上可以写 `enum` / `struct` 声明的类型名，未声明的名字被 validate 拒绝；列表带元素类型（`List<Int>`、`List<Any>`，可嵌套），元素不合就是错误。
- 静态结构与动态对象是两件事：`struct` 是静态的（指名、字段带类型、写错字段名报错、`get` 带出字段类型），`record` 是动态的（形状未知，`Ty::Record` 故意不带字段，`get` 恒为 `Any`）。动态侧算子 `put` / `keys` / `has` 只吃 `Record`，把 `struct` 值喂进去是类型错误——字典不能悄悄吞掉结构。
- 动态侧还缺：没有 null、没有数值/数组的统一联合类型（`serde_json::Value` 那种"一个类型装下所有动态数据"的形态），没有删键（`put` 是加/覆盖，返回新值），键序是 BTreeMap 的字典序。
- 和类型由 `match(on: s, Circle: f, Rect: g)` 消费：臂的形参必须就是变体的字段（同名同序），每个变体都要有臂，少一个 validate 就拒；被 match 的值必须来自构造器（直接或经 def 内联），validate 由它得知该和类型有哪些臂。
- 和类型还没有穷尽性以外的东西：没有 `[*]` 兜底臂；`match` 也不接受类型为 `Any` 的值。
- `match` 的臂必须产出同一类型，该类型记在节点上（`Graph.node_tys`），出边按它检查，`score graph` 显示成 `.out: Text`。
- 属性只有 `@doc` / `@tag` 两个，且没有消费方（不进指纹、不影响运行）；自定义装饰器还没有注册机制。
- 属性目前能落的位置是声明（`def` / `enum` / `struct`）和一行里的第一个节点；字段上的属性还没有（字段没有消费方之前不加）。
- 前端分两遍，都不可选：parse 检查"文字与已读到的声明"（形参是否存在、变体带不带这个字段、match 各臂类型是否一致、实参类型是否合端口——含字段读取）；validate 检查"装配好的图"（端口存在、每个入边唯一、条件分支必有 `[*]`、环必须有预算、边上流过去的类型）。CLI 每条命令都先跑这两遍，所以"通过"就等于"能跑"。
- 没有类型推断：每个类型要么是程序写下的（端口、字段、def 签名），要么是读的时候记下的（`Graph.node_tys`、`types::known_ty`），两者都只是查表。
- okm 查询层只是想法，先不做。

## 验收总览

```
cargo test
cargo clippy --all-targets -- -D warnings
cargo run -p score-cli -- run examples/sum.score
```