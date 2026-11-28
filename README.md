# score

A graph-first language: the language is a binary graph, and text is one source that generates it. **Status: design draft with a first vertical slice** — the parser, the validator and a single-threaded worklist runtime live under `crates/`; the order is in [docs/PLAN.md](docs/PLAN.md).

Design: [docs/ideas/](docs/ideas/README.md) — one document per idea; retired ones live under docs/ideas/retired/ (written in Chinese). The pre-split long draft stays at [docs/design.md](docs/design.md) as history.

Positioning: [docs/ideas/94-value-proposition.md](docs/ideas/94-value-proposition.md) — code generation is cheap now, so the cost that decides is the cost of a wrong step; a statically checked, resumable, parallel plan is the layer that bash and Python glue do badly.

## Running the slice

```
cargo run -p score-cli -- validate examples/sum.score
cargo run -p score-cli -- run examples/sum.score --trace
cargo run -p score-cli -- run examples/sum.score --journal /tmp/run.jsonl
cargo run -p score-cli -- run examples/sum.score --resume /tmp/run.jsonl --journal /tmp/run2.jsonl
```

The journal is one JSON line per firing (`seq`, `kind`, `node`, `op`, `port`, `value`), under a header carrying the graph's fingerprint. `--resume` seeds a run from such a file: the values are re-delivered along the edges and the graph continues from the first node with no entry. A journal from a different graph is refused outright, so an edited program cannot be resumed against stale values.

A ring's progress is not derivable from its firings, so a back edge writes a `kind: "budget"` line whenever it spends budget. Resuming therefore continues from the budget the journal had left instead of handing the ring a fresh one. Every node that fired is seeded, and a seeded node does not fire again, so a resumed ring stays where it stopped rather than turning afresh.

`graph` prints the parsed graph back as text: the declarations, then every node with its ports and arguments, then every edge with its guard and budget — what validation sees, after `def` calls have been inlined.

Two passes, neither optional: parsing checks the text against the declarations it has read (does this def have that parameter, does this variant carry that field, do the match arms agree, does the argument's type fit the port — a field read included), and validation checks the assembled graph (every port exists, one edge per input, a `[*]` arm for every conditional branch, no ring without a budget, and the type that travels along each edge). `score` runs both before doing anything, so "checked" means runnable. There is no type inference: every type is either declared by the program or recorded while reading it, and `types::known_ty` only looks the answer up.

Implemented today: the v0 surface (bindings, `<in|out>` ports, `->` / `-->` / `-[cond]->`, `const`, `def` with inlining), validation (ports, types, one edge per input, a `[*]` arm for every conditional branch, no ring without a budget), list and record values, and a worklist runtime with twenty-seven in-process operators (`lit`, `add`, `concat`, `upper`, `len`, `print`, `range`, `count`, `sum`, `join`, `at`, `map`, `fold`, `filter`, `any`, `all`, `gt`, `eq`, `to_text`, `to_int`, `record`, `get`, `put`, `keys`, `has`, `tag`), plus `match` for sums over def bodies.

A record is the dynamic side of data: an object whose shape the checker does not know, the way `serde_json`'s object is dynamic. It is built from its arguments (`r = record(name: "ada", age: 36)`), and its static type is `Record` with no fields — deliberately, so `get` on it always yields Any. Its keys may be computed values, not only names written in the source: `put(r, key, value)` returns a new object with that key set, `keys` lists them in order, `has` asks whether one is there. `get` also reads a tagged value's field, and `tag` returns its variant name — the unchecked way to look inside a sum, next to `match`.

`to_text` renders any value the way `print` does, and `to_int` reads an integer back out of a text — trimmed, and an error naming the text when it cannot. Without them nothing could cross between the two halves of the value world.

A struct is the static side: a declared shape with typed fields, nominal (`Person` is not any other record). Reading a field off it whose key is written out carries that field's type, and a key it does not declare is an error: `` `x` reads `nmae`, which `Person` does not have ``. The dynamic operators refuse it — `put(r: p, …)` on a `Person` is a type error — so a dictionary cannot quietly eat a structure.

Types are declared with braces, like the rest of the language: `enum Shape { Circle { r: Int } Rect { w: Int, h: Int } }` and `struct Person { name: Text, age: Int }`. A variant name is also the port its value leaves on, so `c = Circle(r: 3)` is read at `c`'s `Circle` port; a struct name is its own constructor and yields a record.

A declared type can be named on a port: `def describe(s: Shape) -> (out: Text)`. A name the program does not declare is rejected, since the parser cannot tell a typo from a type. A list names what it holds — `List<Int>`, or `List<Any>` when nobody says — and the element is checked: `range` produces `List<Int>`, so passing it where `List<Text>` is wanted is refused. `Record` still carries no field types.

Sums are consumed by `match`, which names one def per variant: `m = match(on: s, Circle: circle, Rect: rect)`. The arm runs with the payload's fields seeded into its parameters by name, so an arm's parameters must be the variant's fields, in order. Every variant has to be named — a missing arm is a validation error, and so is an arm that is not a variant of that sum. The value matched on must come from a constructor (directly or through a def), which is what tells validation which sum the arms have to cover. The arms must also agree on the type they produce, and that type becomes the node's, so an edge out of a `match` is checked like any other.

Attributes are compile-time tags written `@name` or `@name(value)` in front of an element — on their own line above it, or inline before it — and today only `@doc("…")` and `@tag(x)` exist. An attribute never becomes a value and never changes what a run does, so it stays out of the graph fingerprint; an unknown name is a parse error rather than a word that is silently ignored, and an attribute with nothing after it is an error too.

`map` and `fold` take a def as their body: the def's graph is cloned once as a template whose param nodes the runtime seeds, and it runs once per element (`fold` threads the accumulator through the same body). `filter`, `any` and `all` take the same template and read a Bool from it.

A def is either called with arguments (`m = double(x: n)`) or entered from a chain (`n -> double -> print`): a single-parameter def takes the chained value by default, a wider one names the entry port with `<param|>`. Chaining and an argument for the same parameter is an error.

Rings are graph cycles with a budget: `acc -[budget: 5]-> acc<a|>` carries the value back into its own input at most five times, and a plain edge into the same port supplies the first value. The runtime fires a node again only when one of its input stamps moved, so an ordinary program behaves as before while a ring turns until its budget runs out. A cycle without a budget is rejected by validation.

Not implemented yet: exhaustiveness over a sum — a value carries its tag, but validate does not yet enumerate a sum's arms and require each to be wired or explicitly ignored; okm sources (the okm query layer is an idea parked for later). Neither `List` nor `Record` carries its member types: they stand for "a list of anything" and "a record of anything" until `Ty` becomes a recursive type.

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