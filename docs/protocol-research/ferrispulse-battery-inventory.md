# 灵脉（FerrisPulse）电池协议清单（研究用）

状态：**初步静态盘点**，不是可直接运行的协议包。上游快照：`4917a10e238ed3dba2938969752886353c8ff5e9`（GPL-3.0）。本文件只记录协议事实与工程边界，不复制 C# 实现。

## 结论先说

- 可以整理出一份“灵脉能取电量的设备/协议事实清单”。
- **不能直接得到一份 AnyBattery 可用的被动 profile 集合**：灵脉大多数协议是主动查询（Output/Feature Set/Get），而当前手动适配 MVP 只允许被动监听 input report。
- 灵脉只有两个声明式 `*.fp-adapter.json` 示例（Sony INZONE Buds、MCHOSE A5 V2 Ultra），两者都包含主动查询；其余协议分散在约 16 个 C# Helper 中。
- 要逐条接入 AnyBattery，仍需要：固定上游 commit、记录来源行号、按型号/接口分类、补真实样本、人工 review。不能把“有 parser”当成“Windows 上一定可读”。

## 模块盘点

| 模块 | 设备族 | 主要入口 | 查询方式 | 被动 MVP 可用 | 备注 |
|---|---|---|---|---|---|
| `GenericHidHelper.cs` | 标准 HID 电池 | Usage Page `0x59` / `0x85` | 标准 HID input/Feature | 部分 | 不是灵脉私有协议，AnyBattery 已有标准 HID 路径。 |
| `BluetoothDeviceHelper.cs` | 通用 BLE | Windows GATT `0x180F` / PnP 属性 | 系统属性/GATT | 部分 | 取决于 Windows 是否暴露 Battery Service。 |
| `XInputHelper.cs` | Xbox 手柄 | XInput / WGI | 系统 API | 系统路径 | 不是 HID 私有协议。 |
| `AppleDeviceHelper.cs` | Apple / Beats | BLE advertisement / WinRT | 广播 + 系统属性 | 部分 | AirPods/Beats 有左右耳/充电盒多节点；不是简单单字节。 |
| `RazerDeviceHelper.cs` | Razer 鼠标 | 90B Feature | SetFeature → GetFeature | 否 | 主动查询，含 TID/队列/外回报文处理。 |
| `RazerHeadsetHelper.cs` | Razer 耳机 | 64B PA/PI | 主动查询 | 否 | 会话/脚本传输，复杂度高。 |
| `LogitechDeviceHelper.cs` | Logitech 鼠标/耳机 | HID++ 1.0/2.0 | 主动 Feature 查询 | 否 | 含 0x1004、0x1F20 ADC 曲线、接收器槽位。 |
| `HyperXDeviceHelper.cs` | HyperX Cloud III Wireless | `0x66` 状态报文 | `0x89` 查询 | 否 | 0x0D 响应；有离线缓存。 |
| `SteelSeriesDeviceHelper.cs` | Arctis Nova 5 / 5X | `0x00B0` | Output 查询 | 否 | 状态上报，但入口仍依赖主动查询。 |
| `MchoseDeviceHelper.cs` | MCHOSE G87 V2 键盘 | `0x4A` / `0x87` | 主动查询 | 否 | Report ID 0x13/0x09；有离线缓存。 |
| `MchoseMouseHelper.cs` | MCHOSE A5 V2 Ultra 等 | 2.4G / WebHID 协议 | 主动查询 | 否 | 与键盘 Helper 不同路径。 |
| `NuphyDeviceHelper.cs` | NuPhy 键盘 | HID 私有报文 | 主动查询 | 否 | 有线/接收器 PID 区分。 |
| `RapooDeviceHelper.cs` | Rapoo 2.4G/有线鼠标 | Report `0x07` 遥测 | 监听 + 查询 | 需逐型号确认 | 有“实时遥测”通道，可能适合被动；必须用真机样本确认。 |
| `ValkyrieDeviceHelper.cs` | Valkyrie 键盘/鼠标 | HID 私有报文 | 主动查询 | 否 | 型号库+私有帧。 |
| `VgnDeviceHelper.cs` | VGN / VXE / ATK | HID 私有报文 | 主动查询 | 否 | 含握手/校验和。 |
| `FlydigiDeviceHelper.cs` | 飞智 APEX 6 / Vader 5 Pro | Usage `0xFFA0`，`5A A5` | 命令 `0x01` 查询 | 否 | 手柄私有帧。 |
| `SonyInzoneHelper.cs` | Sony INZONE Buds | 64B Output/Input | 主动查询 | 否 | 左右耳+充电盒多节点，`SonyInzone` 校验。 |
| `UniversalGamingDeviceHelper.cs` | CompX / Beken / Vaxee / ASUS / Corsair / Keychron / Wooting 等 | Feature / EEPROM | 主动查询 | 否 | 覆盖品牌最多，但每个协议族都要单独验证。 |
| `FdasRuntimeHelper.cs` | 用户 JSON 适配 | `adapters/*.fp-adapter.json` | 由 profile 决定 | 取决于 profile | 示例两个都是主动查询；被动 profile 需要 `request.kind = none`。 |

## 每个协议可提取的事实

在不复制 C# 代码的前提下，可以整理：

- VID/PID、Usage Page/Usage、MI/接口号、input/output/feature 长度；
- 请求字节、report ID、固定匹配头；
- 电量字节 offset、类型、线性/分段公式、min/max；
- 充电字节/位掩码、多节点（左/右/盒）；
- 超时、重试、校验和类型、样本 Hex；
- 来源文件、行号、上游 commit、许可证标记。

不应直接搬运：

- C# 函数体、类结构、缓存/回退/设备选择逻辑；
- 注释、命名、整套常量表的表达；
- 把 GPL 代码机械翻译成 Rust 来规避许可证。

## 与当前被动 MVP 的关系

当前 `protocol_wizard` 只接受：

```text
request.kind  = none
response.kind = input
verification  = unverified
```

因此：

- 灵脉里“主动查询才能拿到电量”的协议（Razer、Logitech、HyperX、SteelSeries、MCHOSE、Sony、VGN、Flydigi、Universal 等）**不能**直接导入为被动 profile。
- 只有设备会主动发出含电量 input report 时，才能用两点校准向导；Rapoo 的遥测通道是候选，但必须真机确认。
- 灵脉清单更适合作为“内置主动适配器”的参考来源，而不是被动 MVP 的数据源。

## 下一步建议

1. 固定上游 commit，写一个只读脚本枚举 `src/Hardware/*.cs` 与 `examples/adapters/*.json`，输出 JSONL：来源文件/行号/hash、VID/PID、usage、请求类型、offset、公式、未知项。
2. 人工 review 每条事实，标记 `mvp_passive_eligible` 与证据等级；不自动把 C# 翻译成可执行 profile。
3. 对每个候选设备补真实 input 样本；没有样本的一律标“未验证”。
4. 只有确认“设备会主动上报”的候选，才交给 `protocol_wizard` 做两点校准。

本文件是盘点，不是兼容性承诺。
