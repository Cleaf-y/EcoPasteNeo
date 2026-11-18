# 开发注意事项

本文记录修改 EcoPaste 时容易遗漏的现有行为和原因。通用编码规则以根目录 `AGENTS.md` 为准；修改相关实现时同步更新本文，不维护完整命令、字段或默认值清单。

## 剪贴板读取与写回

手动读取与监听捕获共用 `build_item_with_settings`、`persist_and_notify`，避免两条路径在去重、来源应用、提示音和刷新事件上产生差异。原生剪贴板句柄不要跨越 `await`；先同步读取或写回，再把可跨线程的数据交给异步入库逻辑。

Windows 剪贴板变更通知可能早于其他程序释放剪贴板。监听器只对读取错误做有界重试；成功读到内容和成功返回空内容都立即结束。耗尽重试后只记录最终错误，不逐次刷日志。不要在普通自动测试中直接操作用户的系统剪贴板。

写回历史记录前调用 `WritebackGuard::suppress`，监听器重新计算内容哈希后通过 `should_skip` 判断是否跳过。抑制依据是内容哈希和短期有效期，不是全局布尔开关，否则会吞掉期间发生的其他复制。修改写回格式时，写入端与读取端必须得到相同的哈希。

文本的 `content` 保存所选 HTML、RTF 或纯文本表示，`search_text` 保存系统提供的纯文本，供搜索和纯文本粘贴使用。图片内容保存资源文件名，文件内容保存换行分隔的路径。敏感信息识别、脱敏、文件条目、颜色预览和可用操作由 Rust 准备，前端不要重新推导。

来源应用在读取剪贴板前捕获，避免异步入库时前台应用已经改变。macOS 用 bundle id，Windows 用可执行文件路径标识来源；应用信息入库失败时可以去掉来源，但不能丢弃剪贴板内容。

文本子类型识别与颜色预览校验共用 `crates/core` 的 `content` 模块，判定顺序为 URL、邮箱、颜色、本机存在的绝对路径。`ClipboardSubKind` 也由该模块定义，JSON 与 SQLite 值保持小写；旧应用通过 core 的 `sqlx` feature 启用数据库适配，不再单独维护识别正则或子类型枚举。

捕获载荷、`Capture` / `CaptureKind` / `Sensitive` / `Filters` 设置、敏感检测、图片原图和缩略图实现统一在 [`crates/core/src/clipboard/`](../crates/core/src/clipboard/)。默认 `capture` feature 提供纯载荷转换与持久化，`system-clipboard` feature 才引入原生读取器、来源探测和监听入口；测试构造载荷，不操作系统剪贴板。设置字段、默认值、优先级及敏感内容的“收录与标记、显示时脱敏”语义保持原样。

Neo 使用 `ImageStore::new(&DataPaths)`，原图和懒生成的缩略图都位于已确认身份的 Neo 资源目录。旧 Tauri 的 `ImageStore` 只适配原数据目录与热切换，实际图片操作委托 core，不把旧目录切换为 Neo 目录。`db::items::upsert_item` 在同一写事务中判重并写入，避免手动读取与监听同时到达时重复插入；数据库结构及已发布迁移不变。

正式面板通过 `HistorySource::start_capture` 接入监听，保留 `native::start_watcher` 返回的 `WatcherHandle`，退出时释放以停止监听。监听器在自己的线程创建原生句柄，先记录来源并检查排除应用，再同步读取、应用当前 `CaptureSettings` 和 `WritebackGuard`，只向接收方投递拥有所有权的 `PreparedCapture`；设置回调返回 `None` 时暂停读取。接收方排队处理，使用当前应用会话的数据库池调用 `persist_capture`，成功得到 `CaptureUpdate` 后再通知面板重新查询；不要在监听回调中等待数据库或缓存旧池。手动读取使用 `native::read_capture` 和同一个 `persist_capture`。core 不调用 `AppHandle` 或 `emit`；旧 Tauri 的 `persist_and_notify` 委托同一持久化实现，随后沿用原提示音和 `clipboard://updated` 通知。

写回格式统一在 `crates/core/src/clipboard/write.rs`。`PreparedWrite::new` 只准备载荷和指纹，不碰系统剪贴板；`write` 在同步段登记指纹后一次写入全部格式。HTML/RTF 同时带纯文本回退，纯文本模式只写 Text；macOS 图片直接写原 PNG，避免重编码改变捕获指纹。旧 Tauri 的 writer 仅做错误适配。

