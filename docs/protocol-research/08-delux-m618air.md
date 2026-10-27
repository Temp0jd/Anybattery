# 08 · DELUX M618Air（1D57:FA60 / model 0x24）协议研究

> 状态：缩放审计完成；已按官方 `0x4290b0 → 0x421b90` 调用链修改 `extra.rs`，并完成一次 30 秒硬超时的只读 `.pi/diag` 扫描。未安装驱动、未发送配置/固件命令；另有用户提供的官方软件现场读数。
>
> 日期：2026-10-27
>
> 现场基线：`VID:PID=1D57:FA60`、`MI 02`、`usage 000A:0000`、`input_len=5`、被动报文 `03 24 40 01 06`。用户已确认鼠标型号为 **DELUX M618Air**；同一鼠标的官方 DELUX 驱动当场显示 **80%**。MouseBatteryTray 与 AnyBattery 的 6 是直接百分比假设，已确认错误。
>
> **2026-10-27 缩放审计更正**：上一版“`raw` 是直接 0–100 百分比、6 就是 6%”的结论过度。官方 M618Air handler `0x4290b0` 把 `byte[4]` 传给 UI helper `0x421b90`；后者按 `1..8` 档位映射为 `10/20/40/60/80/100`，`6` 对应 **80%**。state `02/03` 的充电分支位于另一个函数 `0x429460`，不属于 M618Air。本版 parser 已按 `1..8` 档位修正。用户现场官方 DELUX 驱动对同一帧 `03 24 40 01 06` 显示 **80%**，与 `0x421b90` 的 `6→80` 完全一致，缩放结论已获真机校准；其他档位仍需按同一方法抽查。

## 结论先行

| 问题 | 结论 | 证据强度 |
|---|---|---|
| `03 24 40 <state> <raw>` 的 `<raw>` 含义 | **M618Air 上是 1..8 档位，不是直接百分比**；`6` 是第 6 档，对应 **80%**，不是 6%。`0` 被 handler 拒绝，`9..100` 在 helper 中饱和为 100%。 | **强**：官方 `0x4290b0` → `0x421b90` 完整调用链 |
| `<state>` 含义 | `0x4290b0` 只读取 `byte[4]`，**不读取 state**。`0x01` 是当前唯一接受的 M618Air 档位帧；`0x02/0x03` 的充电分支在另一个函数 `0x429460`，不属于 M618Air，当前拒绝且不猜充电/百分比；`0x80` 同样拒绝，不套 Attack Shark。 | **强**（否定性证据） / 充电语义未确立 |
| 除 `device_id` 外的型号区分 | **当前 2.4G 路径没有可靠的公开区分方法**。接收器产品字符串是通用的 `2.4G Wireless Device`；report descriptor 相同（`000A:0000`、5 字节）；有线 PID 很可能是 `1D57:FA21`，但与 M618Mini/Mini DB 共用；`0x0B` 配置通道没有公开的型号查询。**不能全局硬编码 `0x24=DELUX`，也不能全局硬编码 `0x24=Attack Shark V6`** | **强**（否定性证据） |
| `input_len == 5` 是否改 `>=5`；`0x85` 缩放是否受影响 | **保持 `==5`**；M618Air 的事件 collection 就是 5 字节，parser 本身已接受 `>=5`。`0x85` 缩放**不受影响**，它是 Attack Shark X6 专用，M618Air 是 `0x24` | **强**（descriptor 证据） |

**对现场的直接判断**：`03 24 40 01 06` 按官方 M618Air 代码应显示 **80%**，而不是 6%；`0x421b90` 的档位映射是型号专属证据，优先于 MouseBatteryTray 的“直接 6”规则。当前 parser 已按 80% 实现；用户现场官方 DELUX 驱动读数已确认该帧为 80%，缩放结论不再依赖单纯静态推断。

---

## A. 最强证据：DELUX 官方 2025 Office Driver 静态分析

### A.1 安装包与可复现信息

- 官方下载页：<https://www.deluxworld.com/en-service-12.html>
  - 访问时（2026-10-27）页面第 452 行列出 `M618air / M618mini`，但该行的 `Device Driver` 链接为空（`href=""`）。
  - 同页 `M618XSC` 行链接到 `618XSD驱动文件.rar`；该安装包实际是 **DELUX Office Driver**，内部包含 M618Air 模型页。
- 下载 URL：<https://www.deluxworld.com/uploads/admin/file/20250530/618XSD%E9%A9%B1%E5%8A%A8%E6%96%87%E4%BB%B6.rar>
- 下载的 RAR SHA256：`8ca5af8d08c72e31d5e2e6eeced283ca03801d27ba3ed2d3555ac481a80e8709`
- 解包出的 Inno 安装器 `618XSD驱动文件.exe` SHA256：`708d83ec0c1fe4f0b7a5968b38fa7c39589c513d33a2b71c1bc11a172f5716a1`
- 安装器：Inno Setup 6.1.0，`DELUX Office Driver`
- 解包后关键文件：

| 文件 | SHA256 | 说明 |
|---|---|---|
| `app/Mouse.exe` | `8cee1fd9a9ed4dd0369cfdd2f7d0afbf7298e939806d09e148ef9aa4cb165404` | 主程序；型号分发与电量 UI |
| `app/res/lan.xml` | `baaba0da6182e0996f49f661b78fb794d52377bf6879f6a58d2be46af9d13798` | 5 个型号的显示名 |
| `app/hiddriver_ms_1.dll` | `f9efbf0630218b3e13e894b132d77ceb16113acef1bc841e25af8ce57eac39f4` | 与 `hiddriver_ms_4.dll` 完全相同 |
| `app/hiddriver_ms_4.dll` | `f9efbf0630218b3e13e894b132d77ceb16113acef1bc841e25af8ce57eac39f4` | 与 `hiddriver_ms_1.dll` 完全相同 |

