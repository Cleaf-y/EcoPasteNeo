# macOS 原生历史面板

`crates/app` 是独立的 GPUI 应用入口，生成 `EcoPasteNeo`。现有 Tauri 应用仍通过 `src-tauri/Cargo.toml` 构建。当前原生面板提供 SQLite 历史列表、搜索、分页、后台剪贴板捕获、记录预览、右键管理和所选记录写回。macOS 的运行与验证步骤见下文。Windows 原生面板暂缓，旧 Windows 实现保留。

## 构建与运行

```bash
cargo build -p ecopaste-neo-app --bin EcoPasteNeo --locked
cargo run -p ecopaste-neo-app --bin EcoPasteNeo -- --show
./scripts/package-neo-macos.sh
```

打包脚本支持标准 `CARGO_TARGET_DIR` 环境变量，默认使用仓库 `target/`。本地脚本输出 `dist/EcoPasteNeo.app`，使用临时签名，采用 core 中的正式产品名和应用标识。该脚本构建 debug 版本，默认使用 `dev` 数据目录，不是发布或更新流程。

点击菜单栏的 **Neo** 可显示历史或退出。全局快捷键为 **⌃⌥⌘V**，与独立原型的 **⌃⌥⌘P** 不同。注册被占用的快捷键会报错并退出，不自动换键。面板显示时将焦点放在搜索框；输入法组字期间，方向键和 Esc 交给输入法，组字结束后 ↑↓ 选择记录、Esc 隐藏。非固定时点击其他应用会隐藏面板。Enter 或点击记录粘贴；Shift+Enter 和“纯文本粘贴”按既有规则去掉富文本格式，文件记录转为换行分隔的路径。面板中的“固定窗口”仅作用于当前会话。

可选启动参数：

| 参数 | 作用 |
|---|---|
| `--no-capture` | 禁用系统监听，用于只读列表测试；默认启动监听，不读取旧 EcoPaste 数据 |
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

应用启动不自动造数据；默认启动系统监听并将后续复制写入所选 Neo 数据库。只验证列表时加 `--no-capture`。生成命令要求显式指定父目录和 `dev` 环境；已有记录时拒绝重复填充，避免把“重开持久化”误测为“再次生成数据”。夹具生成命令不读取系统剪贴板，不发送按键。运行应用时，粘贴只由用户的 Enter、记录点击或粘贴按钮触发；系统监听默认启用。

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

## 记录预览

选择记录后按 **⌘P**，或点击卡片上的“预览”打开完整内容。预览打开时，主面板的 ↑↓ 选择会同步切换预览；预览窗口内的 ↑↓ 也可以切换记录。Esc 先关闭预览，再按一次才隐藏主面板。预览的“关闭预览”按钮只返回主面板，不重新保存粘贴目标。原有记录点击粘贴和 Enter 粘贴保持可用。

列表仍只查询摘要，预览经 `HistorySource::preview` 按 ID 在 worker 查询全文并准备资源。预览请求有独立编号；切换记录、关闭窗口、搜索变化和主面板隐藏都会让旧结果失效。敏感记录只返回遮盖状态，不能通过预览读取原文。HTML/RTF 显示系统捕获时保存的纯文本；没有纯文本的旧记录显示空内容，不把标记当正文显示。

长文本使用变高虚拟列表，按段滚动到末尾。图片使用经过 Neo 图片目录校验的原图，并显示原始像素尺寸；原图丢失时显示不可用，即使缩略图仍存在也不替代原图。文件列表展示全部引用，每次打开重新读取文件信息，保留已丢失项。预览作为非激活子窗口随主面板隐藏，不改变原外部应用的粘贴目标。

隔离夹具可用以下命令生成。测试包关闭系统监听，仍允许验证者主动触发粘贴：

```bash
fixture_parent="$(mktemp -d /private/tmp/ecopaste-preview07.XXXXXX)"
cargo run -p ecopaste-neo-app --example seed_preview --locked -- \
  --data-parent "$fixture_parent" --environment dev
./scripts/package-neo-macos.sh --test-data-parent "$fixture_parent" --test-no-capture
```

7 条夹具分别可用 `preview07-long`、`preview07-short`、`preview07-html`、`preview07-rtf`、`preview07-image`、`preview07-files`、`preview07-sensitive` 搜索。长文本末尾为 `PREVIEW07-LONG-END-7F3A`，完整对照文件保存在测试库的 `state/preview07/long-expected.txt`；原图为 4096×2048。`preview07-switch` 只匹配大图和短文本，用于快速切换后立即关闭的检查。文件夹具包含 `move-me.txt`，移到别处后重新打开预览应显示不可用。

