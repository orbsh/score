# score

A graph-first language: the language is a binary graph, and text is one source that generates it. **Status: design draft with a first vertical slice** — the parser, the validator and a single-threaded worklist runtime live under `crates/`; the order is in [docs/PLAN.md](docs/PLAN.md).

Design: [docs/ideas/](docs/ideas/README.md) — one document per idea; retired ones live under docs/ideas/retired/ (written in Chinese). The pre-split long draft stays at [docs/design.md](docs/design.md) as history.

Positioning: [docs/ideas/94-value-proposition.md](docs/ideas/94-value-proposition.md) — code generation is cheap now, so the cost that decides is the cost of a wrong step; a statically checked, resumable, parallel plan is the layer that bash and Python glue do badly.

## Running the slice

```
cargo run -p score-cli -- validate examples/sum.score
cargo run -p score-cli -- run examples/sum.score --trace
```

Implemented today: the v0 surface (bindings, `<in|out>` ports, `->` / `-->` / `-[cond]->`, `const`, `def` with inlining), validation (ports, types, one edge per input, a `[*]` arm for every conditional branch), and a worklist runtime with six in-process operators (`lit`, `add`, `concat`, `upper`, `len`, `print`).

Not implemented yet: containers, rings, records and sums, okm sources. `def` exists in a first form — parameters are supplied as arguments, the output is picked by name, and recursion is rejected (definitions must precede their calls, which makes a cycle impossible).

Core proposition: the semantics is a graph — steps, control edges, subgraphs — and a run is the append-only log of what fired. A step fires when its inputs resolve; nothing is ordered by its position in the text. Subgraphs are the language's functions: a named graph with a typed signature, instantiated by wiring and inlined before it runs. A step's arguments are literals or references to other steps' ports; the only place an expression appears is an edge predicate. The artifact is the graph's binary form: text is a front end that generates it, rendering is a projection of the same bytes, and a subgraph exported from a graph store is a program without a conversion step.

The Lisp correspondence, stated precisely: Lisp gives a program a list representation, and the list is not the semantics — the AST is. Here the binary graph is both the semantics and the interchange format, which Lisp does not have (its AST lives in memory). The gain is not macros but structure: the same program can be rendered, folded level by level, resumed from its document, audited edge by edge, and stored as the graph it is.

Layers:

```
layer   what it is                                                        rules live in
graph   steps, kinds, control edges, budgets, subgraph signatures         the binary plan format; schematic implements the fold today
text    front ends that generate that binary (KDL, JSON, an editor)       this project (docs/ideas)
leaf    the expression language, used at edge predicates                  this project; evaluated by the executor
```

Constraints that follow from the graph being the source of truth:

- A subgraph's inside never connects to its outside except through its typed ports — otherwise collapsing it would hide edges and the drawing would lie (`84`).
- There is no call stack: instantiation is inlined at validate time with depth and instance caps, so expansion is a bounded worklist, not a fixed point (`83`, `57`).
- A `request` step may wait for days, so any lower artifact needs a resumable frame or the run document as its continuation (`85`).
- The model is schematic's, extended in schematic — never a second parallel model — so a later engine replacement swaps `validate`/`ready`/`advance`, not the format (`87`).

## The leaf

The leaf is a pure expression language with one place in the model: an edge's `when` predicate (`x > 5`). The declaration door lowers a predicate to a selector node, so the kernel still only sees labels and `*`. Node arguments are literals or references — computation is drawn as nodes (`88`) — and whether arguments may also take expressions is a policy, not a mechanism.

The expression surface is the one the earlier drafts described (a call is receiver + name + the arguments the declaration states; a type is data shape + a set of trait labels); the modules, traits and backend chapters do not apply. Evaluation belongs to the executor: a tree walk first, a bytecode evaluator only if measurement asks for one (`77`, `79`).

## Layout

```
docs/
├── ideas/          一个想法一份文档（允许冲突，没有终态）
│   └── retired/    不采用的方案，头部写为什么
├── reference/      语法总表与示例程序（谓词表达式）
└── design.md       拆分前的长文，保留备查

README.md
```

Read [docs/README.md](docs/README.md) for the layout rule, or jump to [docs/ideas/README.md](docs/ideas/README.md) for the idea index.

## Name

score: a joint that interweaves two ends into one continuous thing — the half-twist that makes a ribbon one-sided, and the operation that inlines a subgraph into its host. The Möbius strip is the image, but its name is a person's; the joint is the half that has a common name. On disk everything is ASCII: `score`, `script.score`.