> 说明：这是厂商专有软件，无开源许可证。本文只做静态解包、哈希和反汇编引用，不运行、不安装、不分发二进制。`hiddriver_ms_1.dll` 与 `hiddriver_ms_4.dll` 的哈希与 `gadgrandez/delux-m618xsd-macos` 记录的 `hiddriver_ms_4.dll` 一致，说明该安装包与公开 M618XSD 逆向资料同源。

### A.2 官方型号表：M618AIR = 内部 `ms_3`

`app/res/lan.xml` 第 9–13 行：

```xml
<Text id="device_1_text" value="M618Mini"/>
<Text id="device_2_text" value="M618Mini DB"/>
<Text id="device_3_text" value="M618AIR"/>
<Text id="device_4_text" value="M618XSD"/>
<Text id="device_5_text" value="M618XSC"/>
```

对应 5 个资源目录 `MS_1` … `MS_5`。其中 `MS_3` 就是 M618AIR。

### A.3 `Mouse.exe` 的型号分发：index 2 要求 `report[1] == 0x24`

`Mouse.exe` 是 32 位 PE，image base `0x400000`。报文处理函数在 `0x40c3f0`，它把 5 字节报文拆成：

```text
byte[0] = 0x03       report id
byte[1] = model id
byte[2] = event type（0x40 = 电量/充电）
byte[3] = status / 低字节
byte[4] = value / 高字节
```

随后按选择值 `[edi+index*4+0x20d898]`（0..4）分发；初始化时它与 5 个模型槽位一一对应。关键比较：

```asm
; index 0 -> M618Mini
40c44c: cmp bl,0x21

; index 1 -> M618Mini DB
40c538: cmp bl,0x22

; index 2 -> M618AIR
40c60c: cmp eax,0x2
40c615: cmp BYTE PTR [ecx],0x3
40c624: cmp bl,0x24
40c627: jne 0x40c6e2
...
40c6e2: push [ebp+0xc]
40c6ea: call 0x4290b0        ; M618AIR 的事件处理

; index 3 -> M618XSD
40c701: cmp BYTE PTR [ecx],al   ; al = 0x03
40c70f: cmp bl,0x28
...
40c7d5: call 0x432d10        ; M618XSD 的事件处理

; index 4 -> M618XSC
40c7fb: cmp bl,0x2a
```

按 `lan.xml` 的 `device_1..5_text` 顺序与 index 0..4 配对，得到：

| index | `lan.xml` 名称 | 期望 `report[1]` | 事件处理器 |
|---|---|---|---|
| 0 | M618Mini | `0x21` | — |
| 1 | M618Mini DB | `0x22` | — |
| 2 | **M618AIR** | **`0x24`** | `0x4290b0` |
| 3 | M618XSD | `0x28` | `0x432d10` |
| 4 | M618XSC | `0x2a` | — |

其中 `0x28 = M618XSD` 已被公开的 M618XSD 逆向文档独立确认（见 B.1），这反证了顺序配对的可靠性。`MS_3` 资源目录、`device_3_text=M618AIR` 与 index 2 的 `0x24` 三者一致。

另有一个辅助观察：`Mouse.exe` 在初始化槽位时出现资源字符串 `PID&0024`（`0x44e4c4`），同一初始化块设置 `VID 1D57 / wired PID FA21 / receiver PID FA60`。这说明 `0x24` 不仅用于报文，也用于该型号的内部标识。

### A.4 M618AIR 的缩放调用链：`0x4290b0` → `0x421b90`（审计更正）

上一版把 `byte[4]` 说成“直接 0–100 百分比”，漏看了实际 UI helper。完整调用链如下。

#### A.4.1 0x4290b0 确实是 M618Air 的 `03 24 40 01` 电量路径

- `Mouse.exe` 的型号分发在 `0x40c3f0`。选择值 2 对应 `report[1] == 0x24`，分支在 `0x40c6e2` 调用 `0x4290b0`（见 A.3）。
- `0x4290b0` 的 `event type = byte[2] << 8`；`byte[2] == 0x40` 时进入 `0x429273` 的 `0x4000` 分支。
- 该分支从 `[ebp+0xc]` 取低 16 位（`byte[4] << 8 | byte[3]`），只取高字节：

```asm
429273: cmp eax,0x4000
429278: jne 0x429368
4292a9: mov eax,[ebp+0xc]     ; byte[4]<<8 | byte[3]
4292b7: shr eax,0x8
4292ba: movzx ebx,al          ; ebx = byte[4]
...
429341: cmp ebx,0x1
429344: jb 0x429453           ; byte[4] == 0 被拒绝
42934a: cmp ebx,0x64
42934d: ja 0x429453           ; byte[4] > 100 被拒绝
429353: push ebx
429354: mov ecx,esi
429356: mov [esi+0x174],ebx
42935c: call 0x421b90          ; 电量 UI helper
```

结论：`byte[3]`（state）在这个分支里**没有被读取**；`byte[4]` 是唯一传给 `0x421b90` 的电量参数。它不是音量、DPI 或按键状态：调用点位于 `event type=0x40` 的电量分支，且 helper 写入电量进度条并格式化 `%d%%`。

#### A.4.2 0x421b90 的完整映射

`0x421b90` 的输入是上面传入的 `byte[4]`，映射如下：