`--test-no-capture` 必须与 `--test-data-parent` 同用，将 `ECOPASTENEO_NO_CAPTURE=1` 写入该测试包的启动环境。正常包默认仍开启监听；这个选项用于预览验证，不代替任务 05 的捕获验收。每次生成夹具都要求空库，不覆盖已有记录。

## 右键菜单与记录管理

右击卡片可收藏或取消收藏、置顶或取消置顶、编辑或清空备注、删除单条记录。菜单绑定右击卡片的 ID，与键盘选中项独立；列表刷新和置顶重排不会更换操作目标。卡片标题显示收藏和置顶状态。操作完成后按当前关键词重查列表，条数和排序与 SQLite 一致。

编辑备注和删除确认在主面板内进行。备注输入时 Enter 保存、Esc 取消，输入法组字期间仍由输入框处理按键；这些按键不会触发列表粘贴。删除需要再次确认，取消不写库。关闭菜单、取消或完成编辑后恢复列表输入焦点，保留唤出时的外部粘贴目标。管理操作会关闭旧预览；删除文件记录保留原文件，删除图片记录清理其 Neo 原图和缩略图。收藏、置顶和备注不刷新最近使用时间。

使用全新的临时库验证，避免删除已有历史：

```bash
fixture_parent="$(mktemp -d /private/tmp/ecopaste-menu08.XXXXXX)"
cargo run -p ecopaste-neo-app --example seed_management --locked -- \
  --data-parent "$fixture_parent" --environment dev
./scripts/package-neo-macos.sh --test-data-parent "$fixture_parent" --test-no-capture
```

夹具仅含 5 条：`menu08-pair` 匹配 A、B 两条文本；`menu08-delete` 为专用删除文本；`menu08-files` 引用 `state/menu08/keep-original.txt`；`menu08-image` 为专用图片。生成器拒绝覆盖非空数据库。测试包退出重开仍使用同一库，不重新生成夹具。

先用方向键选 A、右击 B，逐项修改并核对 B；给 B 写入独有备注后搜索，再清空备注核对旧词不再匹配。分别取消和确认专用记录的删除，核对列表、条数、预览和原文件。最后从外部专用文稿唤出，关闭菜单或备注编辑后按 Enter，核对仍粘贴到原光标处。真实粘贴由验证者主动触发。

## 文件和图片拖出

在文件或图片卡片上按住左键，移动超过 6pt 后拖到 Finder 目标文件夹的空白处。文件引用在按下时由 worker 重新读取，只交付仍存在的路径；全部丢失时显示原因。图片交付 Neo 保存的原 PNG，原图缺失或不可用时拒绝拖出，即使卡片仍有缩略图。文本拖出和 Windows 原生拖放不在当前实现范围内。

原生会话只允许复制，修饰键也不能把 Neo 原图移走。拖动预览使用通用文件图标，起拖时不解码大图。Esc 取消或拖动失败后，同一次鼠标操作不会变成点击粘贴；下一次单击和右击仍分别粘贴或打开菜单。拖动不会通过系统通用剪贴板或模拟粘贴传输文件，也不根据原生结束事件显示成功提示，需核对 Finder 实收。

隔离夹具包含单文件、多文件、4096×2048 原图、全部丢失的文件、仅有缩略图的缺失原图，以及用于点击粘贴的文本：

```bash
fixture_parent="$(mktemp -d /private/tmp/ecopaste-drag09.XXXXXX)"
cargo run -p ecopaste-neo-app --example seed_drag --locked -- \
  --data-parent "$fixture_parent" --environment dev
./scripts/package-neo-macos.sh --test-data-parent "$fixture_parent" --test-no-capture
```

搜索词分别为 `drag09-single`、`drag09-multi`、`drag09-original`、`drag09-missing`、`drag09-image-missing` 和 `drag09-paste`。生成命令打印源文件与空 Finder 目标文件夹路径，均在该测试库的 `state/drag09/` 下。先检查单文件、多文件与 PNG 实收，再将多文件中的源文件依次移至 `moved/`，重新拖动并核对仅交付剩余文件或明确提示全部丢失。使用空的 `targets/cancel/` 检查 Esc 取消，最后在外部临时文稿核对没有误粘贴，并检查后续点击和右键。

## 粘贴与捕获验收

先保存需要保留的剪贴板内容，退出其他正在运行的 Neo 测试包，再从独立测试目录启动。默认捕获设置沿用 core 的类型顺序、来源排除和敏感内容规则；托盘“设置”可修改采集总开关和五种内容类型，不导入旧 Tauri 设置。敏感文本在卡片中隐藏，写回仍读取完整原文。

