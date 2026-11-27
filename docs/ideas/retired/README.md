# 已归档：不采用的方案

这里放的是表达式语言时期的想法。它们在语义换成图之后不再成立，保留是为了查理由，不是候选。
每条文档头部的「结局」行写了它为什么不用；下面按族汇总。

## 表达式表面

语言的表达式表面整体退场：调用原语、括号与分组、中缀、管道、闭包与柯里化、块实参、具名函数、字母名中缀。表达式现在只剩边谓词这一个位置，谓词语法未定。

- [11 调用只有一个原语：接收者 + 名字 + arity 个实参](11-method-chain-primitive.md)
- [12 实参不加括号，arity 分组](12-arity-grouping.md)
- [13 符号名即运算符，优先级由首字符定级](13-infix-by-spelling.md)
- [14 不设管道运算符](14-no-pipe.md)
- [15 管道运算符](15-pipe-operator.md)
- [16 方法不是值，函数只有块](16-methods-are-not-values.md)
- [17 自动柯里化与偏应用](17-currying.md)
- [18 独立具名函数](18-named-functions.md)
- [19 尾块是普通实参](19-block-as-arg.md)
- [1A 字母名中缀 / 反引号 / 声明式 fixity](1A-named-infix.md)

## 声明与模块

图里没有用户代码结构：点访问、容器、模块与命名空间、装饰器、宏、构造控制、数据先行。子图取代了模块，端口取代了字段访问。

- [21 字段即方法，点只有一种读法](21-unified-dot-access.md)
- [22 struct / object / 模块是同一构造](22-three-containers.md)
- [23 模块是值，import 是绑定](23-modules-as-values.md)
- [24 命名空间树](24-namespace-tree.md)
- [25 装饰器是编译期标签](25-decorators-as-tags.md)
- [26 Python 式装饰器](26-python-decorators.md)
- [27 宏是语法层的函数](27-macros.md)
- [28 构造受控：字段默认模块私有](28-construct-controlled.md)
- [29 数据先行的硬杠杆与软杠杆](29-data-first-levers.md)

## 类型的机制

类型只剩端口上的形状与标签：继承、字典传递、单态化、种类、高阶合一、三种组合、arity 上限都不存在。

- [32 继承与线性化](32-inheritance.md)
- [33 字典传递](33-dict-passing.md)
- [34 单态化](34-monomorphization.md)
- [35 种类参数只用来按构造子派发](35-kinds.md)
- [36 种类推断与高阶合一](36-kind-inference.md)
- [38 组合只有三种：链、Fn、合取](38-composition.md)
- [39 arity 上限](39-arity-cap.md)

## 数据

谓词不需要查询语言；文本的内部表示归执行器。

- [43 JsonPath 式查询](43-jsonpath-query.md)
- [44 Text 的表示](44-text-repr.md)

## 后端与编译

用户代码不再编译成产物：无优化器、Cranelift、wasm-gc、自建 GC、层级区域（56a–56f）、逃逸分析、全程序、单展开目标。执行器自己可以借这些思路，但它们不是语言特性。

- [52 没有优化器，没有 LLVM](52-no-optimizer.md)
- [53 Cranelift 当后端](53-cranelift.md)
- [54 wasm-gc 作为运行时](54-wasm-gc.md)
- [55 自建 GC 与分配器](55-own-gc.md)
- [56a 区域当参数：把「提升」变成选句柄](56a-region-param-passing.md)
- [56b 禁止跨层反向引用：只比深度](56b-depth-check.md)
- [56c 闭包与 async：move 或独立区域树](56c-closure-async-region.md)
- [56d 小 GC 兜底：两套堆，零互指](56d-hybrid-gc.md)
- [56e 区域里的 Drop：清理链，不是逐值析构](56e-drop-under-arena.md)
- [56f 并发：每线程一棵区域树，任务靠 scope 借](56f-concurrency-region-tree.md)
- [57a 没有逃逸分析：分配点与逃逸点都在语法里](57a-no-escape-analysis.md)
- [58 全程序一个模块，没有链接器](58-whole-program.md)
- [59 所有糖展开到同一个目标](59-single-expansion-target.md)

## 宿主与运行时

脚本运行时不存在：三宿主产物、借用检查、核心 async、迭代协议、早退、模式覆盖。等待由调度器表达，并发由节点与容器表达。

- [61 一份产物，三个宿主](61-three-hosts.md)
- [64 borrow checker](64-borrow-checker.md)
- [65 核心 async](65-core-async.md)
- [66 迭代协议与循环](66-iteration-protocol.md)
- [67 早退](67-early-return.md)
- [68 模式覆盖](68-pattern-coverage.md)

## 表面语法

整程序的表面是图的文本表示，不是表达式语法；Rust 形、注记次序、Scala 形都只对旧语言成立。

- [75 Rust 表面语法](75-rust-surface-syntax.md)
- [80 注记一律后缀，因为删除只能影响后缀](80-position-order.md)
- [81 表面改成 Scala 形，只取链](81-scala-surface.md)