```asm
421bc2: cmp edx,0x8
421bc5: jl 0x421bce
421bc7: mov esi,0x64          ; >= 8 -> 100
421bce: lea eax,[edx-0x6]
421bd1: cmp eax,0x1
421bd4: ja 0x421bdd
421bd6: mov esi,0x50          ; 6,7 -> 80
421bdd: lea eax,[edx-0x4]
421be0: cmp eax,0x1
421be3: ja 0x421bec
421be5: mov esi,0x3c          ; 4,5 -> 60
421bec: lea eax,[edx-0x3]
421bef: test eax,eax
421bf1: jne 0x421bf8
421bf3: lea esi,[eax+0x28]    ; 3 -> 40
421bf8: ...                   ; 2 -> 20, 0/1 -> 10
421c07: mov ecx,[edi+0x3bc]   ; 电量进度条
421c2d: push esi
421c3b: push 0x44bc88         ; "%d%%"
421c41: call 0x445334
```

映射表（实际接受域由调用方限制为 `1..=100`）：

| `byte[4]` | `0x421b90` 输出 |
|---|---|
| `0` | 调用方先拒绝（`jb 1`），不会进入 helper |
| `1` | `10` |
| `2` | `20` |
| `3` | `40` |
| `4`、`5` | `60` |
| `6`、`7` | `80` |
| `8`–`100` | `100`（helper 对所有 `>=8` 饱和） |

这不是“百分比经过 UI 四舍五入”：输入域明显是 `1..8` 档位；`6` 是第 6 档，显示 **80%**，不是 6%。`0x421b90` 输出的是 UI 百分比（写入进度条/`%d%%` 文本），不是音量或 DPI 枚举。

#### A.4.3 state 0x02/0x03 不属于这条 M618Air 证据链

上一版引用的 `429583` 充电分支位于**另一个函数** `0x429460`，它由 `0x42a363` 调用；`0x429460` 不是选择值 2 的 M618Air handler。M618Air 的 `0x4290b0` 只处理 `byte[4]`，不读取 `byte[3]`，也没有 `status==2/3` 的充电分支。

因此：

- **不能**用 `0x429460` 的 `status==2/3` 充电逻辑解释 M618Air。
- `state=0x02/0x03` 的充电/满电含义在 M618Air 上**未确立**；本版 parser 直接拒绝这些 state，不猜百分比，也不声称充电。
- `state=0x80` 同样不在这条已证实的分支语义内；继续拒绝，不套 Attack Shark 的 `0x80=充电`。

#### A.4.4 结论

**M618Air `03 24 40 01 <raw>` 的 `<raw>` 是 1..8 档位，不是直接百分比；现场 `raw=6` 按官方 helper 应显示 80%，不是 6%。** 该结论来自官方二进制的完整调用链，并已由用户现场官方 DELUX 驱动读数（80%）校准，强于 MouseBatteryTray 的“直接 6”规则。其他档位仍需抽查，见 F。

#### A.4.5 现场校准与 AnyBattery diag（2026-10-27）

- **用户官方软件读数**：同一 M618Air 在官方 DELUX 驱动下，被动帧 `03 24 40 01 06` 时显示 **80%**。
- **AnyBattery 只读 diag**：用当前 `extra.rs` 重建 `.pi/diag`，通过 `.pi/run-diag.ps1` 以 30000 ms 硬超时运行；扫描在超时前结束，输出：
  - `COUNTS devices=4 bluetooth=2 hid=9`
  - `1D57 2.4G 无线鼠标 | Usb24g | Available | Percent { level: 80, charging: Some(false) } | report 03 24 40 01 · raw 7 · 80%`
  - 本次 diag 捕获到 `raw=7`（helper 同样映射 80%）；用户官方读数是在 `raw=6` 时得到 80%。两者一致。
- 输出文件：`.pi/diag-1d57-80.txt`（两次运行结果一致；本次捕获 raw 7，映射 80%）。
- 结论：`raw=6/7 → 80%` 已同时获得官方软件与 AnyBattery 实机验证；扫描有界，未触发 30 秒硬超时。

### A.5 有线 PID（推断）

`Mouse.exe` 中 5 个模型槽位的初始化依次设置：

- 前 3 个槽位：`VID 0x1D57`，wired PID `0xFA21`，receiver PID `0xFA60`
- 第 4 个槽位：wired PID `0xFA28`（M618XSD）
- 第 5 个槽位：wired PID `0xFA2A`（M618XSC）

结合 `device_3_text=M618AIR`，**M618Air 的有线 PID 很可能是 `1D57:FA21`**，与 M618Mini / M618Mini DB 共用。该结论是静态推断，需真机枚举确认；即使确认，它也**不能**在 2.4G 模式下用于区分，因为接收器统一是 `1D57:FA60`。

---

## B. 社区/开源交叉证据

### B.1 `gadgrandez/delux-m618xsd-macos`（MIT）

- 仓库：<https://github.com/gadgrandez/delux-m618xsd-macos>
- commit：`3be3c392c4306a01929f8c74d1f4c84e1d62da93`（2026-10-15）
- 许可证：MIT
- 关键文件：
  - `docs/PROTOCOL.md`：<https://github.com/gadgrandez/delux-m618xsd-macos/blob/3be3c392c4306a01929f8c74d1f4c84e1d62da93/docs/PROTOCOL.md>
  - `DeluxDriver/Sources/DeluxKit/HIDMouse.swift`：<https://github.com/gadgrandez/delux-m618xsd-macos/blob/3be3c392c4306a01929f8c74d1f4c84e1d62da93/DeluxDriver/Sources/DeluxKit/HIDMouse.swift>
  - `DeluxDriver/Sources/selftest/main.swift`：<https://github.com/gadgrandez/delux-m618xsd-macos/blob/3be3c392c4306a01929f8c74d1f4c84e1d62da93/DeluxDriver/Sources/selftest/main.swift>

它记录的是 DELUX M618XSD（model byte `0x28`），不是 M618Air，但同一 DELUX / Xenta 接收器平台：

