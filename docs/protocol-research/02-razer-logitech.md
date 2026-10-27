> 状态：子代理研究输入。所有“假支持”、充电语义和曲线结论须经主代理一手源码复核后实施。

# Lane 2 深度研究简报：Razer + Logitech 电量协议

日期：2026-10-27  
范围：只研究 Razer 与 Logitech；不访问真实 HID/蓝牙设备，不运行扫描/探针，不修改 AnyBattery 产品代码。  
基线：AnyBattery 工作树 HEAD `70906c7`，含未提交 UI/协议改动；Razer/Logitech 关键实现位于 `crates/anybattery-windows/src/vendor.rs`、`extra.rs`、`hid.rs`、`scan.rs`。  
证据原则：协议事实优先来自一手源码/官方文档；HaloBattery、FerrisPulse 作为 Windows 独立实现交叉印证；GPL 代码只读事实，不复制实现。

> **FerrisPulse 校正（2026-10-27）**：本文下方早期 Razer PID 映射来自 OpenRazer/HaloBattery 快照，已过时。以下实现事实以 FerrisPulse commit `4917a10e238ed3dba2938969752886353c8ff5e9`（GPL-3.0；仅提取协议事实，不复制代码）和当前工作树 `crates/anybattery-windows/src/vendor.rs` 的 `razer_name`/`razer_uses_special_protocol` 为准：
> - Razer Viper V2 Pro 是 `1532:00B4`（有线）/ `1532:00B5`（接收器），首选 TID `0x1F`，Community。
> - `1532:00A5` / `1532:00A6` 是 Razer Cobra Pro，不是 Viper V2 Pro。
> - 原文 B2/B3/C1 中 `00A5/00A6 = Viper V2 Pro`、`00B4 = Naga V2 HyperSpeed` 等旧映射只保留作研究历史，**不得再作为实现依据**；当前 90B 表以代码为准。
> - 本文其余“当前代码”差距描述以基线 `70906c7` 为准；`0528`/`0557`/90–91 字节等已在当前工作树修正，勿当作最新实现状态。

## 结论速览

1. **Razer 当前表基本是 HaloBattery `KNOWN` 的拷贝，并继承了它最危险的两个型号错误**：`1532:0528` 不是 90 字节标准协议，而是 64 字节 `0xFF` Feature 协议；`1532:0557` 的公开 USB ID 是 **BlackShark V2 X USB（有线）**，不是 BlackShark V2 Pro 2023。当前却把两者都标成 `Verified` 并走 `read_razer`（90 字节）路径。[7][8][10][32]
2. **`razer_query` 的“先关句柄、再开新句柄 GetFeature”不是已被证实的通用根因**。HaloBattery/FerrisPulse 确实都在同一句柄上完成 Set/Get，但 Microsoft 的 API 语义是“对 top-level collection 操作”，没有文档说必须同一句柄；不要把它当成一次性通用修复。应优先排查型号/协议分支、interface、90/91 字节、transaction ID、Synapse 抢占和外回报文。[8][16][35][36]
3. **Razer PA 耳机当前只读电量，不读充电**；`0x0555/0x0556` 和 `0x053A` 都有公开的 `0x2A` 充电命令，当前 `read_razer_pa` 固定 `charging: None`。[11][12]
4. **Logitech 当前实现缺少短/长 collection 配对、1.0 回退、特征 capability、unit ID 和 3 个接收器 PID**；空槽/离线时会白等，某些设备会读不到，某些状态会被误判为“充电”。[13][18][19][21]
5. **Logitech 电压曲线存在公开实现冲突**：`0x1F20` 在 3905 mV 时，Solaar/OpenLogi/HaloBattery 曲线给约 67%，按 Linux 内核耳机曲线计算约 87%。当前用的是前者；在真机与 G HUB 对照前不要宣称精确。[13][18][30][31]
6. **`0x1004/0x1000` 的 status=4 也存在冲突**：Solaar/OpenLogi 称“慢充”，Linux 内核把 4 当充电错误。当前把 1..=4 全当充电；对 4 应至少标为不确定。[18][29][31]

---

## A. 最重要新发现

### A1. Razer `0x0528` 当前“已验证”是假支持

当前 `razer_name` 把 `0x0528` 标为 `"Razer BlackShark V2 Pro (2020)", 0x3F, Verified`，然后 `read_razer` 会用 91 字节、class `0x07`、command `0x80` 去读。[当前 `vendor.rs:101`, `vendor.rs:234-270`]

但 HaloBattery 的 2020 专用实现明确写明：该接收器使用 **64 字节 Feature Report `0xFF`**，请求为：

```text
FF 0A 00 FD 04 12 F1 02 05 00 ... 00   (64 字节)
```

回复前缀为：

```text
FF 0F 05 FE 12 04 1F 08 05 ...
```

字段：state 在 offset 11，电压 big-endian 在 12..13，设备自报电量在 14；state `0x09` 为充电中，`0x06` 为充电完成，`0x01/0x02/0x03` 为放电/低电/极低电；`FF 01 00 FE 12 04 1F 08 05` 是“无新回复”的陈旧前缀，必须拒绝。[10][9]

HaloBattery 还明确说：0528 的硬件**拒绝**通用 90 字节和 PA 读取，不能做 generic/PA fallback；其充电线 `0x052E` 被跳过，电量从接收器读。[8][40]

因此当前对 0528 的 `Verified` 标注和 `read_razer` 路径都不成立；它最多显示“未识别”，不会得到电量。

### A2. Razer `0x0557` 不是 BlackShark V2 Pro 2023，而是有线 BlackShark V2 X USB

当前表：

```rust
0x0557 => ("Razer BlackShark V2 Pro (2023)", 0x3F, verified)
```

[当前 `vendor.rs:104`]

公开 USB ID 数据库把 `1532:0557` 标为 **Razer BlackShark V2 X USB**，USB class `01-01-00`（audio），不是无线耳机的 2.4 GHz 接收器。[32]

当前排除列表只排除 `0x053A | 0x0555 | 0x0556`，没有排除 0557；如果该设备暴露的某个 HID collection 满足 `feature_len >= 90` 或是 vendor collection，0557 就会进入 `read_razer` 候选路径，向一个有线 USB 音频设备发 90 字节 Feature 请求。至少应删除该型号表项；即使它确实暴露 HID collection，也不应该按无线耳机去读。[当前 `vendor.rs:53-59`]

### A3. Razer 2023 耳机 PA 协议当前缺少充电命令

`0x0555/0x0556` 的 PA 协议有两条命令：

- `0x21` 电量
- `0x2A` 充电

HaloBattery 的 `PASession.query()` 对两条都发；`read_battery()` 返回 `(level, charging)`。[11]

当前 `read_razer_pa` 只发 `0x21`，最后固定：

```rust
Some(reading(level, None, "Razer PA 耳机", ...))
```

[当前 `extra.rs:330-338`]

因此即使电量读对，充电状态永远未知。`0x053A` 的 Barracuda 也一样有 `0x2A`，当前同样丢弃。[12]

### A4. Razer 90 字节报文长度不是永远 91

OpenRazer 的 `struct razer_report` 是 90 字节（无 report ID）；Windows HID Feature Report 通常在 90 字节前再加 report ID 0，所以是 91 字节。[2][3]

FerrisPulse 的 Windows 实现不是固定 91，而是：

```csharp
bool prepended = (caps.FeatureReportByteLength == 91);
int offset = prepended ? 1 : 0;
...
byte[] req = CreateRazerReport(..., caps.FeatureReportByteLength, prepended);
```

即根据 `HidP_GetCaps().FeatureReportByteLength` 适配 90 或 91。[16]

当前 `build_razer_request` 永远返回 `[u8; 91]`，`feature_get` 永远请求 91；如果某个 collection 的 FeatureReportByteLength 是 90，`HidD_SetFeature/GetFeature` 可能直接失败。[当前 `vendor.rs:186-200`, `vendor.rs:220`; `hid.rs:271-297`]

这不是“协议不同”，而是 Windows HID 长度适配缺口；应把实际 `info.feature_len` 传进 builder/parser。

### A5. Razer 参考实现都用同一句柄，但没有证据证明“必须同一句柄”

HaloBattery 的 `_read()` 打开一次 `hid.device()`，在同一次 `_query()` 里先 `send_feature_report`，再循环 `get_feature_report`，最后才 `close()`。[8]

FerrisPulse 在 `CreateFile` 后，对同一个 `handle` 调 `HidD_SetFeature` 和 `HidD_GetFeature`。[16]

但 Microsoft 文档只说明：