Neo 面板先检查当前进程的辅助功能权限，再读取完整记录并写回。准备结果返回时及发送键盘事件前，再次检查权限和目标进程。非固定隐藏后发送；固定窗口交出 key 状态后发送，再恢复 key，不激活应用。不要用 CGEvent 已投递推断目标收到内容。捕获队列与持久化消费者由面板会话持有，退出先停止监听，再等待队列中已接收的内容落库。`--no-capture` 仅供只读 UI 验证，不能替代真实捕获验收。

实现入口：[内容识别](../crates/core/src/content.rs)、[写回抑制规则](../crates/core/src/writeback.rs)、[监听器](../src-tauri/src/clipboard/watcher.rs)、[写回](../src-tauri/src/clipboard/write.rs)、[命令](../src-tauri/src/commands/clipboard.rs)。从仓库根目录运行 `cargo test -p ecopaste-neo-core` 即可验证内容识别与写回抑制，无需 Tauri、GPUI 或系统剪贴板。现有桌面应用仍用 `src-tauri/Cargo.toml` 和原锁文件构建。

## 数据库、资源目录与备份

`DatabaseState` 管理当前连接池。命令通过 `db.pool().await` 获取当前池，仓储接收 `&SqlitePool`；不要长期保存可能已被替换的旧池。覆盖导入备份和切换数据目录需要通过 `DatabaseState::close_and_replace` 关闭并替换连接池。

持久化根目录统一经 `core::paths` 解析。开发和发布数据分别位于 `dev/`、`prod/`；自定义根目录由启动配置 `storage.json` 与目录内的 `.ecopaste-storage.json` 标识共同确定。功能模块不要自行解析系统应用目录，绕过自定义目录设置。

Neo 核心的正式身份统一在 [`crates/core/src/identity.rs`](../crates/core/src/identity.rs)：产品名 `EcoPasteNeo`、应用标识 `io.github.cleaf-y.ecopaste-neo`。新应用在打开 SQLite 或设置前调用 [`Storage::open`](../crates/core/src/paths.rs)，使用返回的 `DataPaths`；调用方传入不含应用标识的平台本地数据父目录（macOS 的 Application Support、Windows 的 LocalAppData），并明确选择 `Environment::Dev` 或 `Environment::Prod`。默认根目录为 `<平台父目录>/io.github.cleaf-y.ecopaste-neo/<dev|prod>`，`open_custom(parent)` 使用 `<parent>/EcoPasteNeoData/<dev|prod>` 并在默认根目录的 `storage.json` 保存选择。

Neo 在创建数据库目录前验证 `.ecopasteneo-storage.json` 中的应用标识、格式版本和环境；只有新建/空目录可以创建标记。已有 Neo 标记原样保留。旧 `.ecopaste-storage.json` 不改名，不作为 Neo 身份；无 Neo 标记的非空目录、损坏或不匹配的元数据、已丢失的自定义目录均报错，不自动接管或回退到另一数据库。此模块不依赖 Tauri，旧 Tauri 配置和 `core::paths` 继续使用旧身份与目录规则。用临时目录运行 `cargo test -p ecopaste-neo-core --test storage_identity` 验证隔离，无需启动应用。

运行中切换目录会暂停剪贴板监听、复制已知数据目录、替换数据库池，并同步设置和资源路径，最后广播设置及剪贴板刷新。修改其中一环时检查其余状态，避免数据库已切换但图片、图标或设置仍指向旧目录。

历史模型、分页与详情查询统一在 [`crates/core/src/db/`](../crates/core/src/db/)，旧 Tauri 的 `db::models` 和只读仓储直接复用它们。新调用方先用 `Storage::open()` 确认 Neo 目录身份，再调用 `db::open(&paths)` 打开 SQLite；旧 Tauri 仍通过自己的 `db_path` 解析旧目录，并使用共享的 `db::open_at` 初始化连接池。core 默认启用 `sqlx` feature，测试不需要 Tauri 或 GPUI。

迁移 SQL 仅保留在根 [`migrations/`](../migrations/)，由 core 的 `db::MIGRATOR` 嵌入，两个应用入口和内存库测试共用同一份迁移。已发布的 `0001_init.sql` 仅移动位置，内容及校验和不变；不要在旧 `src-tauri/migrations/` 再维护副本。core 的 `build.rs` 跟踪迁移目录变动，使新增迁移能触发重新编译。

