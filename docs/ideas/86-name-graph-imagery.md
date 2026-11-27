# 定名：splice

**状态**：在用（可改）

**一句话**：语言叫 splice——把两端交织成一条连续的东西的接合；莫比乌斯环的半扭是它，子图内联也是它。

**动机**：名字决定读的人第一眼把它归到哪一类。ψ 的意象是「场里的振幅」，指旧语义（表达式求值）；语义换成图之后，名字要指同一件事。莫比乌斯环的意象合得上，但那个词是人名；去掉人名，英文里没有指这个对象的通用名词，只有指它两部分的词：材料（band、ribbon、strip、loop）与接合（twist、seam、splice、suture）。这门语言要的是接合那一半：折叠之后内外是同一个面，内联就是把一段接到另一段上。

**形态**：

判定次序：意象 → 同域撞名 → 落盘 ASCII；意象优先于流畅，撞名优先于可打。

撞名结论（每个名字一轮检索，语言域加软件域；crates.io、npm、商标未逐个扫）：

- **band**：字面上是「Möbius band」去掉人名的通用名词，语言域空；太通用（乐队、频带），当不了名字。
- **ribbon**：RibbonLang 已存在。
- **seam**：有 seam-rs crate，另有 "Seam: provably safe local edits on graphs"（同域的图编辑）。
- **splice**：语言域空；Perl 的 splice 关键字、Rascal 的 list splice、Unix splice(2) 在别处用这个词。
- **suture**：撞 Sutra（一门语言，差一个字母），另有 Go 的 suture 库。
- **torsion**：语言域空，但医学里的扭转是急诊词。
- **coil**：coil-lang.dev 已是脚本语言。
- **fold**：CS 里 fold = reduce，名字会被读成归约。
- **loop / strip / ring**：通用词，当不了名字。

选 splice：它命名的是动作——把两端接成一条连续的东西。莫比乌斯环的半扭是接合，子图内联也是接合；语言域空；没有坏联想（基因剪接、绳结拼接、胶片接片都中性或正面）。

落盘：目录与仓库 `splice`，扩展名 `.splice`，命令行 `splice run x.splice`。

**代价**：

- 读起来是动作不是物件，与 aura、gravity 这类名词不完全同类。
- 搜索会与 splice(2)、Perl 关键字混。
- 六个字母，比 weft 长；`splice run` 有两个 s 音连读。

**冲突**：[72](72-name-psi.md)（旧名 ψ，保留为「继续叫 ψ」这个反向做法的记录）。

**若改掉会动到**：README、仓库名、扩展名、[72](72-name-psi.md)。