- `HidD_SetFeature` 向一个 top-level collection 发送 feature report；
- `HidD_GetFeature` 从一个 top-level collection 返回 feature report；
- 两者都以 `HANDLE HidDeviceObject` 为参数，没有“必须同一句柄”的说明。[35][36]

**推断**：feature report 的状态属于设备/collection，不属于文件句柄；关闭再打开同一路径，理论上仍访问同一 collection。当前写法是“与参考实现不同”，但不是已被证实的根因。真正需要真机 A/B 的是：同句柄 vs 重开句柄、91 vs 90、接口选择、Synapse 并发。不要把“改同句柄”作为唯一补丁。

### A6. Razer 外回报文/Synapse 抢占：当前只轮询，不重发

HaloBattery 的 issue #108 记录：Synapse/Chroma 以很高频率向同一 collection 发 class `0x0F` 灯光帧，设备的回复缓冲里经常是别人的 `status=0x02, cmd=0f:03`。它的 `_query()` 在读到外回报文时会：

- 继续读；
- 每 4 个外回报文重发一次自己的请求，最多 3 次；
- 最后返回 `STATUS_FOREIGN` 并给出“另一个 app 占用”的诊断。[8][37]

当前 `razer_query` 只在 `status == 0x01 || 0x02` 时继续循环，**没有重发**；如果 Synapse 持续覆盖，它会一直读到 1 秒超时，然后失败。[当前 `vendor.rs:218-230`]

### A7. Logitech 当前只打开一个 HID path，接收器的错误回复可能在短 collection 上

HaloBattery 的 `_Channel` 同时打开：

- long path（20 字节，`ff00:0002`）
- short path（7 字节，`ff00:0001`）

每次请求都同时读两个 path；因为接收器的错误回复（`0x08` 空槽、`0x09` 离线、`0x01` HID++1.0）经常以**短报文**返回。[13]

当前 `read_logitech` 只遍历单个 `info.path`，`hpp_request` 也只读这个 path。对于 long collection，如果错误回复从 short collection 来，当前看不到，只能等 350ms 超时；空槽/离线会变慢，某些设备会直接读不到。[当前 `vendor.rs:544-585`, `vendor.rs:716-733`]

### A8. Logitech 接收器 PID 列表缺 3 个 Lightspeed PID

Solaar 的 `KNOWN_RECEIVERS` 包括 `0xC545`、`0xC547`、`0xC54D`；当前 `LOGITECH_RECEIVERS` 只有 20 个，缺这三个。[21][当前 `vendor.rs:524-527`]

当前还有 product string 兜底（`receiver/unifying/lightspeed/bolt`），所以不是所有设备都失败；但如果产品字符串是通用的 `USB Receiver`，就会被当成非接收器，只探 slot `0xFF`，从而漏掉 1..6 槽设备。

### A9. Logitech `0x1001` 当前把“外接电源但未充电”误判为充电

Linux 内核对 `0x1001` 的 flags 解析是：

- bit7 = 外接电源存在；
- bits0..2 = 充电状态：0 charging，1 full，2 not charging，3..7 unknown；
- bit3 fast，bit4 slow，bit5 critical。[31]

当前只做：

```rust
(payload[2] & 0x80 != 0)
```

[当前 `vendor.rs:685-691`]

因此 flags `0x82`（外接电源、但 status=2 not charging）会被显示为“充电中”。这是明确的状态误判。

### A10. Logitech `0x1000` 的 0% 是合法放电值，当前会丢弃

HID++ 2.0 草案明确：`BatteryDischargeLevel` 0 表示 unknown，但在 discharging 状态下也可以是合法的空电量；Linux 内核在 status=0 时直接使用 `data[0]`，status=3 full 时把 capacity 强制为 100。[25][31]

当前 `F_STATUS` 只接受 `1..=100`，0 被跳过。[当前 `vendor.rs:677-684`]

结果：0% 的鼠标可能不显示；full 状态但 byte0=0 时也不会显示 100%。

### A11. Logitech `0x1004` 当前没有读 capability，也没有处理无 SoC 的 bucket

`0x1004` 的正确流程：

1. fn0 `get_battery_capabilities`：byte0 bit0..3 表示支持 critical/low/good/full，byte1 bit1 表示有 percentage gauge；
2. fn1 `get_battery_info`：byte0=percentage（无 gauge 时无意义），byte1=bucket (1/2/4/8)，byte2=status，byte3=external power。[29][31]

当前只调 fn1，并把 byte0 当百分比；如果设备没有 SoC，byte0=0，当前直接跳过，不会用 byte1 的 90/50/20/5 粗略值。[当前 `vendor.rs:659-676`]

HaloBattery 已经实现了这个 fallback：`APPROX_LEVELS = {8:(90,"full"),4:(50,"good"),2:(20,"low"),1:(5,"critical")}`。[13]

### A12. Logitech `0x1F20` 当前接受 0x0F 等未知 flags

Linux 内核只接受三种 linked 状态：

- `0x01` discharging
- `0x03` charging
- `0x07` full

`0x0F` 和 default 都是 UNKNOWN。[31]

当前只检查 bit0/bit1：

```rust
if payload[2] & 0x01 == 0 || mv < 2500 { continue; }
(voltage_to_percent(mv), Some(payload[2] & 0x02 != 0))
```

[当前 `vendor.rs:692-698`]

因此 `0x0F` 会被当成“已连接、充电中”，而内核认为未知。应只接受 0x01/0x03/0x07；其他值拒绝或标未知。

### A13. Logitech `0x0005` 设备类型当前是 UI 近似，不是协议枚举

官方 HID++ 2.0 草案的 `GetDeviceType`：

```text
Keyboard=0, RemoteControl=1, NUMPAD=2, Mouse=3,
Touchpad=4, Trackball=5, Presenter=6, Receiver=7
```

[25]

OpenLogi 的扩展枚举进一步给 Headset=8。[26]

当前：

```rust
0 | 2 => Keyboard,
3 | 4 | 5 => Mouse,
8 => Headset,
```

[当前 `vendor.rs:641-648`]

2 是 NUMPAD，4 是 Touchpad，5 是 Trackball；把它们映射成 Keyboard/Mouse 是 UI 简化，不是协议事实。若不想扩 `DeviceKind`，至少注释清楚“近似”。

### A14. Logitech 当前没有 HID++ 1.0 回退

Solaar 的 `hidpp10.get_battery()`：

- 先读 register `0x07`（BATTERY_STATUS），再读 `0x0D`（BATTERY_CHARGE）；
- 请求是短报文 `10 <slot> 81 <reg> 00 00 00`；
- `0x0D` 的 data[0] 是百分比，data[2] 高半字节 `0x30/0x50/0x90` 分别放电/充电/充满；
- `0x07` 的 data[0] 是 7/5/3/1（full/good/low/critical），data[1] 的 `0x21`/`0x22` 表示充电/充满；
- 只读，不写。[19]

当前完全没有 1.0 分支；对于没有 feature table 的老设备，`hpp_feature_index(0x0000)` 会返回错误码 `0x01`，当前只是 `None`，不会回退。[当前 `vendor.rs:587-597`]

### A15. Logitech 当前没有 unit ID，接收器/有线同一设备可能显示两张卡

HaloBattery 用 feature `0x0003` fn0 的 bytes 1..5 作为 unit ID；Solaar 的 `get_ids()` 也把 `ids[1:5]` 当 unitId。[13][18]

当前 key 只用 `logitech:{slot}`，没有 unit ID。[当前 `vendor.rs:701-710`]

因此同一鼠标通过接收器和 USB 线同时出现时，container ID 不同，会生成两张卡；README 里“一个接收器下多设备”没问题，但“同一设备跨接收器/线合并”没有实现。

### A16. Logitech G535 的 collection 不是 `0xFF00`

HeadsetControl 的 G535：

- PID `0x0AC4`
- collection `000C:0001`
- battery command `11 FF 05 0D ...`
- 电压在 response bytes 4..5，状态在 byte 6。[23]

当前 `read_logitech` 第一行就是：

```rust
if info.usage_page < 0xFF00 { return Vec::new(); }
```

[当前 `vendor.rs:716-719`]

所以 G535 直接被排除。HaloBattery 也为 G535 单独列了 `HEADSET_COLLECTION = {0x0AC4: (0x000C,0x0001)}`。[13]

### A17. Logitech G533/G633 系列是固定 feature index，不是通用 0x1F20 发现

HeadsetControl：

- G533 `0x0A66`：`11 FF 07 01 ...`，电压 response[4:6]，状态 [6]；
- G633/G635/G733/G933/G935：`11 FF 08 0A ...`，同样电压 [4:6]；
- G535：`11 FF 05 0D ...`。[23]

