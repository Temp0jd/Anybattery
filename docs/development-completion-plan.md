# AnyBattery 完成路线与验收计划

日期：2026-10-27  
分工：主代理负责规划、证据审查、集成与验收；实现 worker 使用 `command-code/deepseek/deepseek-v4.1-flash`。每个阶段单独修改、单独审查，不把 UI、协议和发布改动混成一个不可回退的大 diff。

> 2026-10-28 更新：单实例、文件日志/panic hook、分阶段扫描 + 看门狗、用户被动 2.4G 适配 MVP 已实现；当前测试为 core 37、windows 209（GNU + wine）、app 76；UI 已按 400×528 / 320×480 紧凑规格重构，刷新间隔使用自绘像素滑杆。下表保留当时的基线判断，最新状态以上述更新和 README 为准。

## 1. 已核实基线

- 起始 HEAD：`70906c7`，工作区干净。
- `cargo test -p anybattery-core`：13 项通过。
- Linux `cargo test --workspace --all-targets`：因宿主缺少 `gdk-pixbuf-2.0` 停在系统依赖构建，不能据此判断 Windows 应用代码失败。
- 已准备 `cargo-xwin 0.23.1`、`x86_64-pc-windows-msvc` target、clang/lld；Windows 交叉检查在 worker 修改完成后执行。
- 当前环境不能替代 Windows 真机：托盘、通知、DPI、HID、蓝牙、睡眠唤醒与长时运行均需单独验收。

## 2. 实现状态审查结论

| 范围 | 代码证据与结论 |
|---|---|
| 像素 UI、主题、卡片、设置、诊断、动效 | 已有较完整实现；本轮只做遗留缺口与静态布局审查，不重新设计 |
| 扫描状态 | 已有单 worker 和重扫标记；结果通道断开、手动刷新排队语义需完成本轮复核 |
| 排序 | `scan.rs` 已按 `DeviceSnapshot::sort_key` 排序，但其键是状态组→电量→名称，并非设计稿完整描述；不得再写“完全未排序” |
| 粗粒度电量 | UI 三段低/中/满，不换算伪百分比 |
| 缓存与休眠 | 没有跨扫描/跨启动缓存合并与 `state.json`；`Asleep/Stale/Cached` 类型或 UI 不等于功能完成 |
| JSON 协议 | core 有 schema、校验、提取和单测；扫描执行、导入试读、保存和 UI 尚未接入 |
| BLE GATT | 未实现主动 Battery Service 读取；目前只有 PnP 属性路径 |
| 系统 Toast | 未实现；当前 Toast 是应用内浮层，不是 Windows 系统通知 |
| 单实例 | 未实现命名 mutex 与第二实例唤起 |
| 发布 | 静态 CRT 配置已存在；本轮修改后的 exe、SHA-256 和 zip 尚待构建 |

## 3. P0：当前渲染与交互收尾

实施边界仅 `anybattery-app` 的 UI 文件，不新增依赖、不改协议层。

验收点：

1. 保留既有 B1–B10 修复；扫描线程/结果通道失败能退出扫描态，不循环重试、不逐帧刷错。
2. 显示设置变化可在扫描中排队且只排队一次；普通手动刷新在扫描中不额外排队。
3. 滑块拖动中即时显示、结束提交；键盘和非拖动修改可保存；无关保存不强制重应用主题。
4. 长中英文按实际像素宽度截断；380×520、430×680 的卡片、设置、页脚和诊断布局无代码层面的重叠。
5. 无电量卡片中文语义准确；展示排序若调整，必须明确与现有 `sort_key` 的关系并有测试。
6. 动画关闭后所有新增补间立即到位；Toast 到期有有限重绘；空闲没有新增永久高频 repaint。
7. `git diff --check` 通过；core 测试通过；Windows 目标检查通过；rustfmt 全仓信息性结果如实记录，不扩大格式 diff。

## 4. P1：构建与当前版本交付门

- 运行 Windows debug check 和 release build，确认 `AnyBattery.exe` 为 x64 PE、资源内嵌、静态 CRT。
- 生成 zip 与 SHA-256；不声称已签名，不声称完成 Windows 真机验证。
- README 与发布说明必须和代码一致。在缓存真正实现前，不把“休眠保留上次值”列为已交付能力。
- Windows 手测最低矩阵：380×520、430×680；100%/125%/150% DPI；深浅主题；动画开关；筛选；扫描中改显示设置；托盘图标重置；长设备名。

