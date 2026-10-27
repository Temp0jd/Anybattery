# 像素电池图标与自绘标题栏实施计划

日期：2026-10-27。用户新要求：替换应用图标为像素风电池；窗口与 GitHub 示意图一致，不再使用 Windows 原生最小化/最大化/关闭按钮。主代理负责方案与验收门，DeepSeek worker 负责实现、资源生成和测试。

## 参考与现状

- GitHub 示意图：`docs/ui-mockup-pixel.png` / `docs/ui-mockup-pixel.svg`。标题栏为 40px、顶部浅底、2px 深色分隔线、左侧 AnyBattery、右侧三个像素按钮。
- 当前 `main.rs` 使用 `ViewportBuilder::with_decorations(true)`，必须改为自绘标题栏。
- 保留现有八位堂像素风、暖色/深色主题、硬边框、零圆角、Fusion Pixel、设备卡片和已有 UI 修复。
- 旧 release 打包已因本需求暂停；完成前不得把旧包称为最终版。

## 图标

1. 使用项目自有像素图形：透明底、电池外框与正极、3 格左右电量块、有限色板、硬边缘，无文字/圆角/抗锯齿渐变。以 16×16 或 32×32 整数网格设计，最近邻缩放。
2. 同一图形用于：
   - exe 的 Windows 图标资源（多尺寸 `.ico`，至少 16/32/48/256）；
   - `icons::window_icon()` 的窗口/任务栏 RGBA；
   - 自绘标题栏左侧小图标；
   - 托盘默认图标。托盘仍按最低电量/充电动态变色，不破坏现有逻辑。
3. 优先用同一像素 map/绘制函数生成各目标，避免四套不一致图形。可新增源 SVG 供维护，但运行时不依赖外部文件。
4. exe 图标用最小 build-dependency/build.rs 嵌入；不引入 WebView2、动态 CRT 或运行时资源文件。构建后验证资源确实进入 exe。

## 自绘标题栏

1. `decorations=false`，`resizable=true`，最小 client 仍为 380×520，默认 430×680。
2. 在所有页面顶部绘制约 40 逻辑像素标题栏：主题浅底/深色对应色、2px 底部分隔线、左电池图标 + AnyBattery、右侧最小化/最大化或还原/关闭三个像素按钮。
3. 按钮保持方形、清晰 hover/pressed 反馈和中文 tooltip；关闭 hover 可用主色红。不得出现 Windows 原生 caption 按钮。
4. 行为：
   - 最小化调用 ViewportCommand，保留原生最小化动画；
   - 最大化/还原切换并更新图标；
   - 关闭调用现有 `hide()`，即隐藏到托盘，不退出；退出仍只走托盘菜单；
   - 非按钮区域单拖移动窗口，双击切换最大化；按钮不能误触发拖动；
   - 保留 Windows snap/drag 行为（由 DragWindow 提供时）。
5. 保留边框/角落缩放。优先使用 eframe 0.37 的 `StartDragResize`/对应 API；先核对实际版本 API，不猜签名。最大化时禁用边缘缩放。若纯 egui 无法实现，报告所需最小 Windows 非客户区方案，不静默丢掉缩放。
6. 标题栏跟随主题；高 DPI 使用现有 snap/pixel 规则。增加标题栏后，设备、设置、诊断页在默认/最小 client 不重叠、不丢页脚；需要滚动时使用现有滚动区。

## 文件边界

允许：
- `crates/anybattery-app/src/{main.rs,app.rs,widgets.rs,icons.rs,theme.rs,tray.rs}`（只改必要文件，保留已有未提交 UI 修改）
- `crates/anybattery-app/Cargo.toml`、新的 `build.rs`
- `crates/anybattery-app/assets/` 内新增图标资源
- `docs/ui-mockup-pixel.svg` 与 `docs/ui-mockup-pixel.png`：改为自绘标题栏说明并加入电池图标
- `.pi/` 下测试脚本、构建和截图产物

不允许：修改 Windows 协议代码、README、DESIGN、其他 docs、release 或提交。README/DESIGN 的原生标题栏文字由后续文档任务统一更新。

## 验证门

1. `git diff --check`、`cargo test -p anybattery-core`。
2. `cargo xwin check -p anybattery-app --target x86_64-pc-windows-msvc --all-targets`。
3. `cargo xwin test -p anybattery-app --target x86_64-pc-windows-msvc --no-run` 并运行 app 测试 exe；同时运行 anybattery-windows 测试，确保无回归。
4. release 构建；PE 仍为 x64 GUI、静态 CRT，无 WebView2；图标资源存在。
5. Windows 实机截图：默认 430×680 与最小 380×520 client，当前 150% DPI；必须实际读取截图，其他窗口遮挡/裁剪/旧原生按钮仍在均不算通过。
6. 交互验证：最小化、最大化/还原、标题栏拖动、边缘或角落缩放、关闭到托盘（进程仍在）及再次显示。自动化失败要如实标记，不能用静态代码代替交互验收。
7. 扫描回归：M618 80、OPPO/Zero 有效电量、键盘不猜百分比；UI 修改不得改变协议。

完成后交付修改文件、资源生成方式、行为/构建/截图/交互证据及未决限制。