这些是设备特定的 feature index/function，不是“先 feature discovery 找 0x1F20”。HaloBattery 文档又说 G-series 用 `0x1F20`，两者并不完全一致；这是型号/固件差异，必须逐 PID 真机确认，不能用一个通用 parser 覆盖全部。[9][13][23]

### A18. Razer 的 `0x08` transaction ID 没有一手依据

当前 `read_razer` 的 TID 尝试列表包含 `0x08`：

```rust
[preferred_tid, 0x1F, 0x9F, 0x3F, 0xFF, 0x08]
```

[当前 `vendor.rs:246`]

OpenRazer 的电池/充电分支只使用 `0x1F/0x3F/0xFF/0x9F`；HaloBattery 的 `TRANSACTION_IDS` 也包含 0x08，但没有任何 PID 的 `KNOWN` 使用它，只是 fallback。[4][5][8]

因此 `0x08` 应标为“未验证启发式”，不能作为支持依据；对未知型号盲目尝试会增加误发风险。

---

## B. 协议/型号矩阵

### B1. Razer 通用 90 字节 Feature 协议

来源：OpenRazer `struct razer_report`、`razer_calculate_crc()`、`get_razer_report()`、`razer_send_payload()`。[2][3][4]

| 字段 | offset（90 字节结构，无 report ID） | 说明 |
|---|---:|---|
| status | 0 | 请求 0x00；回复 0x01 busy / 0x02 success / 0x03 failure / 0x04 timeout / 0x05 not supported |
| transaction_id | 1 | 0x1F / 0x3F / 0x9F / 0xFF；基线还试 0x08（无一手依据），当前代码已移除 |
| remaining_packets | 2..3 | big-endian，普通单包为 0 |
| protocol_type | 4 | 0x00 |
| data_size | 5 | 普通电池请求为 0x02 |
| command_class | 6 | 0x07 = power |
| command_id | 7 | 0x80 = battery level；0x84 = charging |
| arguments | 8..87 | arg[0] 在 8；arg[1] 在 9；电池值在 9 |
| crc | 88 | XOR bytes 2..87（含 class/id/args，不含 status/tid） |
| reserved | 89 | 0x00 |

Windows HID 额外加 report ID 0，总长 91；但必须根据 `FeatureReportByteLength` 判断是 90 还是 91。[16]

**请求向量**（tid=0x1F，class=0x07，cmd=0x80，无参数）：

```text
00 | 00 1F 00 00 00 02 07 80 00 00 ... 00 | 85 | 00
```

`0x84` 的 CRC 是 `0x81`（0x02^0x07^0x84）。

**成功回复向量**（DeathAdder V4 Pro，HaloBattery 实机）：

```text
02 1F 00 00 00 02 07 80 00 AB ...
```

`raw=0xAB=171`，`round(171/255*100)=67%`。[40]

**状态语义**：0x02 才表示成功；0x01 继续等；0x03/0x04/0x05 是最终失败/超时/不支持。OpenRazer 的 `razer_send_payload()` 还会检查 `remaining_packets/class/command_id` 与请求一致，并把 busy 当成功重试。[2][4]

**接口/等待（OpenRazer，Linux USB control）**：

- 鼠标：默认 interface index 0；部分 wired 鼠标 index 3；等待 600us / 31ms / 59.9ms / 400ms 随 PID 不同。[4]
- 键盘：interface index 2 或 3；DeathStalker V2 wireless 等待 4.9ms；其余按默认/Chroma wait。[5]
- 当前 AnyBattery 没有 interface 选择，只按 HID collection 探测；这是与 OpenRazer 最大的结构性差异之一。

### B2. Razer 鼠标矩阵（按 TID 分组）

> **旧表警告**：下表是 FerrisPulse 校正前的 OpenRazer/HaloBattery 快照，PID→型号关系已过时，不得作为实现依据；校正见文首“FerrisPulse 校正”。

以下 PID→TID 来自 OpenRazer `razer_attr_read_charge_level()`（commit `6820f9d`），HaloBattery 的表与其一致；这是基线 `70906c7` 时的快照，与当时的表除 0528/053A/0555/0556/0557/00A4 外基本一致。当前表已按 FerrisPulse 重写，本表只作研究历史。[4][8][14]

| TID | PID | 型号/连接（OpenRazer 名称） |
|---|---|---|
| 0x1F | 0062 | Atheris receiver |
| 0x1F | 006F / 0070 | Lancehead Wireless receiver / wired |
| 0x1F | 0077 / 0080 | Pro Click receiver / wired |
| 0x1F | 0086 / 0088 | Basilisk Ultimate wired / receiver |
| 0x1F | 008F / 0090 | Naga Pro wired / wireless |
| 0x1F | 0094 | Orochi V2 receiver（BT 0x0095 未列入） |
| 0x1F | 009A | Pro Click Mini receiver |
| 0x1F | 009C | DeathAdder V2 X HyperSpeed |
| 0x1F | 009E / 009F | Viper Mini SE wired / wireless |
| 0x1F | 00A5 / 00A6 | Viper V2 Pro wired / wireless |
| 0x1F | 00A7 / 00A8 | Naga V2 Pro wired / wireless |
| 0x1F | 00AA / 00AB | Basilisk V3 Pro wired / wireless |
| 0x1F | 00AF / 00B0 | Cobra Pro wired / wireless |
| 0x1F | 00B3 | HyperPolling Wireless Dongle |
| 0x1F | 00B4 | Naga V2 HyperSpeed receiver |
| 0x1F | 00B6 / 00B7 | DeathAdder V3 Pro wired / wireless |
| 0x1F | 00B8 | Viper V3 HyperSpeed |
| 0x1F | 00B9 | Basilisk V3 X HyperSpeed |
| 0x1F | 00BE / 00BF | DeathAdder V4 Pro wired / wireless |
| 0x1F | 00C0 / 00C1 | Viper V3 Pro wired / wireless |
| 0x1F | 00C2 / 00C3 | DeathAdder V3 Pro wired / wireless（ALT） |
| 0x1F | 00C4 / 00C5 | DeathAdder V3 HyperSpeed wired / wireless |
| 0x1F | 00C7 / 00C8 | Pro Click V2 Vertical Edition wired / wireless |
| 0x1F | 00CC / 00CD | Basilisk V3 Pro 35K wired / wireless |
| 0x1F | 00D0 / 00D1 | Pro Click V2 wired / wireless |
| 0x1F | 00D3 / 00D4 | Basilisk Mobile wired / receiver |
| 0x1F | 00D6 / 00D7 | Basilisk V3 Pro 35K Phantom Green wired / wireless |
| 0x3F | 0059 / 005A | Lancehead wired / wireless |
| 0x3F | 0072 / 0073 | Mamba Wireless receiver / wired |
| 0x3F | 007C / 007D | DeathAdder V2 Pro wired / wireless |
| 0xFF | 001F | Naga Epic |
| 0xFF | 0024 / 0025 | Mamba (2012) wired / wireless |
| 0xFF | 0032 | Ouroboros |
| 0xFF | 003E / 003F | Naga Epic Chroma / dock |
| 0xFF | 0044 / 0045 | Mamba wired / wireless |
| 0xFF | 007A / 007B | Viper Ultimate wired / wireless |
| 0xFF | 0083 | Basilisk X HyperSpeed |

**注意**：

- 0x0095 Orochi V2 Bluetooth 在 OpenRazer 有 battery 分支，但 HaloBattery 故意不列，因为 Windows 蓝牙属性已覆盖；当前也不列。不要盲目加。[8][14]
- 0x00CB Basilisk V3 35K 是 wired-only，OpenRazer 有 battery 分支但 HaloBattery 故意不列；当前也不列。[8][14]
- 0x00A4 Mouse Dock Pro：OpenRazer 把它放在 accessory driver，**没有** `charge_level`/`is_charging` 属性；HaloBattery 却把它列在 `KNOWN` 并给 TID 0x1F。当前也列了。它可能是作为接收器转发鼠标电量，但公开一手源码没有证明。标为 **未验证**，不要写 `Verified`。[8][13][42]

### B3. Razer 键盘矩阵

来源：OpenRazer `razer_attr_read_charge_level()` + `razer_get_report_params()`；HaloBattery 的 `KEYBOARD_INTERFACE` 与测试。[5][8][14]