搜索和分页由 `db::items::query_items_page` 返回完整 `ClipboardItemPage`（`list`、`total`、`hasMore`），旧命令只补充展示数据。关键词任一空白分词少于 3 个字符时使用 LIKE，所有分词至少 3 个字符时使用 FTS5；搜索范围均为 `search_text` 和备注。置顶始终优先，然后按所选字段排序，最终按唯一 ID 倒序打破平局，保证静态数据集跨页排序稳定。调用方使用返回的 `total`、`hasMore`，不要根据页大小猜测是否还有内容。

文本列表查询不加载完整 `content` 或 `search_text`，只保留摘要；预览和写回通过 `find_item_by_id(&pool, id)` 按需获取原文。查询始终接收调用方当前的 `&SqlitePool`，不会缓存连接池或重新按路径打开数据库。运行 `cargo test -p ecopaste-neo-core --test history_query` 可验证实际迁移、筛选、搜索、摘要/详情、分页及 Neo 临时文件库关闭重开。

收藏、置顶、备注和单条删除统一调用 core 的 `db::items`，旧 Tauri 仓储仅适配应用错误。前三种操作不改 `updated_at`；备注变化由既有 FTS 触发器同步索引，不新增迁移。删除图片先提交删行，再由调用方使用同一 `ImageStore::remove` 清理原图与缩略图；清理失败要明确提示记录已删，不能让用户误以为删行失败。文件记录只删除数据库引用，绝不删除引用的用户文件。

实现入口：[连接池状态](../src-tauri/src/db/state.rs)、[路径解析](../src-tauri/src/core/paths.rs)、[目录切换](../src-tauri/src/commands/storage.rs)、[备份](../src-tauri/src/backup/mod.rs)。

## 设置写入与前端镜像

`SettingsStore::update` 只接受对象 patch：对象递归合并，数组整体替换，校验后通过临时文件和原子重命名持久化。缺失字段的 serde 默认值不能代替字段重命名、默认值变更等已发布配置的迁移策略。

组件调用 `updateSettings`，由 Rust 持久化并发送 `settings://updated`，各窗口再更新 `settingsState`。即使是发起修改的窗口也不直接乐观写入镜像；返回快照可用于后续操作，但渲染等待广播。设置已写入后的系统状态刷新失败通常只记日志、不回滚；直接操作系统状态的命令，如 `set_autostart`，按自身失败语义处理。

打开窗口时，范围、类别、分组分别应用。`preserve` 表示保留该维度，`all` 表示全部；自定义分组编码为 `group:<id>`，不能保存裸 ID。

新增偏好设置 `action` 时，补齐两种语言的 `title`、`description`、`controlLabel` 及 `settingVisual` 图标，否则界面会显示原始翻译键。

实现入口：[设置存储](../src-tauri/src/settings/store.rs)、[设置命令](../src-tauri/src/commands/settings.rs)、[前端镜像](../src/stores/settings.ts)。

## 窗口生命周期与消息投递

窗口显隐统一调用 `window::show_window`、`hide_window`、`toggle_window`，不要绕过几何保存、定位、生命周期和 `window://visibility` 通知。可销毁窗口重建后，前端等待 `settingsReady` 再调用 `notify_window_ready`。

偏好设置窗口可能尚未创建或正在重建，不能只向它发送一次事件。设置高亮通过 `open_preference_with_highlight` 暂存，再由 `take_pending_preference_highlight` 消费；收到备份文件同样通过 `emit_received_backup` 与 `take_pending_backup` 交接，避免监听器挂载前丢消息。

macOS 剪贴板窗口是 NSPanel：先注册插件再调用 `to_panel`，延迟显示在主线程执行，实际显示后再发送显隐和生命周期事件。固定窗口粘贴时先交出 key window 状态，让目标应用收到模拟按键，再恢复。修改这条路径需要实际检查自动隐藏、固定粘贴、键盘导航和 Spaces 行为。

实现入口：[窗口管理](../src-tauri/src/window/mod.rs)、[生命周期](../src-tauri/src/window/lifecycle/mod.rs)、[macOS 窗口](../src-tauri/src/window/macos.rs)。

## GPUI 原生历史面板