- `PROTOCOL.md` 第 13–16 行：有线 `1d57:fa28`、2.4G receiver `1d57:fa60`、蓝牙 `045e:0028`。
- 第 23–24 行：`MI_02` 的 `0x000B:0000` 配置 collection 和 `0x000A:0000` 事件 collection；事件是 5 字节 input report `0x03`。
- 第 135–142 行：`03 28 TT VV VV`，`TT=0x40` 为 battery/charging。
- `HIDMouse.swift` 第 15–26 行：`value = report[3] | report[4]<<8`；`value & 0xff == 2` 判充电；否则 `pct = value >> 8`，仅接受 `1...100`。
- `selftest/main.swift` 第 136–137 行：
  - `03 28 40 00 55` → 85%
  - `03 28 40 02 00` → charging

这条证据只证明 **M618XSD（0x28）** 的直接百分比与 `status=2` 充电语义；不能外推到 M618Air（0x24）。M618Air 的官方路径是 A.4 的 1..8 档位映射。

### B.2 `OpenMouse-Project/mouse-protocol`（AGPL-3.0-or-later）

- 仓库：<https://github.com/OpenMouse-Project/mouse-protocol>
- commit：`326f292bbe29ea73801cda1b2f0434f4fea62d18`（2026-10-26）
- 许可证：AGPL-3.0-or-later
- 关键文件：
  - `src/drivers/attackshark/hid.ts`：<https://github.com/OpenMouse-Project/mouse-protocol/blob/326f292bbe29ea73801cda1b2f0434f4fea62d18/src/drivers/attackshark/hid.ts>
  - `captures/delux-m600-pro/windows-webhid.txt`：<https://github.com/OpenMouse-Project/mouse-protocol/blob/326f292bbe29ea73801cda1b2f0434f4fea62d18/captures/delux-m600-pro/windows-webhid.txt>
  - `docs/delux-m600-pro-testing.md`：<https://github.com/OpenMouse-Project/mouse-protocol/blob/326f292bbe29ea73801cda1b2f0434f4fea62d18/docs/delux-m600-pro-testing.md>

它给出了两条重要事实：

1. **同一 `1D57:FA60` 接收器上的 DELUX M600 Pro**：`03 20 40 01 4d` = model `0x20`、77%；Windows `MI_02&COL03` 是 `UP=0x000A U=0x0000 In=5`，与现场完全同形。
2. **0x24 的现有映射是 Attack Shark V6**：`X11_RECEIVER_MODELS` 第 130 行 `[0x24, attackShark("V6")]`；`parseBatteryReport` 第 1064–1068 行对 `0x24` 按 percent 解析，且 `isBatteryReport` 只接受 `data[3] === 0x01`。这说明 OpenMouse 对 **Attack Shark V6 固件**按直接百分比解析；它的**品牌/型号名**只按 ID 表，无法覆盖 DELUX M618Air。DELUX 官方 M618Air 分支是 1..8 档位，二者不能混用。

> 重要冲突：OpenMouse 的 `0x24 = Attack Shark V6` 与 DELUX 官方 `0x24 = M618AIR` 并存。两者都不是“全局真相”；它们是不同厂商/固件上下文中的 ID 解释。详见 C.3。

### B.3 `incconutwo/mouse-battery-tray`（MIT）

- 仓库：<https://github.com/incconutwo/mouse-battery-tray>
- commit：`c1c6059472dab366c700a7f7516e0dbffde68fd7`（2026-07-25）
- 许可证：MIT
- 关键文件：
  - `battery_tray.pyw`：<https://github.com/incconutwo/mouse-battery-tray/blob/c1c6059472dab366c700a7f7516e0dbffde68fd7/battery_tray.pyw>
  - `devices.py`：<https://github.com/incconutwo/mouse-battery-tray/blob/c1c6059472dab366c700a7f7516e0dbffde68fd7/devices.py>
  - `CHANGELOG.md`：<https://github.com/incconutwo/mouse-battery-tray/blob/c1c6059472dab366c700a7f7516e0dbffde68fd7/CHANGELOG.md>

- `devices.py` 第 10–17 行只映射 `0x55/0x10/0x85/0x4d/0xbe/0x07`，**没有 `0x24`**；`1d57:fa60` 的默认名是 `"Attack Shark Mouse"`（第 21–24 行）。所以它显示 6 只是它按直接百分比解析；这不能作为 M618Air 正确缩放的证据，也不构成 `0x24=DELUX` 或 `0x24=V6` 的名称证据。
- `battery_tray.pyw` 第 428–437 行：`data[2] == 0x40` 时取 `data[4]`；只有 `device_id == 0x85 && 0 < raw <= 10` 才 `*10`，其他 ID 直接使用 raw。
- 第 440–451 行：`data[3]` 注释 `0x01=discharge, 0x02/0x03/0x80=dock charging`。这是 **Beken/Attack Shark 家族** 的经验规则，不是 DELUX M618Air 官方规则；对 M618Air，`0x80` 不应套用。
- `CHANGELOG.md` 第 21 行：明确把 10× 缩放限制为 X6（`0x85`），并说明“standard mice jumping to 100% when reaching 10% battery”。第 38–39 行同样是 X6/X11 的充电与缩放修复。

### B.4 其它已打开但只能作旁证的来源

- `noah-nuebling/mac-mouse-fix` issue #1738：<https://github.com/noah-nuebling/mac-mouse-fix/issues/1738>
  - 用户报告 `多彩 DELUX M618AIR`，蓝牙产品名为 `RivieraWaves SAS BT5.2 Mouse`，识别为 5 键但 4/5 键不能设置。
  - 证明型号存在、蓝牙产品字符串是通用的；与 2.4G 电量协议无关。