| PID | 型号 | TID | interface |
|---|---|---|---|
| 0x025A / 0x025C | BlackWidow V3 Pro wired / wireless | 0x3F / 0x9F | 2 / 2 |
| 0x0258 / 0x0271 | BlackWidow V3 Mini HyperSpeed wired / wireless | 0x1F / 0x9F | 3 / 3 |
| 0x0290 / 0x0292 | DeathStalker V2 Pro wireless / wired | 0x9F / 0x1F | 2 / 3 |
| 0x0296 / 0x0298 | DeathStalker V2 Pro TKL wireless / wired | 0x9F / 0x1F | 2 / 3 |
| 0x02B9 / 0x02BA | BlackWidow V4 Mini HyperSpeed wired / wireless | 0x1F / 0x9F | 3 / 3 |
| 0x02D5 / 0x02D7 | BlackWidow V4 Tenkeyless HyperSpeed wireless / wired | 0x9F / 0x1F | 2 / 3 |

HaloBattery 的硬件验证：只有 0x0296（DeathStalker V2 Pro TKL receiver）确认；其余键盘未在硬件上测试。[41] 当前把 0x0296/0x0298/0x02D5/0x02D7 都标 `Verified`，其中 0x0298、0x02D5、0x02D7 属于过度声明。[当前 `vendor.rs:108-115`]

### B4. Razer 耳机特殊协议矩阵

| PID | 型号 | 协议 | report ID | 请求 | 回复/字段 | 证据 |
|---|---|---|---|---|---|---|
| 0x0528 | BlackShark V2 Pro (2020) | 64 字节 Feature `0xFF` | 0xFF | `FF 0A 00 FD 04 12 F1 02 05` + zeros | prefix `FF 0F 05 FE 12 04 1F 08 05`; state@11; mV@12..13 BE; level@14; no-reply prefix `FF 01 00 FE...` | HaloBattery 2020 专用实现 + 硬件；OpenRazer issue #1280 [10][39] |
| 0x0555 / 0x0556 | BlackShark V2 Pro 2.4 (2023) | PA，64 字节 output/input | 0x02 | `[2]=8, [5]='P',[6]='A',[7]=8,[9]=3,[10]=0x21` | reply `[5]='P',[6]='I'`; cmd echo @12; ACK @13; len @14; level @15；充电 cmd `0x2A` | HaloBattery PA + 未合并 OpenRazer PR #2862；0555 的 USB ID 见 [33][11][34] |
| 0x053A | Barracuda Pro (2.4 GHz) | PA，64 字节 output/input | 0x01 | `[2]=8,[3]='P',[4]='A',[5]=8,[6]=8,[7]=3,[8]=0x21` | reply `[3]='P',[4]='I'`; cmd echo @13; ACK @14; len @15; level @16；充电 `0x2A` | HaloBattery Barracuda + USBPcap + 硬件 [12] |
| 0x0557 | BlackShark V2 X USB（有线） | 无电池协议 | — | — | — | linux-hardware USB ID；不是 2023 Pro [32] |

**0x0556 未决**：HaloBattery `PA_PIDS` 包含 0x0556，但公开 USB ID 数据库没有独立条目可核实；当前把它标 `Verified` 没有依据。应标 `Community/Unverified`，等 USB descriptor 或真机。

**0x0528 请求向量**（64 字节）：

```text
FF 0A 00 FD 04 12 F1 02 05 00 00 ... 00
```

**0x0528 回复向量**（HaloBattery 实机记录）：

```text
on battery:      FF 0F 05 FE 12 04 1F 08 05 03 05 01 0E A0 32 ...
charging:        FF 0F 05 FE 12 04 1F 08 05 05 03 09 10 80 50 ...
charge complete: FF 0F 05 FE 12 04 1F 08 05 06 05 06 10 88 64 ...
no reply:        FF 01 00 FE 12 04 1F 08 05 ...
```

解析：state=`[11]`，mV=`([12]<<8)|[13]`，level=`[14]`。上面的 16 字节里 `[11]=01/09/06`，`[12:14]=0E A0`（3744mV），`[14]=32`（50%）。注意：2020 的设备自报电量是粗档位（观察值 100→80→50→30→10），不是精确百分比；充电中 state=0x09 时 HaloBattery 会隐藏不可靠的电量，只显示充电。[9][10]

**PA 2023 请求向量**（64 字节，report id 2）：

```text
02 80 08 00 00 50 41 08 00 03 21 00 ...
```

远程唤醒：

```text
02 80 07 00 00 50 41 0E 00 02 E1 01 ...
```

**PA Barracuda 请求向量**（report id 1）：

```text
01 80 08 50 41 08 08 03 21 00 ...
```

远程唤醒：

```text
01 80 07 50 41 0E 08 02 E1 01 ...
```

PA 的 ACK 形状：`cmd echo` 后 `0x01` = reply，`0x02` = notification；长度在下一字节；数据从再下一字节开始。当前 parser 接受 `0x01/0x02`，但没有校验 `P/I`，也没有校验 report ID 与 command 的完整位置，存在误收风险。[当前 `extra.rs:310-324`; 12]

---

### B5. Logitech HID++ 2.0 报文/接收器

官方 HID++ 2.0 文档；Solaar 的实现见 [17][20]：

| byte | 字段 |
|---:|---|
| 0 | report ID：0x10 short（7 字节）/ 0x11 long（20 字节） |
| 1 | device index；0xFF = corded / receiver |
| 2 | feature index |
| 3 | function（高 4 bit） + software ID（低 4 bit） |
| 4.. | parameters |

[24][25]

- Feature discovery：向 feature 0（IRoot）fn0 发 feature ID 高/低字节，返回 feature index。[24][25]
- 错误回复：short `10 <idx> 8F <feature> <func> <code>`，long `11 <idx> FF <feature> <func> <code>`；Solaar 会同时匹配 feature/function 再判错。[20]
- 软件 ID：Solaar 每次请求递增，并在 bit3 置 1，避免把 notification 当 reply；HaloBattery 用 0x0A..0x0F 轮转，并明确说可与 G HUB 同时运行。[13][20]
- 接收器 device index：1..6（Bolt max 6；Unifying/Nano/Lightspeed 多数 1..6；Solaar 的 `__iter__` 会探到 7，`__getitem__` 允许到 15）。[13][21]
- 直连设备：0xFF。[24]

**接收器 PID 矩阵（Solaar `base_usb.py`）**：

| 类型 | PID | 说明 |
|---|---|---|
| Bolt | C548 | max_devices=6 |
| Unifying | C52B, C532 |  |
| Nano | C518, C51A, C51B, C521, C525, C526, C52E, C52F, C531, C534, C535, C537 | C534 max 2 |
| Lightspeed | C539, C53A, C53D, C53F, C541, C545, C547, C54D | 当前缺 C545/C547/C54D |
| 27MHz | C517 | 非 HID++，当前不需要 |
| Lenovo | 6042（vendor 6127） | 非 Logitech VID |

[21][当前 `vendor.rs:524-527`]

### B6. Logitech 电池 feature 精确 offset/缩放

| feature | fn | 请求 | 回复 | 语义/当前问题 |
|---|---:|---|---|---|
| 0x1004 Unified Battery | 0 | capabilities | byte0 bit0..3 critical/low/good/full；byte1 bit0 rechargeable，bit1 percentage gauge | 当前未读；无 gauge 时不能把 byte0 当百分比 [29][31] |
| 0x1004 | 1 | get info | byte0 pct；byte1 bucket 1/2/4/8；byte2 status 0..7；byte3 external power | 当前只读 0/2，缺 bucket；status 4 语义冲突 [29][31] |
| 0x1000 Battery Status | 0 | get level/status | byte0 pct（0 在放电时可为合法空电）；byte1 next；byte2 status 0..7 | 当前 0 被丢；status=3 full 时未补 100 [25][27][31] |
| 0x1001 Battery Voltage | 0 | get voltage | byte0..1 mV BE；byte2 flags：bit7 external power，bits0..2 status，bit3 fast，bit4 slow，bit5 critical | 当前只看 bit7；0x82 会误判充电 [28][31] |
| 0x1F20 ADC Measurement | 0 | get ADC | byte0..1 mV BE；byte2 flags 0x01/0x03/0x07（0x0F/default unknown） | 当前接受 0x0F；曲线冲突 [30][31] |

**0x1004/0x1000 status 枚举**：

```text
0 discharging
1 charging / recharging
2 charging nearly full / final stage
3 full
4 slow charging（Solaar/OpenLogi）或 charging error（Linux kernel）  <-- 冲突
5 invalid battery
6 thermal error
7 other charging error
```

[18][29][31]

**Logitech 1.0 register**：

| register | 请求（short） | 回复 data | 解析 |
|---|---|---|---|
| 0x0D BATTERY_CHARGE | `10 <slot> 81 0D 00 00 00` | data[0]=pct，data[2] 高半字节 | 0x30 discharging，0x50 recharging，0x90 full；pct 必须 1..100 |
| 0x07 BATTERY_STATUS | `10 <slot> 81 07 00 00 00` | data[0]=level，data[1]=charging byte | 7 full(90%)，5 good(50%)，3 low(20%)，1 critical(5%)；0x21=recharging，0x22=full |

