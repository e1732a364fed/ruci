本地构建：

    mdbook build

学习 mdbook:
https://rust-lang.github.io/mdBook/guide/creating.html
https://github.com/rust-lang/mdBook/wiki/Automated-Deployment%3A-GitHub-Actions

2024.12.20

发现生成的目录是有问题的，只有在主页时可用，在其它页面时，
因为相对路径的关系，变得不可访问

关联issue
https://github.com/rust-lang/mdBook/issues/2060

因此目前使用的 mdbook 是 HU90m 的 补丁版：
cargo install --git https://github.com/HU90m/mdBook.git --branch landing-page-links-fix mdbook