- `HarukaYamamoto0/attack-shark-x11-driver` issue #11：<https://github.com/HarukaYamamoto0/attack-shark-x11-driver/issues/11>
  - 讨论 Xenta 代工体系、同一基础协议、Delux 多个型号（M2/M3/M740/M800Mini/M900Mini/M900Pro 等）共用平台；未列出 M618Air，也未给出 `0x24` 的 DELUX 证据。
  - 该 issue 支持“同 VID/PID/协议基础不等于同品牌型号”的判断，但不能替代官方 M618Air 证据。
- `linux-usb.org/usb.ids`：<http://www.linux-usb.org/usb.ids>
  - 第 20937 行：`1d57  Xenta`；`FA60` 未在 usb.ids 中列出。说明 `1D57` 是 Xenta 体系，`FA60` 是通用接收器 PID，不是型号 ID。
- DELUX 官网 sitemap：<https://www.deluxworld.com/sitemap.xml>
  - 有 `m618xsd`、`M618MINI`、`M618DB`、`M618PLUSRGB`、`m618xsu`、`M618ZD` 等产品页，**没有 M618Air 独立产品页**。
  - 官方 service 页有 `M618air / M618mini` 行，但驱动链接为空；不能据此认为“官方没有驱动”，因为 Office Driver 包内部确实包含 M618AIR 模型页，但也不能据此认为该包一定已在 M618Air 真机验证。

---

## C. 四个问题的完整回答

### C.1 `raw` 是直接百分比还是档位/十倍？6 是否有独立证据？

**M618Air 上是 1..8 档位，不是直接百分比，也不是 ×10；现场 `raw=6` 对应 80%。**

完整证据链：

1. `Mouse.exe` 型号分发选择值 2 匹配 `report[1]==0x24`，调用 `0x4290b0`（A.3）。
2. `0x4290b0` 在 `event type=0x40` 分支只取 `byte[4]`，检查 `1..100`，然后 `call 0x421b90`（A.4.1）。
3. `0x421b90` 把 `1..8` 映射为 `10/20/40/60/80/100`；`6` → `80`；`>=8` → `100`（A.4.2）。
4. 用户现场官方 DELUX 驱动对同一鼠标/同一帧 `03 24 40 01 06` 显示 **80%**，与 `0x421b90` 的 `6→80` 完全一致。
5. 因此现场 `03 24 40 01 06` 应为 **80%**，不是 6%。

这与 MouseBatteryTray 的直接 6 规则冲突。MouseBatteryTray 的规则来自 Beken/Attack Shark 通用经验，并未针对 M618Air 的 `0x421b90` 调用链；按“型号专属官方证据优先”，应采用 80%。

**校准状态**：`raw=6 → 80%` 已由用户现场官方 DELUX 驱动读数确认；仍需抽查 `raw=1..8` 的其他档位（以及是否有 `9..100` 的大值）以确认固件输入域。若后续出现与 helper 不符的档位，再单独评估；当前证据不支持直接 6。

### C.2 `state` 的充电/放电/满电含义

对 **DELUX M618Air**（不是 Attack Shark V6）：

| `state` | 官方 M618Air `0x4290b0` 行为 | 本版处理 | 证据强度 |
|---|---|---|---|
| `0x01` | 进入 `0x4000` 分支，只读 `byte[4]`，不读 state；`1..8` 档位 → 10/20/40/60/80/100 | 接受为档位帧；`charging = Some(false)` 沿用前版契约，但官方 handler 本身不解码充电位 | 强（缩放）/ 中（charging） |
| `0x02` | `0x4290b0` 同样不读 state；上一版引用的充电分支在 `0x429460`，不属于 M618Air | **拒绝**，不猜百分比，也不声称充电 | 强（否定性证据） |
| `0x03` | 同上；不能从 `0x429460` 的 `charging_text` 推到 M618Air | **拒绝**，不猜百分比/满电 | 强（否定性证据） |
| `0x00` | `0x4290b0` 不读 state，但无实机样本 | **拒绝** | 弱/未观察 |
| `0x80` | 不在这条已证实的分支语义内；不得套 Attack Shark 的 `0x80=充电` | **拒绝** | 强（否定性证据） |
| 其它 | 无证据 | **拒绝** | — |

**不要做的事**：

- 不要把 `0x02/0x03` 写成 M618Air 的充电/满电；那条分支属于另一个 handler。
- 不要把 `0x80` 标成充电。
- 不要在 `0x02/0x03` 上把 `byte[4]` 当百分比；本版直接拒绝。
- 不要因为 M618XSD（0x28）的 `status=2` 充电语义，就把它套到 M618Air（0x24）。

### C.3 除 `device_id` 外的型号区分方法；0x24 多品牌时如何命名

**结论：当前 2.4G 路径没有可靠的自动型号区分方法，名称必须保持通用，不能全局硬编码品牌。**

逐项核查：

| 方法 | 是否可用 | 证据/原因 |
|---|---|---|
| 产品字符串 | **否** | DELUX M618XSD / M600 Pro 的接收器产品名是 `2.4G Wireless Device`；M618Air 的蓝牙名是 `RivieraWaves SAS BT5.2 Mouse`。都太通用，无法区分品牌/型号。 |
| Report descriptor | **否** | `MI_02` 的 `0x000A:0x0000` 是 5 字节 input；`0x000B:0x0000` 是配置 feature。M618XSD、M600 Pro、现场 M618Air 同形。 |
| 有线 PID | **不能用于 2.4G** | M618Air 很可能是 `1D57:FA21`，但与 M618Mini / M618Mini DB 共用；M618XSD 是 `FA28`，M618XSC 是 `FA2A`。2.4G 接收器统一 `FA60`，没有型号信息。 |
| 固件查询 | **没有公开证据** | `0x0B` 配置通道存在，但 M618XSD 逆向资料明确说官方驱动“从不回读配置”；没有 M618Air 的型号/固件查询命令。主动查询属于新协议，不能在没有证据时实现。 |
| USB serial | **不可靠** | M600 Pro 的抓包明确说有线/接收器都不暴露 serial；M618Air 未见 serial 证据。 |
| `device_id` | **只能给“模型 ID”，不能给品牌** | `0x24` 在 DELUX 官方上下文是 M618AIR，在 OpenMouse/Attack Shark 上下文是 V6。两者都成立，说明 ID 是厂商/固件局部命名，不是全局唯一。 |

