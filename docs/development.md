# 开发注意事项

本文记录修改 EcoPaste 时容易遗漏的现有行为和原因。通用编码规则以根目录 `AGENTS.md` 为准；修改相关实现时同步更新本文，不维护完整命令、字段或默认值清单。

## 剪贴板读取与写回

手动读取与监听捕获共用 `build_item_with_settings`、`persist_and_notify`，避免两条路径在去重、来源应用、提示音和刷新事件上产生差异。原生剪贴板句柄不要跨越 `await`；先同步读取或写回，再把可跨线程的数据交给异步入库逻辑。

Windows 剪贴板变更通知可能早于其他程序释放剪贴板。监听器只对读取错误做有界重试；成功读到内容和成功返回空内容都立即结束。耗尽重试后只记录最终错误，不逐次刷日志。不要在普通自动测试中直接操作用户的系统剪贴板。

写回历史记录前调用 `WritebackGuard::suppress`，监听器重新计算内容哈希后通过 `should_skip` 判断是否跳过。抑制依据是内容哈希和短期有效期，不是全局布尔开关，否则会吞掉期间发生的其他复制。修改写回格式时，写入端与读取端必须得到相同的哈希。

文本的 `content` 保存所选 HTML、RTF 或纯文本表示，`search_text` 保存系统提供的纯文本，供搜索和纯文本粘贴使用。图片内容保存资源文件名，文件内容保存换行分隔的路径。敏感信息识别、脱敏、文件条目、颜色预览和可用操作由 Rust 准备，前端不要重新推导。

来源应用在读取剪贴板前捕获，避免异步入库时前台应用已经改变。macOS 用 bundle id，Windows 用可执行文件路径标识来源；应用信息入库失败时可以去掉来源，但不能丢弃剪贴板内容。

实现入口：[监听器](../src-tauri/src/clipboard/watcher.rs)、[写回](../src-tauri/src/clipboard/write.rs)、[命令](../src-tauri/src/commands/clipboard.rs)。

## 数据库、资源目录与备份

`DatabaseState` 管理当前连接池。命令通过 `db.pool().await` 获取当前池，仓储接收 `&SqlitePool`；不要长期保存可能已被替换的旧池。覆盖导入备份和切换数据目录需要通过 `DatabaseState::close_and_replace` 关闭并替换连接池。

持久化根目录统一经 `core::paths` 解析。开发和发布数据分别位于 `dev/`、`prod/`；自定义根目录由启动配置 `storage.json` 与目录内的 `.ecopaste-storage.json` 标识共同确定。功能模块不要自行解析系统应用目录，绕过自定义目录设置。

运行中切换目录会暂停剪贴板监听、复制已知数据目录、替换数据库池，并同步设置和资源路径，最后广播设置及剪贴板刷新。修改其中一环时检查其余状态，避免数据库已切换但图片、图标或设置仍指向旧目录。

搜索和分页由 Rust 返回最终结果。短关键词与 FTS5 搜索走不同查询，置顶优先于所选排序；前端使用返回的 `total`、`hasMore`，不要根据页大小猜测是否还有内容。

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