[19][13]

### B7. Logitech 耳机矩阵（公开实现；HeadsetControl 见 [22][23]）

| PID | 型号 | collection | 电池命令 | 备注 |
|---|---|---|---|---|
| 0x0A66 | G533 | `FF43:0202` | `11 FF 07 01` | HeadsetControl 固定 feature index 7/fn1；电压 [4:6]，状态 [6] [23] |
| 0x0AC4 | G535 | `000C:0001` | `11 FF 05 0D` | 当前 `usage_page<0xFF00` 直接排除 [13][23] |
| 0x0A5C / 0x0A89 / 0x0A5B / 0x0A87 | G633/G635/G933/G935 | `FF43:0202` | `11 FF 08 0A` | 与 G733 同族；当前无固定命令 [23] |
| 0x0AB5 / 0x0AFE / 0x0B1F | G733 variants | `FF43:0202` | `11 FF 08 0A` | 同上 |
| 0x0AA7 / 0x0AAA / 0x0ABA / 0x0AFB / 0x0AFC | G PRO / G PRO X / G PRO X 2 | 多数 `FF43:0202`；PRO X 2 可能 Centurion `FFA0:0001` report 0x51 | 不是统一 HID++ 电池路径 | HaloBattery 另有 Centurion 实现；本 lane 只标出边界 [9] |

**G533/G535/G633 的电压曲线**：HeadsetControl 用各型号自己的校准表；Linux 内核对 0x1F20 使用 3399..4030 mV 的 100 点曲线；Solaar/OpenLogi/HaloBattery 用 3500..4186 mV 的 13 点曲线。三者在 3905 mV 分别约 87%（按 Linux 内核曲线计算）和 67%（Solaar/OpenLogi）。这是公开实现的真实冲突，不是笔误。[23][30][31]

---

## C. 与当前代码逐项差距

### C1. Razer

| # | 当前代码 | 问题 | 证据 | 严重度 |
|---|---|---|---|---|
| R1 | `razer_name` 0528 → 90 字节，Verified | 0528 实际是 64 字节 `0xFF`，当前无法读；且 0528 硬件拒绝 generic/PA | [10][8][39] | P0 假支持 |
| R2 | `razer_name` 0557 → BlackShark V2 Pro 2023，Verified | 1532:0557 是 BlackShark V2 X USB（有线） | [32] | P0 错型号 |
| R3 | 排除列表只排 053A/0555/0556 | 0557 在满足 `feature_len>=90`/vendor collection 时会进 `read_razer` 候选路径，可能向有线音频设备发 90 字节 | 当前 `vendor.rs:53-59` | P0 |
| R4 | `read_razer_pa` 只读 0x21，charging=None，且不 remote off | PA 有 0x2A；0555/0556/053A 都支持；HaloBattery 最后会 remote off | [11][12] | P1 |
| R5 | PA parser 只看 cmd/len，不验 `P/I` | 可能把别的 64 字节包当电量 | 当前 `extra.rs:310-324` | P1 |
| R6 | `build_razer_request` 固定 91 | 90 字节 FeatureReportByteLength 会失败 | [16] | P1 |
| R7 | 无 interface 选择 | OpenRazer 键盘 2/3、鼠标 0/3；当前可能先探错 collection | [4][5][8] | P1 |
| R8 | 外回报文不重发 | Synapse/Chroma 可覆盖回复；HaloBattery 重发 3 次 | [8][37] | P1 |
| R9 | 不校验 CRC/len/tid' | 可能接受陈旧/错包；当前只对 class/id | [2][3] | P2 |
| R10 | TID 列表含 0x08 | 无一手 PID 依据 | [4][8] | P2 |
| R11 | 02D5/02D7、0298、00BE 标 Verified | 公开硬件只确认 0296（TKL receiver）和 00BF（V4 Pro 无线）；其余未测 | [41] | P2 过度声明 |
| R12 | 0555/0556 标 Verified | 协议来源是未合并 PR #2862；至少应 Community | [34] | P2 |
| R13 | 0528/0557 之外，表基本与 HaloBattery 一致 | 说明错误是继承来的；修正时不要只改一个 PID | [8] | 说明 |
| R14 | 无 Razer parser 单测 | 只有 AULA 测试；Razer/Logitech 无测试 | 当前 `vendor.rs:735-780` | P1 |

### C2. Logitech

| # | 当前代码 | 问题 | 证据 | 严重度 |
|---|---|---|---|---|
| L1 | `LOGITECH_RECEIVERS` 缺 C545/C547/C54D | Lightspeed 接收器可能被当直连，漏 1..6 槽 | [21] | P1 |
| L2 | `read_logitech` 只打开一个 path | 接收器错误回复常在 short collection；空槽/离线会超时 | [13] | P1 |
| L3 | `hpp_request` 固定 SWID 0x0A | 与 G HUB/其他 app 共用时可能串包；应轮转 | [13][20] | P2 |
| L4 | 无 1.0 回退 | 老设备无 feature table，`0x01` 错误后直接放弃 | [19] | P1 |
| L5 | 0x1004 不读 capability、不处理 bucket | 无 SoC 设备会显示未知 | [29][31] | P1 |
| L6 | 0x1000 丢弃 0；full 不补 100 | 0% 放电/满电 byte0=0 会错误 | [25][31] | P1 |
| L7 | 0x1001 只看 bit7 | 外接电源但 status=2 被误判充电 | [31] | P1 |
| L8 | 0x1F20 接受 0x0F/未知 flags | 内核只接受 0x01/0x03/0x07 | [31] | P1 |
| L9 | 0x1F20 用鼠标 13 点曲线 | 与内核耳机曲线差异大；与 OpenLogi 一致但与 Linux 冲突 | [30][31] | P1 需校准 |
| L10 | status 4 当充电；5..7 当不充电 | Solaar/OpenLogi 说 4 慢充，内核说 4 错误；5..7 是错误码，不能简单当 false | [18][29][31] | P1 语义冲突 |
| L11 | 0x0003 unit ID 未读 | 接收器/线同一设备可能两张卡 | [13][18] | P2 |
| L12 | 0x0005 name 短回复也 pad 16 | short collection 名称可能夹 NUL/截断 | [24] | P2 |
| L13 | 0x0005 type 2/4/5 映射为 keyboard/mouse | 官方是 numpad/touchpad/trackball；UI 近似 | [25][26] | P2 |
| L14 | G535 000C 被 `usage_page<0xFF00` 排除 | G535 直接不支持 | [13][23] | P1 |
| L15 | G533/G633 无固定命令 | 通用 0x1F20 发现可能找不到 | [23] | P1 |
| L16 | 无 asleep 缓存 | 设备休眠即消失；HaloBattery 保留 300s | [13] | P2 |
| L17 | 无 feature index 缓存 | 每次扫描重做 discovery，慢 | [13] | P2 |
| L18 | 350ms 超时偏短 | HaloBattery 0.6s/2s；休眠设备可能来不及 | [13] | P2 |
| L19 | 无 Logitech parser 单测 | 没有 1004/1000/1001/1F20/1.0 向量 | 当前 `vendor.rs` | P1 |

### C3. 当前 `read_razer` 控制流的具体问题

```rust
for tid in [preferred_tid, 0x1F, 0x9F, 0x3F, 0xFF, 0x08] {
    if let Some((status, raw)) = razer_query(info, tid, 0x07, 0x80) {
        if status == 0x02 { ... }
    }
}
```

[当前 `vendor.rs:246-257`]

- 如果 `preferred_tid` 错误，会依次试 6 个 TID；每个 TID 都 `SetFeature` + 1 秒轮询，最坏 6 秒，且对每个 HID collection 都重复。
- 如果第一个 TID 返回 `status=0x02` 但 raw 不在 0..100，它会继续试下一个 TID；没有记录“这个接口曾经回答过”的缓存。
- 如果外回报文持续 `status=0x02`，`razer_query` 会一直读到 deadline；不重发。
- 如果 `feature_set` 因长度 91 失败，它不会尝试 90。

---

## D. 实施候选排序

排序原则：证据强度 > 只读/低风险 > 收益 > 是否需要真机。