## 5. P2：应用可靠性（独立后续阶段）

优先级高于继续增加未经真机验证的协议数量。

1. **缓存生命周期**：定义启动缓存、扫描后保留、设备仍在场但无回复、设备移除、24h stale 的精确语义；原子保存，不保存原始报文；先写平台无关合并测试，再接 Windows 扫描。
2. **单实例**：命名 mutex；第二实例唤起已有窗口；托盘/隐藏窗口均可恢复；退出释放，异常启动不锁死。
3. **系统 Toast**：先做 Windows 原型验证 unpackaged exe 的 AUMID/通知权限行为，再接 20%/10% 每放电周期一次与充电复位；不能用在 Linux 可编译代替系统通知验收。

## 6. P3：扩展能力（各自独立审查）

1. JSON 协议执行器、导入时试读、失败原因、用户目录覆盖与诊断；任意发送仍需二次确认。
2. BLE GATT BAS：仅已配对设备、PnP 缺失时兜底、3 秒超时、10 分钟限流，不写 characteristic。
3. Jieli/DARKRIM、Steam Controller 2015 等协议：必须先有可靠报文事实与 fixture，再实现 adapter；未知字节不猜电量。
4. 厂商协议 fixture 与真机矩阵逐步补齐；community/unverified 标记必须保留。

## 7. 完成定义

“代码完成”与“真机验收完成”分开报告。只有对应自动化检查、静态审查和必要的 Windows 验证都有证据时，才能把某项标为完成；硬件相关功能不得仅凭编译通过宣称可用。

## 8. 2026-10-27 本轮验收记录

### 已自动验证

- DeepSeek worker 仅修改 `crates/anybattery-app/src/app.rs` 与 `widgets.rs`；主代理审查了完整 diff。
- `cargo test -p anybattery-core`：12/12 通过。
- `cargo xwin check -p anybattery-app --target x86_64-pc-windows-msvc --all-targets`：通过。
- `cargo xwin test -p anybattery-app --target x86_64-pc-windows-msvc --no-run`：通过；随后通过 WSL Windows interop 实际执行测试 exe，6/6 通过（新增排序 2 项，加上时间格式、对比度与像素对齐 4 项）。
- `cargo xwin build -p anybattery-app --target x86_64-pc-windows-msvc --release`：通过。
- `git diff --check`：通过。
- PE 检查：`PE32+ executable (GUI) x86-64`；导入表无 `VCRUNTIME*`、`MSVCP*` 或 WebView2；zip 内只有 `AnyBattery.exe`。

### 实际渲染证据

在 Windows 11、144 DPI（150%）下启动本次 release，真实蓝牙设备成功显示电量。截图保存在：

- `release/visual-default.png`：客户端 645×1020 物理像素，即 430×680 逻辑尺寸。
- `release/visual-minimum.png`：客户端 570×780 物理像素，即 380×520 逻辑尺寸。

两种尺寸的设备页均可见完整标题、动作区、筛选、设备卡片、百分比、电量格与页脚，没有文字或控件互相覆盖。设置/诊断页本轮仍只有静态审查；自动导航未成功，不计为视觉验收。

### 信息性检查与边界

- `cargo fmt --all -- --check` 仍失败，151 个格式 diff 区段；这是本轮前已有的全仓格式债，没有用大面积格式化扩大改动。
- Linux 全 workspace 测试仍受宿主 GTK 系统依赖影响；Windows 目标的编译、链接与测试 exe 执行已分别完成，不混淆这两类结果。
- 设置/诊断交互、动画开关、Toast 到期、扫描中托盘菜单、长设备名、DPI 100%/125%、设备插拔、睡眠唤醒及 24h soak 仍需继续手测。

### 发布物

- `release/AnyBattery-0.1.0-win64.zip`
- `release/AnyBattery.exe`
- `release/SHA256SUMS.txt`

当前未签名，未提交 Git commit；缓存、系统 Toast、单实例、JSON 导入和主动 GATT 仍按上述后续阶段处理。