**命名建议（最小、准确）**：

1. **保留现名** `"1D57 2.4G 无线鼠标"`。它不声称品牌，不会误导。
2. `detail` 继续显示 `"report 03 24 40 · raw 6"`；`device_id` 已经在 detail 中，足够诊断。
3. 如果产品 UI 需要更具体，最多写成 `"1D57 2.4G 无线鼠标 (model 0x24)"`，**不要**写 `"DELUX M618Air"` 或 `"Attack Shark V6"`。
4. 如果用户明确确认型号，可以未来做“用户别名/覆盖”功能；不要用 `0x24` 自动推断。当前代码没有可靠的 per-device key（无 serial，path 会变），所以不建议在本阶段做。
5. 如果将来拿到 **M618Air 专属** 的 wired PID + product string 组合，或固件返回型号字符串，再把名称细化；届时也要把 `0x24` 的 DELUX 规则限定在“DELUX 品牌/专属 PID”上下文，不能全局覆盖。

### C.4 `input_len == 5` 是否应改 `>=5`；`0x85` 缩放是否受影响

**`input_len == 5` 保持。**

- `is_1d57_status_interface` 目前要求 `MI 02`、`000A:0000`、`input_len == 5`，这正是现场和 M618XSD/M600 Pro 的 collection 形状。
- `parse_1d57_battery` 本身已经用 `report.len() < 5` 拒绝短包，即 parser 接受 `>=5`；接口 gate 用 `==5` 是为了只匹配这个 5 字节事件 collection，避免把同一接收器的其它更长 report 当成电量帧。
- 没有证据表明 M618Air 的该 collection 会声明 6+ 字节。把 gate 改成 `>=5` 会扩大匹配面，可能让其它 `03 xx 40 ...` 的长 report 进入 parser；收益不明确，风险更高。
- 如果未来真机抓到“同一个 `000A:0000` collection 声明长度 >5”的证据，再改成 `>=5` 并保留 parser 的严格头部/长度检查；不要现在预防性放宽。

**`0x85` 缩放不受影响。**

- `0x85` 是 Attack Shark X6 的模型 ID；DELUX 官方 5 型号表（`0x21/0x22/0x24/0x28/0x2a`）没有 `0x85`。
- M618Air 是 `0x24`，不会进入 `0x85` 分支；`0x85` 的 `1..10 -> ×10` 规则保持原样。
- 反过来，也不要把 `0x24` 加进 `0x85` 的缩放分支。现场 `raw=6` 按 M618Air 官方映射是 80%，不是 60%。

---

## D. 已实施的最小修改（`extra.rs`）

> 本节记录本次实际落地的 parser 与测试；`Confidence` 保持 `Community`。

### D.1 Parser

已按官方 `0x421b90` 映射实现：

```rust
fn parse_m618air_level(raw: u8) -> Option<u8> {
    if raw == 0 || raw > 100 {
        return None;
    }
    Some(match raw {
        1 => 10,
        2 => 20,
        3 => 40,
        4 | 5 => 60,
        6 | 7 => 80,
        _ => 100,
    })
}

0x24 => match state {
    0x01 => parse_m618air_level(raw).map(|level| MouseBatteryReport::Direct {
        device_id,
        state,
        raw,
        level,
    }),
    // 0x4290b0 不读 state；02/03 的充电分支在 0x429460，不属于 M618Air。
    _ => None,
},
```

要点：

- `raw=0` 被官方 handler 的 `cmp ebx,1; jb` 拒绝，parser 同样拒绝。
- `raw=1..8` 映射为 `10/20/40/60/80/100`；`raw=6` → `80`。
- `raw=9..100` 在 helper 中同样饱和为 `100`；parser 保留 raw 以便诊断。
- `state=0x02/0x03/0x80` 及其他 state 一律拒绝，不猜百分比、不声称充电。
- `detail` 保留 raw 并显示映射后的 level，例如 `report 03 24 40 01 · raw 6 · 80%`。
- `Confidence` 仍为 `Community`。

### D.2 名称

保持：

```rust
name: Some("1D57 2.4G 无线鼠标".to_owned()),
```

不要改成 `"DELUX M618Air"` 或 `"Attack Shark V6"`。`detail` 已经包含 `device_id` 与 `raw`，足以诊断。

如果主代理坚持要给用户一个型号提示，建议用中性的：

```rust
name: Some("1D57 2.4G 无线鼠标 (model 0x24)".to_owned()),
```

但这仍然不是品牌名；不要把它写成 DELUX。

### D.3 接口 gate

保持：

```rust
info.input_len == 5
```

不要改成 `>=5`。parser 的 `report.len() < 5` 已经处理短包，接口 gate 只负责 collection 形状。

### D.4 `0x85`

保持现有缩放分支，不加 `0x24`。不要重构 `0x10/0x85` 的语义；那是另一个证据域。

### D.5 测试