| 排名 | 候选 | 证据 | 只读 | 收益 | 风险 |
|---:|---|---|---|---|---|
| 1 | Razer 表修正：0528 改协议、0557 删除、0556/0298/02D5/02D7/00BE 降级 | 强 | 是 | 消除假支持/错型号 | 零 |
| 2 | Logitech 接收器列表补 C545/C547/C54D | 强 | 是 | 修复 Lightspeed 漏槽 | 零 |
| 3 | Razer/Logitech parser 单测 | 强 | 是 | 防回归 | 零 |
| 4 | Razer 0528 64 字节 `0xFF` parser | 强（实机） | 是 | 修复一个完全错误的型号 | 低 |
| 5 | Razer PA 0x2A + `P/I`/长度校验 | 强 | 是 | 充电状态 | 低 |
| 6 | Razer 90/91 + interface + 外报重发 | 强 | 是 | 偶发失败 | 中 |
| 7 | Logitech short+long/SWID/error/asleep 通道化 | 强 | 是 | 稳定性/速度 | 中 |
| 8 | Logitech 1.0 回退 | 强 | 是 | 老设备覆盖 | 低 |
| 9 | Logitech 0x1004/0x1000/0x1001/0x1F20 解析修正 | 强 | 是 | 减少误报 | 低-中 |
| 10 | Logitech unit ID / 跨接收器合并 | 中 | 是 | 去重 | 低 |
| 11 | G535/G533 专用分支 | 中 | 是 | 耳机覆盖 | 中 |
| 12 | 同句柄 A/B | 弱（推断） | 是 | 澄清假设 | 低 |

### D1. 立即做（无需硬件，纯数据/表/parser 防线）

1. **修正 Razer 型号表与协议路由**
   - 0528 从 90 字节表移出，改走 64 字节 `0xFF` 专用 parser；或在实现前先标 Unsupported，不要标 Verified。
   - 0557 删除“BlackShark V2 Pro 2023”映射，改为 `BlackShark V2 X USB (wired)` 并禁止电池探测。
   - 0556 降为 `Community/Unverified`。
   - 0298、02D5、02D7、00BE 从 `Verified` 降为 `Community`，除非有本机日志。
   - 证据：[10][32][41]；风险：零硬件风险；收益：消除假支持/错型号。

2. **修正 Logitech 接收器 PID 表**
   - 加 C545/C547/C54D。
   - 证据：[21]；风险：零；收益：G502 X PLUS 等 Lightspeed 接收器不再漏槽。

3. **给 Razer/Logitech 加 parser 单测**
   - 用本文的 90 字节、0528、PA、1004/1000/1001/1F20、1.0 向量。
   - 不涉及硬件；能防止后续回归。

### D2. 高收益只读实现（需要一次真机验证）

4. **Razer 0528 64 字节 `0xFF` parser**
   - 请求/回复/状态/陈旧前缀严格匹配；充电中隐藏不可靠 level。
   - 证据强（HaloBattery 实机 + issue #1280）；只读；收益：修复一个当前完全错误的“支持”型号。
   - 风险：只对 0528；不要套到 0555/0556。

5. **Razer PA 充电 + 严格校验**
   - 对 053A/0555/0556 先 remote on，再 `0x21`，再 `0x2A`，最后 remote off；校验 `P/I`、report ID、cmd echo、长度、0..100。
   - 证据：[11][12]；只读；收益：充电状态；风险：低，但 0556 未确认。

6. **Razer 90/91 长度适配 + interface 优先级 + 外报重发**
   - 用 `info.feature_len` 决定 90/91；按 PID 选 interface（键盘 2/3，鼠标 0/3）；外报每 4 个重发，最多 3 次；严格 CRC/len/status。
   - 证据：[2][4][5][8][16]；收益：解决“某些型号偶发失败”；风险：中，需要真机 A/B。

### D3. Logitech 结构性改造（收益最大，风险中等）

7. **把 Logitech 读取改成“通道”模型**
   - 同时打开 `FF00:0001` short 和 `FF00:0002` long；每个请求同时读 short/long；轮转 SWID 0x0A..0x0F；识别 0x08/0x09/0x04/0x01 错误码；缓存 feature index 和 asleep 状态。
   - 证据：[13][20][21]；收益：空槽/离线/休眠/多接收器稳定；风险：中，需要真机确认 short/long 读取顺序。

8. **补 Logitech 1.0 回退**
   - 2.0 无 feature table 或错误码 0x01 时，读 `0x0D`/`0x07` 寄存器；只读。
   - 证据：[19]；收益：老设备；风险：低。

9. **修正 Logitech battery 解析**
   - 0x1004 先读 capability；无 gauge 用 bucket；0x1000 允许 0 并处理 full；0x1001 解析 bits0..2；0x1F20 只接受 0x01/0x03/0x07；status 5..7 标 unknown；status 4 标 uncertain。
   - 证据：[25][29][30][31]；收益：减少误报；风险：低-中（status 4/曲线需真机）。

10. **补 G535/G533 专用分支**
    - G535 `000C:0001` + `11 FF 05 0D`；G533 `FF43:0202` + `11 FF 07 01`；G633 族 `11 FF 08 0A`。
    - 证据：[23]；收益：耳机覆盖；风险：中，需要对应硬件。

### D4. 暂缓/只做诊断

11. **同句柄 A/B**
    - 不作为默认修复；只在日志中同时测试“同句柄”和“重开句柄”，用同一设备/同一 TID 比较成功率。
    - 证据：参考实现同句柄，但 API 文档不要求；风险：低；收益：澄清假设。

12. **未知 PID 的 0x08 TID**
    - 删除或仅允许显式配置；不要对所有未知 Razer 设备盲试。

---

## E. 需要主代理串行实机验证的具体步骤

> 原则：一次只接一个设备；关闭 Synapse/G HUB/Chroma；只发读命令；不装驱动、不写固件、不 fuzz。

### E1. 先收集描述符（只读）

对目标设备记录：

- VID/PID、product string、manufacturer；
- 每个 HID collection 的 `interface_number`、`usage_page/usage`、`input/output/feature` 长度；
- container ID / instance ID；
- 设备是接收器、线缆还是直连；
- 当前 AnyBattery 诊断页原始输出。

Razer 先按 PID 分支，不要先盲试所有 TID。

### E2. Razer 分支树

**分支 A：PID 0x0528**

1. 关闭 Synapse。
2. 只对 `usage_page=0xFF00, usage=0x01` 的 collection 打开句柄。
3. `SetFeature` 64 字节 `FF 0A 00 FD 04 12 F1 02 05` + zeros。
4. 等 100ms，`GetFeature(0xFF, 64)`，最多 0.5s。
5. 只接受 prefix `FF 0F 05 FE 12 04 1F 08 05`；记录 state/mV/level；拒绝 `FF 01 00...`。
6. 预期：state 0x01/0x02/0x03/0x06/0x09；mV 12..13；level 14。
7. 不要发 90 字节 0x07:0x80；不要发 PA。

**分支 B：PID 0x0555/0x0556（PA 2023）**

1. 找 `usage_page=0xFF00` 的 collection，report length 64。
2. 发 remote on：`02 80 07 00 00 50 41 0E 00 02 E1 01`。
3. 等 35ms；发 `0x21` query：`02 80 08 00 00 50 41 08 00 03 21`。
4. 读 64 字节，校验 `[5:7]='PI'`、`[12]=0x21`、`[13]=0x01/0x02`、`[14]=len`、`[15]=level`。
5. 再发 `0x2A` query（同样 shape，cmd=0x2A），payload[0] 0/1 为充电。
6. 最后 remote off（cmd 0xE1，payload 0）。
7. 若失败：记录“remote 是否被接受”“是否有 PI”“是否只有 echo”“是否被 Synapse 占用”。

**分支 C：PID 0x053A（PA Barracuda）**

同上，但 report ID 1：
- remote：`01 80 07 50 41 0E 08 02 E1 01`
- query：`01 80 08 50 41 08 08 03 21`
- reply command echo @13，ACK @14，len @15，level @16。

**分支 D：PID 0x0557**

停止。该 ID 是 BlackShark V2 X USB（有线），不应有无线电池协议；如果系统蓝牙/有线属性显示电量，走系统属性，不走 Razer 90 字节。

**分支 E：键盘 PID**

1. 找 `interface_number` 2 或 3 的 collection；按 B3 表选 TID。
2. 只发 90 字节 class 0x07 id 0x80，再 0x84。
3. 先试 91 字节；若 `SetFeature` 失败，按 `feature_len` 试 90。
4. 预期回复 `status=0x02`、class 0x07、id 0x80、arg1 0..255；充电 arg1 0/1。
5. 如果只收到 `status=0x04`，是接收器在但键盘睡眠；唤醒键盘再试。
6. 如果只收到 `status=0x02` 但 class/id 不是 0x07:0x80，记录外报，重发最多 3 次。

**分支 F：鼠标 PID**