`crates/app` 与旧 Tauri 入口分别构建，当前只启用 macOS。启动先使用 core 的 `Storage` 验证 Neo 身份，再打开共享数据库；本地 `.app` 打包也从 core 读取产品名与应用标识。构建、隔离夹具和启动参数见[原生面板说明](./native-panel.md)。

历史查询在 Tokio worker 执行，完成后回到 GPUI 更新视图。关键词变更先递增查询代次并清空页；旧代次的成功与失败结果都不能更新界面。分页请求的偏移必须等于当前已加载条数，同一时刻只请求一页。GPUI 变高 `list` 在换词时 `reset`，追加时 `splice`，不能把已测量行的高度悄悄改变。

图片缩略图和文件信息由 core 的可选 `preview` feature 准备，UI 不同步读取文件。图片沿用 `resources/clipboard-images/{origin,thumbnails}/<hash前两位>/<hash>.png` 布局；仅接受哈希文件名，不接管外部路径。输入法组字期间让输入控件处理方向键与 Esc；不要用列表导航拦截选词。macOS 非激活面板、鼠标所在屏幕定位和候选窗层级必须在实际应用中复测，不能用编译结果代替。

预览通过 `HistorySource::preview` 按 ID 异步读取全文，展示数据由 core 的 `preview::record_content` 准备。敏感记录先返回隐藏状态，再按内容类型准备纯文本、原图和全部文件信息；不要在 UI 中从摘要或富文本标记推导正文。原图与缩略图共享哈希文件名和链接路径校验，预览不使用缩略图替代缺失原图。GPUI 图片元素使用按视口计算的绝对宽高并等比例缩放，容器限制绘制范围；不能只靠百分比尺寸或裁切掩盖图片越界。

预览的请求编号独立于列表分页；切换、关闭、搜索变化和主面板隐藏都使旧结果失效。异步完成只能更新当前打开记录，不能显示窗口。子窗口关闭和导航回调要延后到当前实体更新结束后，再调用主面板，避免重复借用同一 GPUI 实体。主面板与预览共享焦点会话；只在唤出主面板时保存外部目标，预览切换和关闭不得调用会重新保存目标的 `show`。

右键菜单在打开时保存记录 ID 与备注，后续菜单事件、备注提交和删除确认都使用该 ID，不能按当前选中项或行号重新取目标。原生 muda 菜单通过托盘共用的唯一接收器分发，实例保留至下次打开；主面板隐藏时撤销未处理事件。菜单跟踪会进入 AppKit 嵌套循环，必须在 GPUI 实体更新之外执行，菜单结束后再打开编辑或确认界面。备注编辑和删除确认复用主面板，进入管理状态时阻止列表粘贴；输入法组字仍交给输入框，关闭或完成后恢复搜索框焦点，不重新保存外部目标。每次写入完成按当前关键词重新查询，并关闭旧预览。

## Windows 输入、自启与管理员启动

剪贴板窗口默认不可聚焦，普通导航通过 Rust 键盘钩子进入 `useKeyboardEvent`。进入编辑态时临时允许聚焦并关闭导航钩子；退出编辑态后恢复，只有窗口仍在前台时才还原之前的前台 HWND。编辑态由剪贴板页面集中协调，单个输入框不要各自调用原生命令。

Ctrl+F 等程序主动聚焦路径先调用 `prepareClipboardWindowEditableFocus()`。Windows 剪贴板窗口中标记 `data-allow-global-keyboard="true"` 的输入框，把按键交回全局处理前必须先失焦；普通可编辑控件保留浏览器输入行为。Ctrl 交接造成窗口失焦时，快捷键提示应等待对应 keyup 才结束，不能提前消失。

正常自启只能有一个有效的 Windows Run 入口。新增入口写 HKCU，不因进程已提权而新增 HKLM。清理遗留 HKLM 失败时避免再创建 HKCU 副本；关闭自启若无法移除 HKLM，应返回失败，不能保存“已关闭”却仍会自启。带 `--auto-launch` 的二次启动不应弹出窗口，普通二次启动仍按用户主动打开处理。

`runAsAdmin` 保存用户意愿，实际是否提权取决于 Windows token。自动提权检查在窗口、托盘、数据库、剪贴板监听初始化之前执行，并遵循自定义数据目录。计划任务必须匹配当前可执行文件路径；`schtasks /Run` 不能传本次启动的动态参数，因此带备份文件路径或 `--auto-launch` 时使用 UAC `runas` 保留参数。只有新进程启动成功才退出旧进程；取消或失败则保留当前进程。管理员重启任务不作为第二条普通自启路径。