写回通过 ID 查询完整内容，文件读取及准备在 worker 执行。macOS 先检查辅助功能权限；未授权时面板显示当前 `.app` 的完整路径，不修改剪贴板、不发送按键。授权系统设置中这个路径对应的 `EcoPasteNeo.app`，不是 Codex、终端或独立原型。授权后重试；系统若要求重启则重开同一测试包。

本次 macOS 27.2.1 实测中，该权限列表位于“隐私与安全 → 设备控制和数据访问”。临时签名测试包重建后若提示未授权，重新核对当前路径的授权状态；若开关已开仍提示未授权，可先移除旧条目，再添加当前测试包并重开。本次直接向已有条目重复添加未生效，完整移除后重新添加才恢复粘贴。原生文件拖动不需要模拟粘贴所用的辅助功能权限。

非固定时写回后隐藏并延迟发送 `⌘V`；固定时保持显示、暂时交出键盘焦点，事件发出后恢复 key window，不激活整个应用。唤出时保存外部前台进程，准备完成及发送前再次核对；目标改变则取消本次发送。按键发送成功不能证明目标光标处已经收到内容，必须在专用文稿和浏览器输入框核对实际结果。

用 `--no-capture` 运行的检查不能作为任务 05 捕获验收。真实捕获验收要求在外部复制、搜索命中、退出重启仍可搜到、面板隐藏时复制仍入库、图片缩略图重开可读；粘贴验收还要核对普通和纯文本格式、非固定键盘/鼠标触发、固定连续两条、写回无重复入库且随后不同的外部复制能入库。全屏、多屏、系统拼音、失焦隐藏、托盘退出和快捷键重启需在本次正式 `.app` 复测。


## 设置窗口与采集开关

通过菜单栏 **Neo → 设置** 打开普通、可获得键盘焦点的 GPUI 窗口。重复打开会激活已有窗口；关闭窗口不退出应用、不停止监听。历史窗口继续使用非激活 NSPanel，不与设置窗口共享窗口类型或粘贴目标。

设置只使用 `Storage::open` 返回的 `DataPaths::settings_path()`，即当前 Neo 环境的 `config/settings.json`。首次启动先原子保存默认值，再允许监听器采集；已有合法文件启动时只读，不重写。JSON 保留 `Capture` 的大小限制、类型顺序、`Sensitive`、`Filters` 及未知字段。开关操作只提交一个字段，在串行写入锁内合并到最新设置，经校验、同目录唯一临时文件写入、同步和原子替换成功后才发布新快照。窗口不提前采用未保存值。

采集总开关默认开启；关闭不改变纯文本、HTML、RTF、图片、文件五个类型的选择，也不影响已有记录查询和粘贴。监听器每次收到通知读取内存中的最新成功快照；暂停时返回 `None`，不读取剪贴板，不重建监听器、不逐次读配置文件。多表示载荷沿用 core 的优先级与回退规则。`--no-capture` 仍禁止本次启动创建监听器，设置开关不能绕过它。

设置中的“排除应用”通过“添加应用…”选择 `.app`，列表显示其 bundle identifier，重复选择同一应用不会增加条目。每项可以移除，也可以清空列表；清空后重启保持为空，不重新补回默认的“钥匙串访问”和“密码”。保存成功后，下一次从匹配来源复制时会在读取内容前跳过；无法识别来源时沿用原有采集处理。规则不按显示名称、窗口标题或域名匹配，也不清理该应用过去的历史。

保存失败时窗口显示原因和设置文件路径，窗口与监听器继续使用成功保存过的值。启动时文件无法读取或解析，则暂停采集并自动打开设置窗口；开关不可用，原文件不被默认值覆盖。修复文件后点击“重试”重新加载，成功后才恢复该文件允许的行为。没有自动修复、配置导入或数据目录热切换。

实测使用全新的临时父目录打包，保持监听开启：

```bash
fixture_parent="$(mktemp -d /private/tmp/ecopaste-settings10.XXXXXX)"
./scripts/package-neo-macos.sh --test-data-parent "$fixture_parent"
```

先检查托盘重复打开、关闭重开和键盘操作；再以不同专用标记测试“采集 A → 暂停复制 B → 恢复复制 C”，核对 A/C 存在、B 不存在且不补录。关闭图片采集、保留纯文本，用专用图片和文字核对类型；总开关关闭后退出重开，确认总开关及图片选择保留。仅在该临时环境测试目录不可写和损坏 JSON，测试后恢复文件与权限。自动测试构造载荷，不操作系统剪贴板。