1. 优先 interface 0；若失败试 interface 3（OpenRazer 对 Naga X/Basilisk V3 等用 3）。
2. 按 B2 表选 TID；不要先试 0x08。
3. 91/90 长度 A/B；同句柄/重开句柄 A/B。
4. 关闭 Synapse 再试；若外报多，重发。
5. 记录 `status=0x02` 的 raw、`0x84` 的 raw、CRC、tid'。

**分支 G：未知 PID**

- 不要盲试 6 个 TID × 所有 collection。
- 先看 product string 是否包含 `hyperspeed/wireless/receiver/dongle/blackshark/barracuda`；没有则大概率 wired-only，跳过。
- 若必须试，最多 3 个 TID（0x1F/0x3F/0xFF），每个一次，严格匹配 class/id/len/CRC；失败即停。

### E3. Logitech 验证

1. 枚举 `VID_046D`；按 `FF00:0001`（short）和 `FF00:0002`（long）分组；记录 receiver PID。
2. 同时打开 short+long；对 slot 1..6：
   - 先 ping：root fn1；
   - 记录错误码：0x08 空槽、0x09 离线、0x01 HID++1.0、0x04 connect fail、0x05 Logitech internal；
   - 特征发现：root fn0 `10 04`（0x1004）、`10 00`、`10 01`、`1F 20`；
   - 0x1004：fn0 capabilities，再 fn1 info；
   - 0x1000：fn0；
   - 0x1001：fn0；
   - 0x1F20：fn0。
3. 对每个成功读数记录原始 payload、feature index、slot、unit ID（0x0003 fn0 bytes1..5）。
4. 对 HID++1.0 设备（0x01）改读 `10 <slot> 81 0D` 和 `81 07`。
5. 对 G535（0x0AC4）单独用 `000C:0001` + `11 FF 05 0D`；对 G533（0x0A66）用 `FF43:0202` + `11 FF 07 01`；对 G633 族用 `11 FF 08 0A`。
6. 与 G HUB 读数对照；特别记录 0x1001 flags=0x82、0x1F20 flags=0x0F、status=4 的实际含义。
7. 记录休眠 30s/2min 后是否还能读；若不能，验证 HaloBattery 式“保留最后值 300s”是否符合产品预期。

### E4. 验收门槛

- Razer 0528：必须看到 `FF 0F...` 新鲜前缀，且 level 与官方/充电状态同步；不能只看“有回复”。
- Razer PA：必须同时拿到 0x21 和 0x2A，且 remote off 后设备仍正常。
- Razer 90 字节：必须严格 class/id/status/CRC；外报不得显示为电量。
- Logitech 2.0：必须通过 feature discovery 找到 feature index；不能把固定 index 硬编码到所有设备。
- Logitech 1.0：必须只在 2.0 无 feature table 时回退；不要对 2.0 设备盲读寄存器。
- 所有新 parser 先过离线单测，再上真机；失败时保留“未知”，不显示猜测百分比。

---

## F. 完整来源清单

编号与正文引用一致；每条含 URL、commit/日期、许可证/类型。

1. **OpenRazer**（GPL-2.0-or-later）— https://github.com/openrazer/openrazer/tree/6820f9da169d354bc7e6e93a0aa8683a6bb75792  
   commit `6820f9da169d354bc7e6e93a0aa8683a6bb75792`，2026-05-02。
2. **OpenRazer `driver/razercommon.h`** — https://github.com/openrazer/openrazer/blob/6820f9da169d354bc7e6e93a0aa8683a6bb75792/driver/razercommon.h#L78-L136  
   `struct razer_report` 90 字节、status 枚举、transaction/command union。
3. **OpenRazer `driver/razercommon.c`** — https://github.com/openrazer/openrazer/blob/6820f9da169d354bc7e6e93a0aa8683a6bb75792/driver/razercommon.c#L63-L143  
   USB control transfer、CRC XOR bytes2..87、`get_razer_report()`。
4. **OpenRazer `driver/razermouse_driver.c`** — https://github.com/openrazer/openrazer/blob/6820f9da169d354bc7e6e93a0aa8683a6bb75792/driver/razermouse_driver.c#L33-L140  
   鼠标 report index/等待；battery/charging switch @L1532-L1730。
5. **OpenRazer `driver/razerkbd_driver.c`** — https://github.com/openrazer/openrazer/blob/6820f9da169d354bc7e6e93a0aa8683a6bb75792/driver/razerkbd_driver.c#L321-L400  
   `razer_get_report_params()` interface 2/3；battery @L663-L751。
6. **OpenRazer `driver/razerchromacommon.c`** — https://github.com/openrazer/openrazer/blob/6820f9da169d354bc7e6e93a0aa8683a6bb75792/driver/razerchromacommon.c#L1052-L1070  
   `razer_chroma_misc_get_battery_level()` / `get_charging_status()`。
7. **HaloBattery**（MIT）— https://github.com/HeyOkay/HaloBattery/tree/2251f0534a8e886c9692ead42056059b4f69d8c0  
   commit `2251f0534a8e886c9692ead42056059b4f69d8c0`。
8. **HaloBattery `providers/razer.py`** — https://github.com/HeyOkay/HaloBattery/blob/2251f0534a8e886c9692ead42056059b4f69d8c0/providers/razer.py#L1-L307  
   90 字节格式、TID 表、`_query` 外报重发、`_read` 同句柄。
9. **HaloBattery `docs/protocols.md`** — https://github.com/HeyOkay/HaloBattery/blob/2251f0534a8e886c9692ead42056059b4f69d8c0/docs/protocols.md#L75-L107  
   Barracuda/0528/0555 协议；Logitech 段 @L290-L312、@L368-L374。
10. **HaloBattery `providers/blackshark2020.py`** — https://github.com/HeyOkay/HaloBattery/blob/2251f0534a8e886c9692ead42056059b4f69d8c0/providers/blackshark2020.py#L43-L127  
    0528 请求/回复/状态/陈旧前缀。
11. **HaloBattery `providers/blackshark.py`** — https://github.com/HeyOkay/HaloBattery/blob/2251f0534a8e886c9692ead42056059b4f69d8c0/providers/blackshark.py#L28-L218  
    0555/0556 PA、0x21/0x2A、remote on/off。
12. **HaloBattery `providers/barracuda.py`** — https://github.com/HeyOkay/HaloBattery/blob/2251f0534a8e886c9692ead42056059b4f69d8c0/providers/barracuda.py#L34-L222  
    053A PA offset、0x21/0x2A。
13. **HaloBattery `providers/logitech.py`** — https://github.com/HeyOkay/HaloBattery/blob/2251f0534a8e886c9692ead42056059b4f69d8c0/providers/logitech.py#L53-L516  
    HID++ 2.0/1.0、short+long、SWID、接收器、heads、unit ID。
14. **HaloBattery Razer tests** — https://github.com/HeyOkay/HaloBattery/blob/2251f0534a8e886c9692ead42056059b4f69d8c0/tests/test_razer.py  
    以及 `tests/test_razer_keyboards.py`、`tests/test_razer_foreign.py`。
15. **HaloBattery Logitech tests** — https://github.com/HeyOkay/HaloBattery/blob/2251f0534a8e886c9692ead42056059b4f69d8c0/tests/test_logitech.py  
    解析向量、short/long、1.0。
16. **FerrisPulse**（GPL-3.0）— https://github.com/Ferris-echo/FerrisPulse/tree/4917a10e238ed3dba2938969752886353c8ff5e9  
    `src/Hardware/RazerDeviceHelper.cs` @L1008-L1165；commit `4917a10e...`。
17. **Solaar**（GPL-2.0-or-later）— https://github.com/pwr-Solaar/Solaar/tree/e7304c4c451cc9bb4f206a914844525e67856a28  
    commit `e7304c4c451cc9bb4f206a914844525e67856a28`，2026-08-17。
18. **Solaar `lib/logitech_receiver/hidpp20.py`** — https://github.com/pwr-Solaar/Solaar/blob/e7304c4c451cc9bb4f206a914844525e67856a28/lib/logitech_receiver/hidpp20.py#L2182-L2318  
    0x1000/0x1001/0x1004/0x1F20 parser、13 点电压曲线、device type。
19. **Solaar `lib/logitech_receiver/hidpp10.py`** — https://github.com/pwr-Solaar/Solaar/blob/e7304c4c451cc9bb4f206a914844525e67856a28/lib/logitech_receiver/hidpp10.py#L56-L112  
    1.0 register 0x07/0x0D、parse_battery_status @L230-L287。
20. **Solaar `lib/logitech_receiver/base.py`** — https://github.com/pwr-Solaar/Solaar/blob/e7304c4c451cc9bb4f206a914844525e67856a28/lib/logitech_receiver/base.py#L90-L133  
    HID++ short/long、SWID、error 匹配 @L672-L800。
