# 协议扩展实施计划（主代理指导稿）

日期：2026-10-27。实施、源码复核和测试尽量交由 `command-code/deepseek/deepseek-v4.1-flash`；主代理只拆分方案、设验收门和控制集成顺序。

## 共同约束

- 先读 `docs/protocol-research/` 对应研究报告，再打开一手源码核验；报告可能有错，不得照抄。
- 每个 worker 只能修改指定文件，保留其他未提交修改，不改 README/docs/release，不提交，不全仓格式化。
- 若子会话的 `fabric_exec` 不可用，直接使用原生 read/grep/find/ls/bash/edit/write；先最小读取确认，不要因单一工具路由失败就放弃任务。
- 真实 HID/蓝牙扫描只能由一个被明确授权的 worker 串行执行；其他 worker 只做 parser 测试和编译。
- 新协议需要精确 VID/PID + usage/interface/report 形状，严格拒绝错误头、长度、checksum、离线和越界值；充电/缩放冲突保持 `None`。
- 每项记录实际核对的仓库、commit、文件和许可证。GPL/无许可证项目只取协议事实，不复制实现。
- 验证：`git diff --check`、`cargo test -p anybattery-core`、`cargo xwin check -p anybattery-app --target x86_64-pc-windows-msvc --all-targets`、`cargo xwin test -p anybattery-windows --target x86_64-pc-windows-msvc --no-run` 并运行测试 exe。

## 已完成 / 进行中

- Bluetooth Hands-Free 服务节点按 ContainerId 合并：已实机 OPPO 单卡显示电量，20 项 Windows 测试通过。
- 标准 HID `0x64/0x65`、移除 `0x66/0x0001` 百分比候选、同报告充电位：已完成，27 项 Windows 测试通过。
- Wave B1/B2 均已完成；B2 集成时 45 项 Windows 测试通过。
- Wave C 已完成：0528/VAXEE/同句柄 Feature、Corsair/PA；各自集成时 73 项 Windows 测试通过。
- HID 审查后续：worker `8ed04926-efbe-446a-bafd-0ec02e047feb`；协议集成只读审查：worker `38d27f18-0c2e-4a17-bfad-7e2489183a22`。
- 最终文档任务需同步修正 `DESIGN.md` 的标准 HID usage，以及 README 的 HeadsetControl GPL-3.0 许可证和实际支持等级。

## Wave B1：`extra.rs` 纯解析纠偏

来源：`03-other-vendors-controllers.md`，本地 SDL/HeadsetControl/HaloBattery 完整仓库。

1. SteelSeries B0 按 PID 区分 Nova7 旧离散档和 Gen2 直接百分比；修 Nova5 离线条件、level/charging 偏移。Rival/Aerox 新协议不在本批。
2. DualSense/DualShock4 按 SDL/Linux 修状态机：DualSense full、0x0C unknown；DS4 11 full、14/15 不错误显示 100。
3. Steam Controller 2025 使用约 5 秒总监听窗口，不在 450ms 空闲时提前退出；不改 `hid.rs`。
4. Keychron `3434:D028/D048` 是 Mouse；8BitDo 补 `0x6012`，证据不足型号不标 Verified。
5. 加纯 parser 测试，保留现有 1D57 行为；本 lane 不改其他文件。

## Wave B2：`vendor.rs` Razer/Logitech 纠偏

来源：`02-razer-logitech.md`，本地 OpenRazer/Solaar/HaloBattery。

1. Razer 0528 不得继续走 90 字节 generic；0557 有线 BlackShark V2 X USB 不得标成 2023 Pro 或发送电池查询。本批先移除错误路由，0528 的 64B 适配留到有同句柄传输方案的后续任务。
2. 核查并修 Verified/Community 过度声明；不将“同句柄”当未证实的万能修复。
3. Logitech 补 C545/C547/C54D；修 0x1001 flags（外接电源不等于充电）、0x1000 合法 0/full、0x1F20 未知 flags；status 4 和电压曲线冲突保持保守。
4. 本批不做 HID++1.0、short/long collection 重构、G535/固定耳机命令或 unit ID 合并，不改 `extra.rs/hid.rs`。
5. 加纯 helper/parser 测试。

## 审查后续队列

- Bluetooth 双模/多根/HFP、根节点优先级、container 规范化和名称合并：代码完成，73 项 Windows 测试通过；真实 OPPO/Zero 复测留到最终串行验收。
- HID 后续已完成：完整 collection identity、Battery System 候选优先级、BTHLE transport；80 项 Windows 测试通过。
- OEM 独立审查7项已全部修复：caps65 SET/GET、A5分组唯一path、PID感知transport、OEM去重和测试覆盖。最终独立验证通过：123/123 Windows tests；`.pi/diag-final-integration.txt` 实扫2.99秒，M61880、OPPO90、Zero46、3554单张unknown且无重复。OEM保持Community。README最终同步 worker `676fd3d3-bec4-484c-b580-4db789924cb9`；release构建/实扫/打包 worker `6c0a3a0f-c4dc-4345-b43f-7cd6bf0a6589`。