macOS 专用插件权限放在带 `platforms` 限定的 capability 中；公共 capability 只能使用两端都有效的权限。Rust 的 `cfg` 不能阻止 Windows 在构建阶段拒绝未知的 macOS 权限名。

实现入口：[Windows 窗口](../src-tauri/src/window/windows.rs)、[管理员启动](../src-tauri/src/admin.rs)、[Windows 自启](../src-tauri/src/autostart/windows.rs)、[应用启动](../src-tauri/src/lib.rs)、[权限配置](../src-tauri/capabilities/)。

## 列表、组件与订阅

`VirtuosoScroller` 将 OverlayScrollbars 提供的 `scrollerRef` 直接交给 Virtuoso，保持可视范围计算、程序滚动和键盘导航使用同一个滚动视口。卡片悬停切换备注与原文时保留被测量内容层，避免暂时卸载使 Virtuoso 记录零高度。

Ant Design 按钮接收自定义图标时使用 `CustomIconButton`。它通过 `classNames.icon` 恢复图标槽的居中样式，不增加额外 DOM 或全局覆盖。普通文字按钮和非按钮装饰图标不需要该包装。

长生命周期订阅通过 ref 调用最新回调，不为避免旧闭包而每次渲染重新订阅。列表隐藏或离开顶部时延后刷新，避免隐藏窗口 IPC 和滚动跳动。分页留在 `useClipboardItems` 中；`clipboardViewState` 的字段会映射为后端查询，新增与查询同名的 UI 字段可能意外成为过滤条件。

HTML、RTF 剪贴板预览当前以纯文本展示，不要仅为复用重新引入富文本 DOM 渲染器。原生菜单和拖出分别经 `popupClipboardItemMenu`、`startDragClipboardItem` 交给 Rust，拖出前阻止浏览器默认拖动。

实现入口：[滚动器](../src/components/VirtuosoScroller/)、[图标按钮](../src/components/CustomIconButton/)、[键盘事件](../src/hooks/useKeyboardEvent.ts)、[列表](../src/pages/Clipboard/components/List.tsx)、[视图状态](../src/stores/clipboardView.ts)。

## 更新与发布

更新检查、签名校验、安装、跳过版本和定时调度由 Rust 管理，React 只展示状态和发起命令。自动检查覆盖整个应用会话，不能只在启动时执行一次；`skippedVersion` 只跳过完全相同的版本。

macOS 安装更新成功后需主动重启，失败时不得重启；Windows 安装器负责退出后的重新启动。运行时只使用更新校验公钥，`TAURI_SIGNING_PRIVATE_KEY` 只用于构建签名，不得进入应用运行时代码。

`package.json` 是 Tauri 版本来源；发布同步更新 Cargo 包和锁文件版本，标签使用 `v${version}`。`.release-it.ts` 中 `ignoreRecommendedBump: true` 用于保留交互选择的版本，不能让 changelog 推荐值覆盖它；beta、rc 发布命令通过 `--preReleaseBase=1` 固定起始编号。发布不上传 npm，打包和 GitHub Release 由 CI 处理。英文更新日志生成后同步中文，再继续发布。

实现入口：[更新逻辑](../src-tauri/src/update.rs)、[发布配置](../.release-it.ts)、[发布命令](../package.json)、[工作流](../.github/workflows/)。

## 尚待完成的 Windows 人工验证

以下是尚未记录完成的桌面交互验证，不表示功能尚未实现。自动测试、类型检查及任务的完成状态不能替代实际 UAC 和计划任务操作。

- [ ] 未提权启动时，引导中的管理员权限操作可用。
- [ ] 授权后出现 UAC，成功启动提权进程后旧进程退出；新进程重新打开引导并显示真实授权状态。
- [ ] 取消 UAC 后原进程保留，并显示错误而不是误报已授权。
- [ ] 发布版本设置 `runAsAdmin=true` 后，后续启动在正常初始化前自动提权。
- [ ] 计划任务与 UAC 回退都实际验证，包括可执行文件路径变化，以及备份文件路径、`--auto-launch` 等启动参数的保留。