21. **Solaar `lib/logitech_receiver/base_usb.py`** — https://github.com/pwr-Solaar/Solaar/blob/e7304c4c451cc9bb4f206a914844525e67856a28/lib/logitech_receiver/base_usb.py#L47-L208  
    接收器 PID/类型/max_devices；`receiver.py` slots @L340-L379。
22. **HeadsetControl**（GPL-3.0）— https://github.com/Sapd/HeadsetControl/tree/8b4b711bdeacf91a867e68409e6668e83b49f62e  
    commit `8b4b711bdeacf91a867e68409e6668e83b49f62e`，2026-10-26。
23. **HeadsetControl Logitech devices** — https://github.com/Sapd/HeadsetControl/blob/8b4b711bdeacf91a867e68409e6668e83b49f62e/lib/devices/logitech_g533.hpp  
    `logitech_g535.hpp`、`logitech_g633_g933_935.hpp`、`lib/devices/protocols/hidpp_protocol.hpp`。
24. **Logitech cpg-docs** — https://github.com/Logitech/cpg-docs/tree/f9107a4e3190d798f9be8c5d596f255f5e5741d2  
    `hidpp20/README.rst`、`hidpp20/features/0x0000-IRoot.rst`；官方 HID++ 2.0 文档。
25. **Logitech HID++2.0 Draft Specification**（2012-06-04）— https://lekensteyn.nl/files/logitech/logitech_hidpp_2.0_specification_draft_2012-06-04.pdf  
    0x0005 device type、0x1000 battery level/status/capability；PDF 文本第 617-760 行。
26. **OpenLogi 0x0005** — https://openlogi.org/hidpp/features/x0005-device-type-and-name  
    设备类型枚举（含 Headset=8）。
27. **OpenLogi 0x1000** — https://openlogi.org/hidpp/features/x1000-battery-status  
    byte0/1/2、status 0..7、capability。
28. **OpenLogi 0x1001** — https://openlogi.org/hidpp/features/x1001-battery-voltage  
    flags bit7/bits0..2/bit3/4/5。
29. **OpenLogi 0x1004** — https://openlogi.org/hidpp/features/x1004-unified-battery  
    fn0 capabilities、fn1 info、status 枚举。
30. **OpenLogi 0x1F20** — https://openlogi.org/hidpp/features/x1f20-adc-measurement  
    flags 0x01/0x03/0x07、3905mV→67% 示例、G733。
31. **Linux kernel `hid-logitech-hidpp.c`** — https://github.com/torvalds/linux/blob/d135ab453a7400a34930bdda1ef4bdc8acfac80e/drivers/hid/hid-logitech-hidpp.c  
    commit `d135ab453a7400a34930bdda1ef4bdc8acfac80e`；0x1000 @L1183-L1350、0x1001 @L1388-L1560、0x1004 @L1562-L1772、0x1F20 @L1853-L1931。
32. **linux-hardware 1532:0557** — https://linux-hardware.org/?id=usb%3A1532-0557  
    Razer BlackShark V2 X USB，class 01-01-00（audio）。
33. **linux-hardware 1532:0555** — https://linux-hardware.org/?id=usb%3A1532-0555  
    Razer BlackShark V2 Pro 2.4。
34. **OpenRazer PR #2862** — https://github.com/openrazer/openrazer/pull/2862  
    “Add support for the Razer BlackShark V2 Pro 2.4 (1532:0555)”，截至研究时 **open/unmerged**。
35. **Microsoft `HidD_SetFeature`** — https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/hidsdi/nf-hidsdi-hidd_setfeature  
    向 top-level collection 发送 feature report。
36. **Microsoft `HidD_GetFeature`** — https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/hidsdi/nf-hidsdi-hidd_getfeature  
    从 top-level collection 返回 feature report。
37. **HaloBattery issue #108** — https://github.com/HeyOkay/HaloBattery/issues/108  
    Synapse/Chroma 占用同一 collection 的外回报文。
38. **libratbag**（MIT）— https://github.com/libratbag/libratbag/tree/8235b5bb6032dea15901ab58a8218956f4494d08  
    检查 commit 内未发现 0x1000/0x1001/0x1004/0x1F20 的电池读取实现；其 HID++ 库主要用于配置，因此不作为电池 offset 证据。
39. **OpenRazer issue #1280** — https://github.com/openrazer/openrazer/issues/1280  
    BlackShark V2 Pro 0528 的社区协议讨论。
40. **HaloBattery CHANGELOG** — https://github.com/HeyOkay/HaloBattery/blob/2251f0534a8e886c9692ead42056059b4f69d8c0/CHANGELOG.md  
    00BF 实机向量 `02 1f ... ab`；0528 说明；1.0 回退。
41. **HaloBattery `docs/devices.md`** — https://github.com/HeyOkay/HaloBattery/blob/2251f0534a8e886c9692ead42056059b4f69d8c0/docs/devices.md#L40-L49  
    哪些型号真正在硬件上验证。
42. **OpenRazer `driver/razeraccessory_driver.c`** — https://github.com/openrazer/openrazer/blob/6820f9da169d354bc7e6e93a0aa8683a6bb75792/driver/razeraccessory_driver.c  
    0x00A4 在 accessory driver 中；检查该文件未发现 `charge_level`/`is_charging` 电池属性。

---

## Coverage Status

### 已直接检查

- AnyBattery 工作树：`vendor.rs`、`extra.rs`、`hid.rs`、`scan.rs`、`README.md`、`docs/protocol-adaptation-notes.md`；确认 HEAD/worktree 状态与行号。
- OpenRazer commit `6820f9d`：Razer report 结构、CRC、鼠标/键盘 battery/charging、interface/wait、PID 定义。
- HaloBattery commit `2251f05`：Razer 90/PA/0528、Logitech 2.0/1.0、测试向量、设备验证表。
- FerrisPulse commit `4917a10`：Windows 90/91、同句柄、TID 0x1F/3F/FF、15ms。
- Solaar commit `e7304c4`：HID++ short/long、SWID、错误匹配、0x1000/1001/1004/1F20、1.0、接收器 PID/slots。
- HeadsetControl commit `8b4b711`：G533/G535/G633 固定命令、collection、电压 offset。
- Logitech cpg-docs + HID++2.0 draft：packet 结构、0x0005/0x1000。
- Linux kernel `d135ab4`：0x1000/1001/1004/1F20 的 status/curve、flags。
- OpenLogi 0x0005/1000/1001/1004/1F20 文档。
- Microsoft HidD_Set/GetFeature 文档。
- linux-hardware 1532:0555/0557。
- libratbag commit `8235b5b`：未发现电池读取实现（负证据）。[38]

### 未做/不能做

- 未连接任何真实 Razer/Logitech HID、接收器或蓝牙设备；未运行 AnyBattery 扫描；未做 Windows 运行时 A/B。
- 未验证 0x0556 的独立型号；未验证 0x00A4 是否能转发鼠标电量；未验证 0x08 TID。
- 未证明“同句柄”是硬性要求；只证明参考实现都这样做、API 文档没有此要求。
- 未解决 0x1004/0x1000 status=4 的“慢充 vs 错误”冲突。
- 未解决 0x1F20 的 3905mV→67%（Solaar/OpenLogi）vs 87%（Linux kernel）曲线冲突。
- 未验证 G533/G633 系列到底走固定 feature index 还是通用 0x1F20；公开实现不一致。

### 研究缺口

- 没有 Razer 官方 HID 协议文档；Razer 侧主要是 OpenRazer/HaloBattery/FerrisPulse 的逆向与实机记录。
- 没有 Logitech 0x1004/0x1001/0x1F20 的官方完整规范；OpenLogi 是高质量二次逆向，Linux 内核/Solaar 是一手实现但彼此有冲突。
- 没有足够数据建立“所有 Razer PID 的 firmware 变体/缩放”表；本文只列公开源码明确支持的 PID。
- 没有 Logitech 全型号 PID→电池 feature 表；Logitech 主要靠 feature discovery，不能从 receiver PID 推设备型号。

---

## 备注：许可证与实现边界

- OpenRazer：GPL-2.0-or-later；只提取协议常量/字段/状态，不复制驱动代码。
- HaloBattery：MIT；可参考，但仍建议按事实重写 parser，保留自己的错误处理。
- FerrisPulse：GPL-3.0；只读事实，不复制 C# 实现。
- Solaar：GPL-2.0-or-later；只读协议事实。
- HeadsetControl：GPL-3.0；只读协议事实。
- Logitech cpg-docs / HID++ draft：官方文档；按事实引用。
- 本文不包含任何厂商固件、密钥、写命令或 fuzz 过程。