- `m618air_state_01_maps_level_six_to_eighty_percent`：现场 `03 24 40 01 06` → level 80，`charging = Some(false)`，detail 保留 raw 6。
- `m618air_state_01_level_table_matches_official_helper`：1→10、2→20、3→40、4/5→60、6/7→80、8..100→100。
- `m618air_state_01_rejects_zero_and_above_one_hundred`：raw 0、raw 101 → `None`。
- `m618air_state_02_03_are_not_percent_readings`：state 02/03 → `None`，不猜充电/百分比。
- `m618air_unknown_states_are_rejected`：0x80/0x00/0x04 → `None`。
- `x85_heartbeat_scales_only_one_to_ten`、`x11_status_reports_percentage`、`x6_heartbeat_scales_raw_by_ten`、`other_beken_device_uses_direct_value` 保持回归。
- 短包、错误 report ID、错误 `byte[2]`、`raw>100` 保持拒绝。

---

## E. 测试向量

### E.1 现场与 M618Air

| 输入（含 report ID） | 期望 |
|---|---|
| `03 24 40 01 06` | `Direct { device_id: 0x24, state: 0x01, raw: 6, level: 80 }`；`charging = Some(false)`；detail `report 03 24 40 01 · raw 6 · 80%` |
| `03 24 40 01 01` | level 10 |
| `03 24 40 01 02` | level 20 |
| `03 24 40 01 03` | level 40 |
| `03 24 40 01 04` / `05` | level 60 |
| `03 24 40 01 06` / `07` | level 80 |
| `03 24 40 01 08` / `09` / `64` | level 100（helper 对 `>=8` 饱和） |
| `03 24 40 01 00` | `None`（官方 handler 先拒绝 0） |
| `03 24 40 01 65` | `None`（>100） |
| `03 24 40 02 2a` | `None`（state 02 不属 M618Air 证据链，不猜充电/百分比） |
| `03 24 40 03 2a` | `None`（同上） |
| `03 24 40 80 06` | `None`（不得判为充电） |
| `03 24 40 00 06` | `None`（未观察） |
| `03 24 50 00 06` | `None`（`byte[2] != 0x40`，是 ACK 不是电量） |
| `03 24 40 01` | `None`（短包） |
| `02 24 40 01 06` | `None`（report ID 错） |
| `03 24 41 01 06` | `None`（event 字节错） |

### E.2 同平台回归

| 输入 | 期望 |
|---|---|
| `03 28 40 00 55` | M618XSD 85%（开源 selftest 向量） |
| `03 28 40 02 00` | M618XSD charging（开源 selftest 向量） |
| `03 20 40 01 4c` | DELUX M600 Pro 76%（OpenMouse Windows 抓包） |
| `03 55 40 01 49` | Attack Shark X11 73% |
| `03 10 40 01 07` | Attack Shark R1 70%（tenths） |
| `03 85 40 01 07` | Attack Shark X6 70%（tenths；保持） |
| `03 85 40 01 0b` | 当前实现 `None`（>10）；不要因为 M618Air 改成直接 11% |
| `03 24 40 01 06` 且 `input_len=6` | 接口 gate 不匹配（当前 `==5`）；这是预期行为，除非有真实 6 字节 descriptor 证据 |

### E.3 命名测试

```rust
let reading = parse/read 0x24 frame;
assert_eq!(reading.name, Some("1D57 2.4G 无线鼠标".to_owned()));
assert!(reading.detail.contains("24"));
assert!(reading.detail.contains("raw 6"));
assert!(reading.detail.contains("80%"));
// 不得出现 "DELUX" / "Attack Shark" / "V6" / "M618"
```

---

## F. 真机验证计划（只读、无写入）

1. **确认 collection 形状**
   - Windows 上枚举 `1D57:FA60` 的 `MI_02`：
     - `000A:0000` input 长度 5；
     - `000B:0000` feature collection 存在；
     - 产品字符串记录为 `2.4G Wireless Device`。
   - 与现场 `input_len=5` 对齐；确认 parser 读到的是 5 字节 `03 ...`。

2. **官方软件读数校准（raw6=80 已完成）**
   - 用户已在同一 M618Air 上完成官方 DELUX Office Driver 读数：被动帧 `03 24 40 01 06`（raw=6）时官方 UI 显示 **80%**。
   - 后续抽查时同时记录：
     - 官方 UI 的百分比；
     - 原始被动帧 `03 24 40 01 <raw>`。
   - raw6=80 已确认；继续抽查 raw1..8 的官方 UI 是否分别为 10/20/40/60/80/100。
   - 如果其他档位也符合 helper，则 1..8 映射完整确认；如果出现不符，记录该档位并单独评估，不得回退到 raw6=6。

3. **放电档位序列**
   - 鼠标充满后拔线，保持 2.4G，持续记录 `03 24 40 01 <raw>`。
   - 观察 raw 是否从 `8` 往下走，以及 raw `8,7,6,5,4,3,2,1` 是否对应官方 UI 的 `100,80,80,80,60,60,40,20,10`。
   - 若 raw 出现 `9..100` 的大值，按官方 helper 仍应显示 100%；记录该现象以确认固件是否真的会送百分比。
   - 若 raw 直接按 0–100 连续下降且官方 UI 显示相同数值，则当前 1..8 结论需要修正；在拿到这种证据前，以官方调用链为准。

4. **充电/未知 state**
   - 插线/放底座，记录是否出现 `state=0x02`、`0x03`、`0x80` 或其他值。
   - 不把 `state=02/03/80` 当百分比，也不声称充电；只记录原始帧和官方 UI 行为，供后续单独研究。
   - 当前 parser 对 02/03/80 一律拒绝；若真机出现，需要新的证据再定义。

5. **命名验证**
   - 记录接收器 product string、manufacturer、serial（若有）、以及有线模式 PID（若可切换）。
   - 确认这些字段是否真的能区分 M618Air 与 M618Mini/Mini DB；如果仍然通用，保持通用名称。