## 集成审查后的运行时修复队列

运行时修复已完成：115项Windows测试通过；实机M61880、OPPO90、Zero46，30秒内有界。README矩阵需将Corsair/Keychron/PA从修复中改为社区（有脚本传输测试、无对应真机）。Razer0555置信度已降Community，115项测试通过；PA实际路由不受影响。README矩阵已校正为110行（✅3/🟡87/🧪4/⚪5/❌11），191/191 PID；DESIGN标准HID已纠正。worker `d3ebba0d-8885-4f6a-a570-37264e5f32b9` 正在删除误恢复的“跨扫描保留上次电量”声明；缓存仍未实现。OEM完成后需同步最终测试数。

- P1 Corsair：同句柄 drain、0A40 firmware/receiver/headset 会话、约 6 秒有界窗口、两次一致答复确认，再读充电；当前 500ms 很可能不足。
- P1 Keychron：限定 interface 4，同一句柄 Feature SET + Input 读取，清旧报告并最多 3 次重试。
- P2 Razer PA 及报告其余项：按完整审查核对 drain/retry/remote-off 会话清理和测试真实性；remote-off 仅限恢复会话状态，不做配置写入。
- 另设 runtime fixtures/传输测试，不能仅凭静态 parser 通过就宣称这些设备可读取。

## Wave C（B 完成后，避免文件冲突）

- Corsair Dark Core/Virtuoso 分协议解析；Razer PA 严格 `P/I` 与充电；SteelSeries Aerox/Rival 分型。
- VAXEE ×5/充电页、Endgame 回包校验，以及 feature 同句柄 helper；证据弱的 Nordic 降级，不强升 Verified。
- Razer BlackShark 2020 `0528` 64B `0xFF` 专用适配。
- 键盘 `0x05` 身份识别与型号码；未知 `0x6E` 不发送可能写配置的 `0x0B`。

## DELUX M618Air 专项

- 用户确认现场 `1D57:FA60` / `03 24 40 01 06` 设备是 DELUX M618Air，不能仅凭共享 `device_id=0x24` 命名为 Attack Shark。
- M618Air 已完成：官方 helper 映射 raw6/7→80%，用户官方80%校准，AnyBattery实机raw7显示80%，102项Windows测试通过，30秒硬超时内有界扫描。`08-delux-m618air.md` 已更新；state02/03/80仍拒绝，名称保持中性。
- 保留通用名称直到有可靠的型号区分证据；缩放与充电语义必须由 M618Air 或确切共享固件证据支持。

## Wave D（独立新协议任务）

- PixArt/WebHID、MCHOSE 反码、Delux M800 Pro 33B 的详细方案已完成：`07-oem-implementation-plan.md`（1077 行，已校验 23 个源码链接与 fixtures）。实施排在 Wave C 文件释放后；M800 Pro 的 Windows MI_01 若无 HID 证据则停止，不增加 raw-USB；V9 Turbo+ 耳机不得进入鼠标反码协议。
- Jieli、Sony INZONE、MCHOSE V9、JBL、HyperX 新家族、Steam 2015。
- 每项独立源码审计、parser fixtures、文件边界与真机计划；没有硬件时不把编译成功写成 Verified。

## 工具故障待重派

- `09-headset-implementation-plan.md` 的 Jieli/Sony/MCHOSE V9/HyperX 方案在初始与恢复 run 中均因子会话只暴露失效的 fabric_exec 而未完成；没有生成文件，停止该恢复链。后续用全新 worker 重派，不得将这些协议写成已适配。
- 同一运行时若原生工具也不存在，恢复一次后即停止，不继续重复空跑。

## UI 新需求：像素图标与自绘标题栏（2026-10-27）

- 用户要求像素风电池应用图标，并按 GitHub 示意图统一窗口，去掉 Windows 原生 caption 按钮。
- 旧 release 打包已暂停，不能作为最终版。实施与验收方案：`docs/ui-titlebar-icon-plan.md`。
- UI worker `5369371c-e969-45d0-9f6f-c0c517b2c283` 负责图标资源、decorations=false、自绘标题栏、窗口行为、exe资源和Windows截图/交互。
- 追加五档电量配色：`docs/battery-color-tiers-plan.md`，0–10红、11–20橙、21–50黄、51–80黄绿、81–100绿；卡片与动态托盘共用helper，已steer同一UI worker。
- 0.1.0 已按用户确认发布：README/DESIGN已同步像素图标、自绘标题栏和五档配色；未重复完整测试/实机扫描。exe SHA256 `3544f73ff1c4c561ab494d9bf5ba2b8fd676274b36daaf38202267e709fba831`，zip SHA256 `8716ef1c40d4f813f0f253457c9e28fcc7f75fc73200a5a77c0cf301a5f49c04`。

## 最后集成

- 一个串行验证 worker 运行真实扫描并保存输出：OPPO/Zero/1D57 不回归、键盘不猜电量、扫描有界。
- 主代理根据 worker 证据决定文档支持等级和 release 更新时间；未决项保留研究状态。
