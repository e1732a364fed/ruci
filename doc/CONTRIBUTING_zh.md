# 理念

代码理念是极简，也是本项目名字rucimple的含义 (ruci 和 rucimp 都是 rucimple 的简称, 意为 如此简单)

奥卡姆剃刀原理：若非必要，勿增实体

稳扎稳打，力求写出高质量的代码。我们将严格按test driven development 进行开发

对版本迭代提出高要求。每提升一个版本号前，都会确保单元测试和文档的完整和更新。
commit 谨慎，commit 的说明详细。每个commit 要完整地解释 该commit的目的和作用，
以及任何开发者对该commit的想法

文档所用语言：中英混用。我们的代码文档面向汉语母语的程序员，因此要求中英语都能用，灵活替换

尽量多写注释，尤其要把可能发生的但直观上不容易查觉的情况写情楚

## 为什么用TDD

测试是对可靠性最好的检验和证明方法。一个东西要想科学，就要有PoC。

# 约定

only use Box::leak if have to.

灵活使用迭代器

不要过多使用元组结构

## 子项目

ruci(定义) - rucimp（实现） - ruci-cmd（可执行文件）

分三个子项目的好处之一是可以有三个不同的版本号。
比如，对于 同一个版本的 rucimp, 可以在其上不断更新可执行文件 ruci-cmd 的版本。
这样版本号能更好地反映哪里产生的修改。

ruci-cmd 在 git commit 中 简称 rcc

## 减少 unwrap, todo!, unimplemented! 数量


## 异步架构

为了避免有人说e抄袭，在项目初期e选用了 async_std。
后来才创建的 tokio 分支，可查看commit历史 求证。
不过难以维护两套异步架构，现在async_std分支只能作为参考了。
