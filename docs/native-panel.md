# macOS 原生历史面板

`crates/app` 是独立的 GPUI 应用入口，生成 `EcoPasteNeo`。现有 Tauri 应用仍通过 `src-tauri/Cargo.toml` 构建。当前原生面板提供 SQLite 历史列表、搜索、分页和键盘选择；系统捕获与写回粘贴尚未接入。Windows 原生面板暂缓，旧 Windows 实现保留。

## 构建与运行

```bash
cargo build -p ecopaste-neo-app --bin EcoPasteNeo --locked
cargo run -p ecopaste-neo-app --bin EcoPasteNeo -- --show
./scripts/package-neo-macos.sh
```

本地脚本输出 `dist/EcoPasteNeo.app`，使用临时签名，采用 core 中的正式产品名和应用标识。该脚本构建 debug 版本，默认使用 `dev` 数据目录，不是发布或更新流程。

点击菜单栏的 **Neo** 可显示历史或退出。全局快捷键为 **⌃⌥⌘V**，与独立原型的 **⌃⌥⌘P** 不同。注册被占用的快捷键会报错并退出，不自动换键。面板显示时将焦点放在搜索框；输入法组字期间，方向键和 Esc 交给输入法，组字结束后 ↑↓ 选择记录、Esc 隐藏。点击其他应用也会隐藏面板。

可选启动参数：

| 参数 | 作用 |
|---|---|
| `--show` | 启动时显示面板，默认隐藏 |
| `--data-parent /absolute/path` | 指定不含应用标识的平台数据父目录，仍经过 `Storage` 的 Neo 身份校验 |
| `--environment dev` / `prod` | 明确选择数据环境；默认按 debug/release 构建区分 |
| `--locale zh-CN` / `en-US` | 界面语言，默认简体中文 |

macOS 默认数据父目录是 `~/Library/Application Support`。`ECOPASTENEO_DATA_PARENT` 提供与 `--data-parent` 相同的启动默认值，显式参数优先；`ECOPASTENEO_SHOW=1` 等同于默认启用 `--show`，用于由 Launch Services 启动的测试包。

## 隔离的 1000 条测试记录

先创建全新的临时目录，再显式生成测试库：

```bash
fixture_parent="$(mktemp -d /private/tmp/ecopaste-panel-fixture.XXXXXX)"
cargo run -p ecopaste-neo-app --example seed_history --locked -- \
  --data-parent "$fixture_parent" --environment dev
./scripts/package-neo-macos.sh --no-build --test-data-parent "$fixture_parent"
```

测试包仍叫 `EcoPasteNeo.app`，身份仍是 `io.github.cleaf-y.ecopaste-neo`。`--test-data-parent` 将临时父目录和启动显示选项写入此包的 `Info.plist/LSEnvironment`，因此从访达打开、退出重开均使用同一库。不要与普通包同时运行；重新执行不带 `--test-data-parent` 的打包命令会生成普通包。

夹具包含 700 条文本、200 条图片和 100 组文件，文本有不同行数，图片有横竖两种比例，文件包括存在、目录和已丢失三种情况。`alpha` 和 `beta` 各匹配 500 条，`needle0042` 匹配 1 条，`no-such-record` 匹配 0 条；清空搜索匹配 1000 条。每页 50 条，滚到已加载内容末尾会自动取下一页，也可点击“加载更多”。

应用启动只读取数据库，不自动造数据。生成命令要求显式指定父目录和 `dev` 环境；已有记录时拒绝重复填充，避免把“重开持久化”误测为“再次生成数据”。夹具及界面测试不读取系统剪贴板，也不发送粘贴按键。

## 查询与依赖维护

`HistorySource` 打开一个经过身份校验的 Neo 数据目录，持有本会话的数据库池和资源路径。SQL 查询使用 core 的 `query_items_page`，数据库和文件预处理在 worker 执行。关键词变化清空已加载页并重置选中项；查询代次（每次搜索变化递增的编号）与页偏移不匹配的响应被丢弃，包括旧查询的错误。下一页只接受连续偏移，同一时刻只发起一页请求。

GPUI `list` 只绘制可见项并记录实测高度；追加页用 `splice`，换词用 `reset`。文本使用数据库摘要，图片使用 core 生成的最长边 300 像素缩略图，文件只准备前三项展示数据。资源不存在时保留记录并显示不可用状态。列表页数依据 core 返回的 `total` 和 `has_more`，不靠页大小猜测。

GPUI 三个包固定使用原型验证过的 `gpui-component 0.10.0`、`gpui-pre 0.3.12` 和 `gpui-pre-platform 0.3.12`。本机使用运行时 Metal 着色器编译。macOS 依赖的两处修补、上游提交、许可证与升级复验要求见 [`vendor/gpui-pre-macos/PATCHES.md`](../vendor/gpui-pre-macos/PATCHES.md)。

```bash
cargo test -p ecopaste-neo-core --all-features --locked
cargo test -p ecopaste-neo-app --lib --locked
cargo clippy -p ecopaste-neo-app -p ecopaste-neo-core --all-targets --all-features --locked -- -D warnings
cargo fmt --all --check
```

这些检查覆盖查询隔离、分页、重开与资源处理，不能替代实际 `.app` 的滚动、键盘、系统拼音和目标应用焦点验证。