6. **回归**
   - 同一接收器上若有其它 1D57 设备，确认 `0x24` 分支不会误伤 `0x55/0x10/0x85/0x28`。
   - 确认 `input_len==5` gate 不会漏掉 M618Air，也不会把其它长 report 放进 parser。

**验收标准**：

- 官方软件与 parser 对同一 `raw=6` 帧均显示 **80%**；
- parser 不把 `raw=6` 显示成 6%，也不显示成 60%；
- `state=02/03/80` 不产生百分比或充电猜测；
- 现有 X11/X6/R1/M600 Pro 测试不回归。

## G. 检索范围与未决项

### G.1 实际检索范围

- DELUX 官网：sitemap、英文 service 页、M618XSD/M618MINI 产品页、官方驱动下载包（2022 M618XSD.zip、2025 618XSD驱动文件.rar）。
- GitHub API 仓库/issue 搜索：`M618Air`、`M618 Air`、`M618`、`M618mini`、`M618XSD`、`DELUX M618`、`1D57 FA60`、`03 24 40 01 06`。
- GitHub 源码：`gadgrandez/delux-m618xsd-macos`、`OpenMouse-Project/mouse-protocol`、`incconutwo/mouse-battery-tray`、`HarukaYamamoto0/attack-shark-x11-driver`、`buildcores/buildcores-open-db`、`FMakareev/Delux-m618`、`AiNyooB/DELUX_M618XSD`。
- Sourcegraph 公共代码搜索：`M618air`、`M618XSD`、`0x24 FA60` 等；`M618air` 无代码结果。
- USB ID 数据库 `usb.ids`。
- 通用搜索引擎（DuckDuckGo/Bing/Google/Brave/Mojeek）在本次环境被反爬或返回无关结果，未作为证据；没有把搜索摘要当作协议证据。
- 已用 `.pi/diag` 做一次只读扫描（30 秒硬超时）；未安装/运行官方驱动；未发送任何配置/固件命令。

### G.2 未决项

1. `state=0x02` 与 `0x03` 在 M618Air 上的含义：当前证据显示 `0x4290b0` 不读 state，上一版的充电分支属于 `0x429460`，不属于 M618Air；需要新的型号专属证据。
2. `state=0x00`、`0x80` 是否会出现；目前均拒绝，不赋予含义。
3. raw6=80 已由用户官方软件读数确认；其他档位（1..8、以及是否出现 9..100）仍需抽查。
4. M618Air 有线 PID 是否为 `1D57:FA21`；目前是从官方驱动初始化块推断，未真机枚举。
5. 官方 2025 Office Driver 是否真的在 M618Air 真机上工作；包内有模型页，但官网 M618air 行的驱动链接为空。
6. `0x24` 是否还被除 Attack Shark V6 之外的其它品牌/型号使用；目前只确认了 DELUX 与 Attack Shark 两个上下文。
7. M618Air 的蓝牙 GATT 电量路径未研究；本次只针对 2.4G 被动帧。
8. 是否存在可读的固件/型号字符串（`0x0B` 配置通道）尚未验证；在拿到证据前不应实现主动查询。

---

## H. 对现有 `04-oem-mouse.md` 的修正提示

`docs/protocol-research/04-oem-mouse.md` 的 A.1 把现场 `0x24` 直接判为 **Attack Shark V6**，并称“终极真相”。对本次用户确认的 DELUX M618Air，这个品牌结论**不成立**：

- `0x24` 在 DELUX 官方 Office Driver 中是 **M618AIR**；
- `0x24` 在 OpenMouse/Attack Shark 表中是 **V6**；
- 两者可以同时存在，因为 `device_id` 是厂商/固件局部编号，不是全局唯一型号 ID。

M618Air 的官方 `0x4290b0 → 0x421b90` 调用链表明 `raw=6` 是第 6 档，应显示 **80%**，不是 6%，也不是 ×10 或 60%；因此 04 中“现场 6%=直接百分比”的结论同样不能套到 M618Air。“现场设备是 Attack Shark V6”“`0x80` 是充电”等结论也不能套用。建议主代理在整合时修正 04 的措辞，避免把 Attack Shark 证据冒充 DELUX 证据。

---

## I. 参考链接汇总

| 来源 | 永久链接/版本 | 许可证 |
|---|---|---|
| DELUX 官方 service 页 | <https://www.deluxworld.com/en-service-12.html> | 厂商页面 |
| DELUX 官方 Office Driver 2025 | <https://www.deluxworld.com/uploads/admin/file/20250530/618XSD%E9%A9%B1%E5%8A%A8%E6%96%87%E4%BB%B6.rar>；RAR SHA256 `8ca5af8d…`；安装器 EXE `708d83ec…` | 专有，仅静态分析 |
| M618XSD macOS 逆向 | <https://github.com/gadgrandez/delux-m618xsd-macos/tree/3be3c392c4306a01929f8c74d1f4c84e1d62da93> | MIT |
| OpenMouse mouse-protocol | <https://github.com/OpenMouse-Project/mouse-protocol/tree/326f292bbe29ea73801cda1b2f0434f4fea62d18> | AGPL-3.0-or-later |
| MouseBatteryTray | <https://github.com/incconutwo/mouse-battery-tray/tree/c1c6059472dab366c700a7f7516e0dbffde68fd7> | MIT |
| mac-mouse-fix #1738 | <https://github.com/noah-nuebling/mac-mouse-fix/issues/1738> | 仓库许可证 |
| Attack Shark X11 driver #11 | <https://github.com/HarukaYamamoto0/attack-shark-x11-driver/issues/11> | 仓库许可证 |
| USB ID 数据库 | <http://www.linux-usb.org/usb.ids> | 公共数据 |
