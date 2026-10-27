研究完成（只读，未修改 AnyBattery 代码/docs/README/release）。

**工具与边界说明**

- `fabric_exec` 仍不可用；本次改用可用的原生 `bash` 完成仓库读取、`git`、`curl` 与源码核对，未编造任何报文。
- 读取的是工作树（HEAD `70906c7`，含未提交 UI/协议修改），未连接真实设备、未运行扫描/探针、未装驱动、未 fuzz。
- 新增 clone 仅放在 `/tmp/anybattery-research-other-controllers`。
- 本报告建议保存为 `docs/protocol-research/03-other-vendors-controllers.md`；按任务边界我没有直接写入该目录。

---

> 状态：子代理研究输入。优先复核已有解析错误；新型号与协议须通过来源和 parser 验收。

# A. 新发现

## A1. SteelSeries 当前 B0 解析存在“低电量被放大”和 Nova 5 分支反向

当前实现：`crates/anybattery-windows/src/extra.rs:531-586`。

1. **Nova 7 Gen2 的 1–4% 会被放大到 25–100%**  
   当前逻辑对 `level_byte <= 4` 一律按离散档位 `×25`：
   ```rust
   let level = if level_byte <= 4 { level_byte * 25 } else { level_byte.min(100) };
   ```
   HeadsetControl 明确按 PID 区分：
   - 离散固件 PID：`0x2202, 0x2206, 0x220A, 0x223A, 0x227A, 0x22A4`，`data[2]` 为 0–4，需 `map(0..4 → 0..100)`；
   - Gen2/新固件 PID：`0x22A1, 0x227E, 0x2258, 0x229E, 0x22AD, 0x22A9, 0x22A5, 0x22A7`，`data[2]` 已是 0–100。  
   因此 Gen2 报 3% 时，当前会显示 75%。[S3 Nova7 L130-L167][S4 L7-L15]

2. **Nova 5 的离线条件被当成在线**  
   HeadsetControl Nova 5：`data[1] == 0x02` 表示离线；`data[3]` 是电量，`data[4] == 0x01` 表示充电。[S3 Nova5 L183-L196]  
   当前代码却在 `link == 0x02` 时读取 `reply[3]` 并返回电量，等于把“离线”当“在线”；在线 Nova 5 又会落入 Nova 7 分支，按 `reply[2]` 解析，结果通常是 0% 或错误值。  
   这不是“型号未覆盖”，而是**已有分支解析错误**。

3. **缺少 GameBuds、Nova Pro、Nova Elite、Aerox 鼠标**  
   当前只发 `00 B0` 和 `00 AA 01`：
   - GameBuds 需要 `B0 … <bud status> <level>`，取外出耳塞的较低电量；
   - Nova Pro `1038:12E0/12E5` 需要报告 ID `06` 的 `06 B0`，电量是 byte 6 的 0–8 九档码，状态在 byte 15；
   - Nova Elite `1038:2244` 需要 `01 B0` 或 `07 B7`；
   - Aerox 3/5/9 需要 `00 D2`，不是 `00 AA 01`。  
   [S3 Nova7/Nova5][S4 L109-L131, L225-L235]

4. **Rival 3 的两种回复形状只支持了其中一种**  
   当前要求 `reply[0] == 0xAA` 且 `level = reply[1]`。公开资料对回复形状本身有分歧：
   - steel-mouse 同时接受 `AA 01 <level> <charging>`（level 在 byte 2）和 `AA <level> <charging>`；
   - rivalcfg 则接受无 `AA` 回显的 3 字节形状（level byte 0，charging byte 2）。  
   当前两种都没完整覆盖：若设备回 `AA 01 55 00`，会显示 1%；若回无回显形状，会直接拒绝。应在拿到具体 PID 的真机抓包后二选一或做严格双形状。[S5 L165-L171][S6 L337-L367][S4 L231-L235]

## A2. Corsair 当前 Bragi 解析偏移错误，Virtuoso 请求帧也不匹配

当前实现：`extra.rs:622-657`。

- **Dark Core RGB Pro SE (`1B1C:1B7F`)**：已测回复形如 `01 02 00 26 02 …`，其中 `26 02` = 550 = 55%。正确解析是 `reply[3] | reply[4] << 8`，且必须先校验 `reply[0] == 0x01`、`reply[1] == 0x02`、`reply[2] == 0x00`。  
  当前却用：
  ```rust
  if reply.len() < 6 || reply[1] != 0x02 || reply[2] != 0x00 { return None; }
  let raw = reply[4] as u16 | ((reply[5] as u16) << 8);
  ```
  即从 byte 4/5 取 16 位，**晚了一个字节**；若 Windows 读回带 report ID `00` 的 65 字节，则 `reply[1]` 变成 `0x01`，直接失败。  
  [S4 L147-L151][S4 providers/corsair.py L24-L60]

- **Virtuoso SE/XT (`1B1C:0A40/0A3E/0A64`)**：HeadsetControl 使用 report ID `0x02` 的 64 字节帧 `02 09 02 0F`；HaloBattery 在 `0A40` 上实测 **Windows 拒绝所有带 report-id 前缀的写**，只接受裸 64 字节 `02 09 02 0F`，回复为 `01 01 02 00 <value LE>`，充电属性是 `0x10`。  
  当前发送的是 65 字节 `00 02 09 02 0F`，且仍按 Dark Core 的 `reply[1] == 0x02` 解析；对 `0A40` 大概率完全无回复，对其它 Virtuoso 也会错位。  
  [S3 Virtuoso L104-L135, L200-L230][S4 L43-L50]

- 当前还漏掉：
  - Void v2 家族 `1B1C:2A08 / 2A02 / 0A97`：65 字节 `00 02 09 02 0F`，需要固件/接收器/耳机三次心跳，回复 `reply[4] | reply[5] << 8` 为 0–1000，除以 10；无充电位。
  - Virtuoso 有线 `0A3D/0A62`，以及 `0A40` 的充电属性 `0x10`。
  - Dark Core 的原始值 0 应拒绝，不能显示 0%；当前只拒绝 `>1000`。

## A3. VAXEE 当前少乘 5，且没有充电查询

当前实现：`vendor.rs:456-472`。

- 请求：feature report `0x0E`，`0E A5 0B 01 01 …`。
- 公开社区实现一致认为电量字节 `reply[5]` 是 **0–20 的 5% 档位**，应 `raw * 5`；当前直接把 `reply[5]` 当百分比，且只拒绝 `>100`。  
  所以 raw 20（100%）当前显示 20%，raw 16（80%）显示 16%。
- 充电页是 `0x10`：`0E A5 10 01 01 …`，`reply[5] == 1` 表示充电；当前没有。
- `feature_roundtrip` 在 SetFeature 后关闭句柄，再重新打开 GetFeature（`vendor.rs:448-454`）；VAXEE/Endgame 这类设备通常要求同一句柄完成 set/get，当前实现可能根本读不到。
- 证据强度：只有 FerrisPulse 等社区实现，且其测试是模拟报文，不是本机硬件；应标“社区/待真机”，不能按 README 的 ✅ 宣称。[S14 L525-L590]

## A4. Endgame Gear / Nordic nRF 当前证据不足，且实现缺少必要的回包校验

当前实现：`vendor.rs:474-514`。

- Endgame：当前 `A1 B4` 后直接取 `reply[16]`、电压 `[17..18]`，没有校验 `reply[1] == 0x01`，也没有“第一次读是唤醒、第二次才是数据”的重试。公开资料还存在**另一套 Beken 16 字节协议**（`0x08/0x04`，电量在 byte 5、充电 byte 6、mV byte 7–8）用于 OP1we/XM2we；两套协议到底对应哪些型号没有统一结论。[S14 L452-L525]
- Nordic：当前 feature `0x05 15 00 00 01`、32 字节、`reply[9]`。我找到的公开一手证据不足；仅有本地未授权小工具使用该帧。另有一套 `1915:0723`、usage `0xFF0A` 的被动 64 字节协议（`0x4A/0x41`，电量 byte 6），说明“Nordic nRF”不是一个可 VID 泛化的协议。
- 因此这两个适配器目前只能算“实验路径”，不应在 README 中列为 ✅。

## A5. Steam Controller 2025 的监听窗口会漏掉 0x43

当前实现：`extra.rs:92-136` + `hid.rs:808-830`。

- 当前 `listen_idle(path, 64, 3000, 450)`：总窗口 3 s，**450 ms 没有新报告就提前返回**。
- 实测/公开资料：Puck 在控制器唤醒时约每 **2.5 s** 发一次 `0x43`（约 0.4 Hz）；HaloBattery 明确记录 0.4 s 窗口会漏，最终使用 **5 s** 窗口。[S4 L346-L350][S4 providers/steam.py L80-L113]
- 因此当前实现不是“偶尔慢”，而是**在安静 slot 上很可能永远读不到**。充电位映射 `1=放电, 2/4=充电` 本身正确，0x43 的 byte 1/2 偏移也正确。[S1 steam_triton L351-L372][S1 controller_structs L678-L697]

## A6. Steam Controller 2015 无线接收器有可用的 Linux 被动电量路径

- Linux `hid-steam` 对 **2015 无线接收器 `28DE:1142`** 的 64 字节报文：
  - `data[0] == 0x01`, `data[1] == 0x00`, `data[2] == 0x04`（battery status）；
  - 电压 `data[12..13]` LE（mV）；
  - 电量 `data[14]`（百分比）。
- 该路径只在 `STEAM_QUIRK_WIRELESS` 下读取；2015 有线 `0x1102` 不走电池读取。[S2 hid-steam L2297-L2322, L2622-L2640, L2682-L2692, L2736-L2770]
- SDL 也列出 `0x1102/1105/1106/1142`，并且 `0x1201/0x1202` 是另一类 `SteamControllerV2`；这些不要在没有报文证据时一起加入。[S1 controller_list L661-L673]
- 当前 AnyBattery 完全不支持 2015；这是“可新增、只读、低风险”的候选，但需要 Windows 真机 fixture 才能标 Verified。

## A7. DualSense / DualShock 4 的偏移基本正确，但状态语义有错

当前实现：`extra.rs:138-190`。

- 偏移正确：
  - DualSense USB `0x01` → `report[53]`；BT `0x31` → `report[54]`；
  - DS4 USB `0x01` → `report[30]`；BT `0x11` → `report[32]`。  
  这与 SDL/Linux 一致。[S1 ps5 L129, L1429-L1453; ps4 L95, L1149-L1169][S2 hid-playstation L245-L267, L1610-L1644]

- 错的是状态：
  - DualSense 高半字节 `2` 表示 **充满**，SDL/Linux 直接给 100%；当前却按低半字节 `×10` 并标“充电”。
  - DualSense 低半字节 `0x0C` 表示“不报告电量”；SDL 专门排除，当前 `.min(10)` 会显示 100%。
  - DS4 有线时 `11` 是满电，`14/15` 是电压/温度/充电错误；当前 `.min(10)` 把它们都变成 100% 且充电。
  - 这些会造成“满电/错误状态被显示成任意百分比”的假读数。[S1 ps5 L1481-L1508; ps4 L1149-L1169][S2 hid-playstation L1610-L1644, L2548-L2589]

## A8. Switch 输入格式正确，但缺少“非 full mode”下的只读子命令

- 当前 `report[2]`、bit 7–5 电量、bit 4 充电，符合 SDL/Linux。[S1 switch L2688-L2714][S2 hid-nintendo L554-L568, L1647-L1680]
- 当前只监听 `0x21/0x30/0x31`，且要求蓝牙路径；如果控制器没有被 Steam/游戏切到 full report，就没有 `0x30`，当前不会主动发只读子命令 `0x02`（request device info），因此可能一直无读数。HaloBattery 明确做了这一步。[S4 L328-L332]
- 当前把电平 `0..8` 映射为 `level*100/8`，与 SDL 的 round 有 1% 级别差异，不影响可用性。

## A9. 8BitDo 格式正确，但型号表不完整且“Verified”标记过头

- 当前 `report[14] & 0x7F`、bit 7 充电、BT `0x01` / USB `0x04`、Pro 3 `0x6009` 固定 `0x04`，与 SDL 一致。[S1 8bitdo L648-L674]
- 缺少 `0x6012`（Ultimate 2 Wireless）；SDL 把它列为 8BitDo 并在同一驱动中支持 powerstate，可作为候选。[S1 controller_list L657]
- 当前对所有型号都写 `Confidence::Verified`，但公开资料只有 Pro 3 蓝牙有真机确认；其他型号应保持 Community，避免过度承诺。[S4 L316-L320]

## A10. Keychron / Lofree 的小问题

- **Keychron**：`3434:D028/D048` 是 **Keychron M5 鼠标 / Ultra-Link 8K 接收器**，当前 `DeviceKind::Keyboard` 是明确误标（`extra.rs:415`）。协议本身 `B3 06` → `B4 06` byte 20 正确；但当前没有限定 interface 4、没有 3 次重试/清空旧报告，可能读到上一轮或错误 collection。[S9 L20-L30, L116-L130][S4 L278-L282]
- **Lofree**：当前只发 start `00 00 01` 和命令 `1A`，没有先发 `AA` 在线查询；如果键盘离线/休眠时 dongle 返回 0，当前会把 0 当 0% 电量。Lofree 自己的 web driver 是先查 online 再查电量。[S4 L284-L288][S12]

## A11. ASUS ROG：默认协议正确，但 PID/接口边界过宽，少数型号偏移不同

当前实现：`extra.rs:342-389`。

- 默认 `00 12 07`、65 字节、回复 echo `12 07`、电量 byte 5、充电 byte 10，与 G-Helper 一致。[S7 L782-L815]
- 当前对**所有** `0B05` 且 `is_vendor_collection()` 的 65 字节接口发 `12 07`，没有 PID allowlist，也没有限定 `mi_00`/interface 0；ASUS 键盘/主板/其他外设的 vendor collection 也可能被误探测。HaloBattery 明确只对 interface 0 的 vendor collection 操作。[S4 L141-L145][S4 providers/asus.py L7-L14]
- 当前的 `steps` PID 列表与 HaloBattery 一致，但缺少：
  - Gladius II Wireless `0x18A0`：`packet[5] * 25`；
  - Strix Carry `0x18B4`：`packet[7] * 25`（偏移不同，不能直接加入当前 `offset+4` 分支）。  
  这两个型号 HaloBattery 因 collection/offset 不同而暂不纳入；若要支持，必须单独 parser，不能“补 PID 就行”。[S7 GladiusIIWireless L118-L126; StrixCarry L84-L92][S4 L22-L25]

## A12. Xbox / WGI 的展示与回退问题

当前实现：`xbox.rs`。

- WGI `RawGameController.TryGetBatteryReport()` 返回 `Windows.Devices.Power.BatteryReport`，用 `RemainingCapacityInMilliwattHours / FullChargeCapacityInMilliwattHours` 算百分比是正确的；官方 API 确实提供 `Status`、`RemainingCapacityInMilliwattHours`、`FullChargeCapacityInMilliwattHours`。[S11]
- 但 `Transport::Usb` 是硬编码（`xbox.rs:69`、`99`）；蓝牙 Xbox 手柄也会显示 USB。
- 没有 Xbox 360 无线 / WGI 不可用时的 XInput 四级回退。XInput 的 `XINPUT_BATTERY_INFORMATION.BatteryLevel` 只有 empty/low/medium/full 四档，不能当精确百分比。[S11 XInput][S1 xbox360w L116-L119]
- `RawGameController` 不保证只包含 Xbox；如果它同时枚举 DualSense/Switch，而 HID adapter 也读同一设备，可能产生重复卡片，需要真机确认。
- `scan.rs:123` 把 `controller_count` 固定为 0；当前 UI 没用到，但诊断/统计语义是错的。

## A13. 文档/许可证层面的两个确定问题

1. `README.md:97` 写“HeadsetControl（MIT）”，但 HeadsetControl 仓库的 `license` 是 **GPL-3.0**，不是 MIT。[S3 license]  
   仅引用协议事实通常不复制代码，但 README 的许可证陈述错误，且若未来复制其实现会直接产生 GPL 兼容性问题。
2. `README.md:33-36` 把 VAXEE / ZYGEN、Endgame Gear、Nordic nRF 标为 ✅，但当前代码：
   - 没有回包头校验；
   - VAXEE 缩放错误；
   - Endgame/Nordic 只有未授权本地小工具或冲突资料；
   - 三者都使用 `feature_roundtrip` 的“关句柄再开”模式。  
   这与 README 自己定义的“✅ = 对报文头、长度、数值范围做校验”不一致，应降级为 🟡/实验或先修实现。

---

# B. 协议 / 型号矩阵

> 说明：`—` 表示公开资料未给出或本报告未验证；`未知` 不等于 0。  
> `当前` 列只描述工作树实现，不代表已真机验证。

## B1. SteelSeries

| 型号 / 家族 | 连接 | VID:PID | 接口 / usage | 请求或被动 | 报文 / report ID | 电量、充电、离线 | 当前状态 | 证据 |
|---|---|---|---|---|---|---|---|---|
| Arctis Nova 7 原版 | 2.4G dongle | 1038:2202/2206/220A/223A/227A/22A4 | iface 3, `FFC0:0001` | output `00 B0` | 64B; reply `B0 <link> <level> <status>` | level byte 2 为 0–4，×25；link 3=连接，status 0=off，1/2=充电，3=电池 | 通用 B0 分支；低电量会被放大 | [S3 Nova7 L130-L167][S4 L7-L15] |
| Arctis Nova 7 Gen2 | 2.4G dongle | 1038:22A1/227E/2258/229E/22AD/22A9/22A5/22A7 | 同上 | 同上 | 同上 | level byte 2 为 0–100；其余同上 | 通用 B0 分支；1–4% 会显示 25–100% | [S3 Nova7 L139-L167] |
| Arctis Nova 5 / 5X | 2.4G dongle | 1038:2232/2253 | iface 3, `FFC0:0001` | `00 B0` | 64B; `B0 <status> <?> <level> <charging>` | status byte 1==02 离线；level byte 3；charging byte 4==1 | **分支反向**：在 `link==02` 时读 byte 3 | [S3 Nova5 L183-L196] |
| Arctis 7+ | 2.4G dongle | 1038:220E/2212/2216/2236 | 同上 | `00 B0` | `B0 <status> <level 0..4> <charging>` | level byte 2 ×25；status byte 1==01 离线；charging byte 3==1 | 可能落入通用 B0 分支；未按 PID 区分 | [S4 L14-L15] |
| Arctis GameBuds / X | dongle/base | 1038:230A/2317 | 同上 | `00 B0` | `B0 ... <L status> <R status> <L %> <R %>` | 取出耳塞 status==03；取较低电量；无充电 | 未实现 | [S4 L16-L20] |
| Arctis Nova Pro Wireless | base station | 1038:12E0/12E5 | iface 3/4, `FFC0:0001` | output `06 B0` | 64B | level byte 6 为 0–8 九档；state byte 15（01 off/02 charging/08 battery） | 未实现；只发 `00 B0` | [S4 L127-L131] |
| Arctis Nova Elite | base station | 1038:2244 (+2246/2249/2270 未纳入) | iface 3, `FFC0` | output `01 B0` 或被动 `07 B7` | 64B | 直接回复：level byte 6，power byte 14，charge byte 15；`07 B7`：level byte 2，charge byte 4（02 charging/08 battery） | 未实现 | [S4 L121-L125] |
| Aerox 3 Wireless | 2.4G dongle | 1038:1838/1878 | iface 3, `FFC0` | output `00 D2` | 64B; reply echo `D2` | level byte 1：bit7 充电；低 7 位 1–21 → `(v-1)*5`，>21 直接百分比；0=off/asleep | 未实现（只发 AA） | [S5 L165-L171][S6 L332-L367][S4 L225-L229] |
| Aerox 5/9 Wireless | 2.4G dongle | 1038:1852/185C/1860/1858/1874 | 同上 | `00 D2` | 同上 | 同上 | 未实现 | [S4 L225-L235] |
| Aerox 3 Gen2 | 2.4G dongle / cable | 1038:1890/1892 | 同上 | `00 D2` / `00 92` | 64B | 直接 0–100%，无 step 缩放 | 未实现 | [S6 devices L58-L70] |
| Rival 3 Wireless / Gen2 | 2.4G dongle | 1038:1830/1872 | iface 3, `FFC0` | output `00 AA 01` | 64B | 两种公开形状：`AA <level> <charging>` 或 `<level> <charging>`；也有 `AA 01 <level> <charging>` 解读 | 只支持 `AA` + byte 1 一种；不完整 | [S6 L337-L367][S4 L231-L235] |

## B2. HyperX / Corsair / ASUS

| 型号 / 家族 | 连接 | VID:PID | 接口 / usage | 请求 | 报文 / report ID | 电量 / 充电 | 当前状态 | 证据 |
|---|---|---|---|---|---|---|---|---|
| Cloud II Wireless | 2.4G dongle | 03F0:0696 / 018B | `FF90:0303`, iface 0 | 52B output `06 FF BB 02` | reply 20B, echo `06 FF BB 02` | level byte 7；充电 cmd `03` reply byte 4==1；电压 byte 5–6 BE | 已有，基本正确 | [S3 CloudII L20-L115] |
| Cloud II Wireless Kingston | 2.4G dongle | 0951:1718 | — | 62B base `06 00 02 00 9A 00 00 68 4A 8E 0A 00 00 00 BB <cmd>` | reply 64B, ID `0B`, echo `BB <cmd>` | level byte 7；充电 cmd 03 byte 4 | 未实现 | [S3 Kingston L27-L105] |
| Cloud II Core Wireless | 2.4G dongle | 03F0:069F / 0995（HaloBattery 也提到 0795） | — | 62/64B base `66 …`；cmd `89` | reply `66 89/0D <state1> <state2> <level>` | level byte 4，仅当 byte 2/3 非 0；充电 `8A/0C` byte 4（0/1/2） | 未实现 | [S8 CloudIICore L12-L22, L192-L210][S4 L33-L39] |
| Cloud III Wireless | 2.4G dongle | 03F0:05B7 / 0C9D | `FF13:0001` | 62B `66 89` | reply 62B `66 0D/89 ...` | level byte 4，仅当 byte 2/3 非 0；充电 `8A/0C` byte 4 | 未实现 | [S8 CloudIII L9, L58-L62, L177-L200][S4 L57-L61] |
| Cloud III S Wireless | 2.4G dongle | 03F0:02CC / 06BE | 64B output report 0x0C | `0C 02 03 01 00 06` | reply input 0x0C; cmd echo byte 5, value byte 6 | level byte 6；充电 cmd 0x48 byte 6 | 未实现 | [S8 CloudIIIS L30-L45, L139-L143, L76-L105] |
| Cloud Alpha 2 Wireless | base station | 03F0:08BE | `FF13:FF00`, iface 2 | 64B `50 02` | reply `51 02` | level byte 2；charge byte 3==1；电压 byte 6–7 LE | 未实现 | [S3 Alpha2 L50-L150][S4 L33-L39] |
| Corsair Void v2 / Virtuoso Max / HS80 Max | 2.4G receiver | 1B1C:2A08 / 2A02 / 0A97 | iface 4 | 65B `00 02 09 02 0F` + firmware/heartbeat 唤醒 | reply 64B | `reply[4] | reply[5]<<8` 为 0–1000，÷10；无充电 | 未实现 | [S3 Void L95-L300][S4 L21-L31] |
| Corsair Virtuoso SE / XT | receiver / cable | 1B1C:0A40 / 0A3E / 0A3D / 0A64 / 0A62 | `FF42:0001`, iface 3 | report ID 0x02，`02 09 02 0F`（0A40 为裸 64B） | reply `01 01 02 00 <value LE>` | value[4..5] LE 为十分之一百分比；充电 property 0x10==1 | 已有 PID，但请求/偏移错误 | [S3 Virtuoso L104-L135][S4 L43-L50] |
| Corsair Dark Core RGB Pro SE | 2.4G dongle | 1B1C:1B7F | `FF42:0001` | 65B `00 02 09 02 0F` | reply `01 02 00 <value LE>` | value[3..4] LE 为十分之一百分比；无充电 | 已有 PID，但偏移晚一字节 | [S4 L147-L151][S4 providers/corsair.py L24-L42] |
| ASUS ROG 默认无线鼠标 | 2.4G / cable | 0B05 多 PID | iface 0, vendor collection | 65B `00 12 07` | reply echo `12 07` | level byte 5（或 report-id 偏移后）；charge byte 10==非 0；0 且未充电=standby | 已有，协议基本正确；触发面过宽 | [S7 L782-L815][S4 L141-L145] |
| ASUS 老款离散档位 | 2.4G / cable | 18E5/18E3/1960/195E/1A59/1A57/1908/1906/1949/1947 | 同上 | `00 12 07` | 同上 | level byte 0–4 ×25 | 已有 PID 列表 | [S7 models][S4 providers/asus.py L79-L90] |
| ASUS Gladius II Wireless | 2.4G | 0B05:18A0 | 不同 collection，`mi_02` | `00 12 07` | — | packet[5] ×25 | 未实现；不能直接加进当前偏移分支 | [S7 GladiusIIWireless L118-L126][S4 L22-L25] |
| ASUS Strix Carry | 2.4G | 0B05:18B4 | 不同 collection，`mi_01` | `00 12 07` | — | **packet[7] ×25** | 未实现；偏移与默认不同 | [S7 StrixCarry L84-L92][S4 L22-L25] |

## B3. VAXEE / Endgame / Nordic / NZXT / Keychron / Lofree

| 型号 / 家族 | 连接 | VID:PID | 接口 / usage | 请求 | 报文 | 电量 / 充电 | 当前状态 | 证据 |
|---|---|---|---|---|---|---|---|---|
| VAXEE / ZYGEN | 2.4G dongle | 3057（多 PID） | usage `FF05` | feature `0E A5 0B 01 01` + zeros | 64B feature | reply[5] 为 0–20，×5；充电页 `0E A5 10 01 01`，reply[5]==1 | 已有，**少乘 5**；无充电；分离句柄 | [S14 L525-L590]（社区，未硬件） |
| Endgame Gear | 2.4G dongle | 3367（多 PID） | usage `FF01` | feature `A1 B4` + zeros | 64B feature | reply[1]==01 有效；level byte 16；mV byte 17–18；需两次读（第一次唤醒） | 已有，但无头校验/无重试/分离句柄 | 社区资料冲突；本地未授权工具 [S14 L452-L525] |
| Nordic nRF 方案 | 2.4G dongle | 1915（多 PID） | usage `FFA0` | feature `05 15 00 00 01` + zeros | 32B feature | reply[9] 0–100 | 已有，但无头校验/无重试；协议不唯一 | 仅本地未授权小工具，证据不足 |
| NZXT Lift Elite | 2.4G dongle / cable | 1E71:2101 / 2129 | `FFCA:0001` | 64B `4E 02 81 00 B0` | reply `4E 02 97` | level byte 17–18 LE；charge byte 22 bit0；CAM 打开时 dongle 不答 | 已有，协议匹配；无独立开源实现 | [S4 L249-L274]（USBPcap + 硬件确认） |
| Keychron M5 / Ultra-Link 8K | 2.4G dongle / cable | 3434:D028 / D048 | iface 4, vendor | feature 64B `B3 06` | input 64B `B4 06` | level byte 20；无充电 | 协议已有；**kind 误标 Keyboard**；无 iface 限制/重试 | [S9 L20-L30, L116-L130][S4 L278-L282] |
| Lofree Hyzen | 2.4G dongle | 388D:0025 | `FF1C:0092` | report 0x04；start `00 00 01`；cmd `AA`/`1A`；end `00 00 02` | 64B report 0x04 | level 在 reply[8]（report ID 后第 7 字节）；AA 在线，1A 电量；无充电 | 已有 1A 路径；**缺 AA 在线检查**；无重试 | [S4 L284-L288][S12]（web driver 二次资料） |

## B4. 手柄

| 型号 / 家族 | 连接 | VID:PID | 接口 / usage | 协议 | 电量 / 充电 | 当前状态 | 证据 |
|---|---|---|---|---|---|---|---|
| Xbox / GameSir / FlyDigi 等 WGI 手柄 | USB / BT / 2.4G | 多 | `Windows.Gaming.Input.RawGameController` | `TryGetBatteryReport()` → `BatteryReport` | `RemainingCapacityInMilliwattHours` / `FullChargeCapacityInMilliwattHours`；Status | 已有；Transport 固定 USB；无 XInput 回退；可能和 HID 重复 | [S11] |
| Xbox 360 Wireless | 2.4G dongle | 045E 多 | XInput / 专用 dongle | `XINPUT_BATTERY_INFORMATION` 或 SDL xbox360w | 四级 empty/low/medium/full；SDL dongle 为 0–255 线性 | 未实现 | [S11 XInput][S1 xbox360w L116-L119] |
| DualSense / Edge | USB / BT | 054C:0CE6 / 0DF2 | usage 1, 0x04/0x05 | input 0x01 / 0x31 | USB byte53 / BT byte54；高半字节 0/1/2；低半字节 0–10；0x0C=unknown | 偏移正确；状态语义错误；未处理 dongle Alt | [S1 ps5 L129, L1429-L1508][S2 hid-playstation L245-L267, L1610-L1644] |
| DualShock 4 | USB / BT / dongle | 054C:05C4/09CC/05C5/0BA0 | usage 1, 0x04/0x05 | input 0x01 / 0x11 | USB byte30 / BT byte32；低半字节 0–10，bit4 线缆；11=full，14/15=error | 偏移正确；11/14/15 语义错误 | [S1 ps4 L95, L1149-L1169][S2 hid-playstation L2548-L2589] |
| Switch Pro / Joy-Con | BT | 057E:2009/2006/2007 | usage 1, 0x04/0x05 | input 0x21/0x30/0x31 | byte2：bit7–5 档位 0–4（映射 0–8），bit4 充电 | 格式正确；无 `0x02` 子命令回退；USB 不读 | [S1 switch L2688-L2714][S2 hid-nintendo L554-L568, L1647-L1680] |
| 8BitDo D-input | BT / USB | 2DC8:6000/6100/6001/6101/6003/6006/6009 | usage 1 | input 0x01 BT / 0x04 USB；Pro3 固定 0x04 | byte14：bit0–6 百分比，bit7 充电 | 格式正确；缺 0x6012；所有型号标 Verified 过头 | [S1 8bitdo L648-L674][S1 controller_list L650-L657] |
| Steam Controller 2025 / Puck | Puck / USB / BLE | 28DE:1302/1303/1304/1305 | Puck iface 2–5, `FF00:0001` 或 gamepad | input 0x43 | byte1 charge（1/2/4），byte2 level；约 2.5s 一次 | 格式正确；**3s/450ms 窗口会漏** | [S1 steam_triton L351-L372][S4 L346-L350] |
| Steam Controller 2015 Wireless | 2.4G receiver | 28DE:1142（wired 1102；BT 1105/1106） | 64B input | report `01 00 04 …` | voltage [12..13] LE，level [14]；仅 wireless | 未实现；可新增只读 | [S2 hid-steam L2297-L2322, L2682-L2692, L2736-L2770] |
| Steam Controller V2 (HEADCRAB) | USB / BT | 28DE:1201/1202 | — | SDL 走 2015 Steam 驱动，电池是否暴露不明确 | 未知 | 未实现，证据不足 | [S1 controller_list L667-L668][S1 SDL_hidapi_steam.c L96-L102] |

---

# C. 当前代码差距（四档）

## C1. 当前已有且基本正确

| 项目 | 证据 |
|---|---|
| HyperX Cloud II `03F0:0696/018B`：52B `06 FF BB 02`、level 7、charging cmd 03 | 与 HeadsetControl 逐字段一致 [S3 CloudII L20-L115] |
| ASUS 默认 `00 12 07`：level byte 5、charging byte 10 | 与 G-Helper 一致 [S7 L782-L815] |
| NZXT Lift Elite `1E71:2101/2129`：`4E 02 81 00 B0`，level 17–18，charge 22 | 与 HaloBattery 捕获一致 [S4 L249-L274] |
| Keychron `3434:D028/D048`：`B3 06` → `B4 06` byte20 | 与 DKMS 一致 [S9 L20-L30] |
| Lofree `388D:0025`：report 0x04、`1A`、level reply[8] | 与 HaloBattery 转述/本地实现一致 [S4 L284-L288] |
| Steam 2025 0x43 的 charge/level 偏移 | 与 SDL 一致 [S1 steam_triton L351-L372] |
| DS4/DS5 的 30/32、53/54 偏移 | 与 SDL/Linux 一致 [S1 ps4/ps5][S2 hid-playstation] |
| Switch byte2 电量/充电位 | 与 SDL/Linux 一致 [S1 switch][S2 hid-nintendo] |
| 8BitDo byte14 / report ID 选择 | 与 SDL 一致 [S1 8bitdo] |
| Xbox WGI mWh 百分比 | 官方 API 支持 [S11] |

## C2. 疑似错误（应优先修）

| 项目 | 位置 | 问题 | 证据 |
|---|---|---|---|
| SteelSeries Nova 7 Gen2 低电量放大 | `extra.rs:539-540` | 1–4% 被 ×25，3%→75% | [S3 Nova7 L139-L167] |
| SteelSeries Nova 5 分支反向 | `extra.rs:552-564` | `link==0x02` 被当在线；在线反落入 Nova7 分支 | [S3 Nova5 L183-L196] |
| SteelSeries Rival 3 形状不完整 | `extra.rs:566-583` | 只支持一种 AA 回复；可能把 `AA 01 55` 读成 1% | [S5][S6 L337-L367] |
| Corsair Dark Core 偏移晚一字节 | `extra.rs:632-640` | `reply[4..5]` 应为 `reply[3..4]`，且无 route/err 校验 | [S4 L147-L151] |
| Corsair Virtuoso 请求帧错误 | `extra.rs:622-657` | 0A40 拒绝带 report-id 前缀；当前 65B 前导 0；充电属性未读 | [S3 Virtuoso][S4 L43-L50] |
| VAXEE 少乘 5 | `vendor.rs:456-471` | raw 20→20%，应 100%；无充电；分离句柄 | [S14 L525-L590]（社区） |
| DS5 状态 2 / 0x0C | `extra.rs:169-172` | 充满未置 100；0x0C 未知被显示 100% | [S1 ps5 L1481-L1508][S2] |
| DS4 11/14/15 | `extra.rs:173-176` | 满电/错误都被 `.min(10)` 成 100% 且充电 | [S1 ps4 L1149-L1169][S2] |
| Keychron M5 kind | `extra.rs:415` | 鼠标标成 Keyboard | [S9 L20-L30] |
| Steam 2025 窗口 | `extra.rs:109` + `hid.rs:808-830` | 450ms idle / 3s total，漏 2.5s 的 0x43 | [S4 L346-L350] |
| Xbox Transport | `xbox.rs:69,99` | 固定 USB | 代码事实 |
| `controller_count` | `scan.rs:123` | 永远 0 | 代码事实 |
| Lofree 离线 0% | `extra.rs:449-467` | 无 AA 在线检查，0 可能是离线而非空电 | [S4 L284-L288] |
| README 许可证 | `README.md:97` | HeadsetControl 写 MIT，实际 GPL-3.0 | [S3 license] |

## C3. 可新增（有较明确公开依据）

| 项目 | 价值 | 依据 |
|---|---|---|
| Steam Controller 2015 `28DE:1142` 被动 `01 00 04`，level 14 | 高；只读；补上 README 明确“未实现”项 | Linux `hid-steam` [S2] |
| SteelSeries Aerox 3/5/9 `00 D2` | 高；SteelSeries 鼠标目前几乎空白 | rivalcfg/steel-mouse/HaloBattery [S5][S6][S4] |
| HyperX Cloud III / III S / II Core / Alpha 2 / Kingston | 高；Cloud 系列缺失多 | HeadsetControl/HyperHeadset [S3][S8] |
| SteelSeries GameBuds / Nova Pro / Nova Elite | 中高；高端耳机 | HeadsetControl/HaloBattery [S3][S4] |
| 8BitDo `2DC8:6012` | 低风险小补丁 | SDL [S1 controller_list] |
| Xbox 360 Wireless / XInput 回退 | 中；老设备 | Microsoft XInput/SDL [S11][S1 xbox360w] |
| ASUS `0x18A0/0x18B4` | 中；必须单独 parser，不可直接加 PID | G-Helper [S7] |
| Corsair Void v2 / Virtuoso 有线 | 中；当前 PID/协议不完整 | HeadsetControl [S3] |
| 双 Sense dongle Alt report（byte 30） | 低；取决于是否存在实际 dongle | SDL [S1 ps5] |

## C4. 证据不足（不要按现有 ✅ 宣称）

| 项目 | 原因 | 建议 |
|---|---|---|
| Nordic nRF 泛化 `1915:FFA0` | 只有未授权本地小工具；且存在 `1915:0723/FF0A` 的另一套被动协议 | 降级为实验；按具体 PID/固件抓包后再写 |
| Endgame Gear 泛化 `3367:FF01` | 与 FerrisPulse 的 Beken 16B 协议冲突；当前无头校验/重试 | 保留实验；不要标 ✅；先确定 OP1we/XM2we 等具体型号 |
| VAXEE 真机 | 社区实现，测试为模拟报文 | 修 `×5` 后仍需真机对照 VAXEE 软件 |
| Lofree | 只有 Lofree web driver 的二次转述，无公开提交 | 保留 🟡；加 AA 在线检查后实测 |
| NZXT | 无独立开源实现，来源是 USBPcap + 一位硬件确认 | 可保留“社区/硬件确认”，但不要写成“开源协议” |
| Steam V2 `1201/1202` | SDL 归为 2015 Steam 驱动，电池暴露不明确 | 不实现，等报文 |
| Xbox GIP 原始百分比 | GIP 只有 4 档，不是精确百分比 | 不作为精确百分比来源；WGI 优先 |

---

# D. 实施候选排序（证据强、只读、收益高、风险低）

| 排序 | 候选 | 证据 | 写入? | 收益 | 风险 | 最小测试向量 / 验收 |
|---|---|---|---|---|---|---|
| 1 | **Steam 2025 监听窗口改为 5s，去掉 450ms 提前退出** | SDL + HaloBattery 硬件 | 只读 | 高；当前很可能读不到 | 低 | `43 01 5f`→95 放电；`43 04 64`→100 充电；空 slot 5s 有界返回 |
| 2 | **SteelSeries B0 按 PID 分表**：Nova7 离散/百分比分开；Nova5 修反向；Arctis7+/GameBuds/NovaPro 分 parser | HeadsetControl + HaloBattery | 只读（B0） | 高；修 3%→75% 与 Nova5 | 中；PID 表需准确 | `B0 03 03 03`→3%（Gen2）；`B0 03 01 03`→25%（旧）；Nova5 `B0 03 00 55 00`→85%；离线 `B0 02 …`→None |
| 3 | **Corsair 拆 Dark Core / Virtuoso / Void v2 三套 parser** | HeadsetControl + HaloBattery 实测 | 只读 get-property | 高；当前几乎全错 | 中；Windows 帧格式需实测 | Dark Core `01 02 00 26 02`→55%；Virtuoso `01 01 02 00 ca 03`→97%；0/`>1000`→拒绝 |
| 4 | **DualSense/DS4 状态机按 SDL/Linux 修正** | SDL + Linux | 只读 | 高；避免 100% 假读数 | 低 | DS5 `0x2?`→100；`0x0C`→Unknown；DS4 11→100、14/15→Unknown |
| 5 | **Keychron 修 kind、限 interface 4、3 次重试/清 stale** | DKMS | 只读 | 中；正确分类 | 低 | `B4 06` byte20=55→55%；kind=Mouse；错误接口不触发 |
| 6 | **VAXEE `×5` + `AA` 充电页 + 同句柄** | FerrisPulse（社区） | 只读 | 中高 | 中；需真机确认 | raw 20→100%；raw 16→80%；`10` 页 1→charging |
| 7 | **ASUS 默认 parser 加 PID/interface allowlist；单独加 18A0/18B4** | G-Helper | 只读 | 中；减少误探测 | 中；18B4 偏移 7 | 18A0 level5×25；18B4 level7×25；非鼠标 0B05 不触发 |
| 8 | **HyperX 按家族逐个加**：Cloud III/III S/II Core/Alpha2/Kingston | HeadsetControl/HyperHeadset | 只读 | 高；覆盖缺口大 | 中高；每家族帧不同 | 每家族一个真实 62/64B fixture，level/charging 双态 |
| 9 | **Steam 2015 `1142` 被动 parser** | Linux 内核 | 只读 | 中高；补明确缺口 | 中；无 Windows 实测 | `01 00 04 … 12 13 39`→level 57；仅 1142；64B 严格 |
| 10 | **8BitDo 加 6012** | SDL | 只读 | 低中 | 低 | 6012 BT `01`/USB `04`，byte14 |
| 11 | **Xbox 360 / XInput 回退 + Transport 修正** | Microsoft/SDL | 只读 | 中 | 中；XInput 四档要标 coarse | XInput 4 档→Coarse；BT 手柄→Bluetooth |
| 12 | **README/证据等级修正**：HeadsetControl 许可证、VAXEE/Endgame/Nordic 降级、Steam 2015 说明 | 文档 | 无 | 高；避免错误承诺 | 低 | 与代码实际校验能力一致 |

> 不建议现在把 Nordic/Endgame 的泛化适配继续“修到 ✅”；应先拿到具体型号的 HID 描述符 + 一次官方软件运行时的完整交换。

---

# E. 主代理串行验证步骤（Windows 真机）

1. **冻结基线**：记录 `git rev-parse HEAD`、`git status --porcelain`、`cargo test -p anybattery-core`；确认本报告不引入代码 diff。基线当前 12/12 通过。
2. **先写 parser 单测，不接设备**：把 D 表的向量写成纯函数测试；特别是：
   - SteelSeries `B0 03 03 03` → 3%，不是 75%；
   - SteelSeries Nova5 `B0 03 00 55 00` → 85%；
   - Corsair `01 02 00 26 02` → 55%；
   - VAXEE raw 20 → 100%；
   - DS5 `0x0C` → Unknown；
   - DS4 14/15 → Unknown；
   - Steam 2015 `[1,0,4,...,14=57]` → 57。
3. **同一句柄验证**：对 VAXEE/Endgame/Nordic，改成 SetFeature/GetFeature 同一 `HANDLE`，中间只做必要 sleep；用 Process Monitor/诊断日志确认没有“关句柄再开”导致的空回复。
4. **逐品牌真机串行**（每步只接一台，避免 container 合并干扰）：
   - SteelSeries：Nova 7 Gen2、Nova 7 旧固件、Nova 5、Arctis 7+、GameBuds；Aerox 3、Rival 3。
   - Corsair：Dark Core RGB Pro SE、Virtuoso SE/XT、Void v2。
   - HyperX：Cloud II、Cloud III、Cloud III S、Cloud II Core、Cloud Alpha 2。
   - ASUS：18E5/18E0 系列、18A0、18B4、1A72。
   - VAXEE/Endgame：先确认具体型号，再抓描述符和 raw reply。
   - Keychron M5、Lofree Hyzen、NZXT Lift Elite。
5. **控制器矩阵**：Xbox Series USB/BT、DS5 USB/BT、DS4 USB/BT、Switch Pro/Joy-Con BT、8BitDo Pro 2/Pro 3 BT/USB、Steam 2025 puck 四个 slot、Steam 2015 无线接收器。
6. **状态与边界**：每台设备测试“在线/充电/满电/离线/休眠/唤醒”六态；确认：
   - 离线不显示旧值为 live；
   - 0 不被当空电或满电；
   - 超时有界（Steam 5s、Corsair 唤醒窗口、Keychron 500ms×3）；
   - 厂商软件（GG、NGENUITY、iCUE、CAM、Synapse）开/关各一次。
7. **去重**：同一接收器 + 线缆、WGI + HID、蓝牙 PnP + HID 各只应一张卡；检查 `key_suffix` 与 container 合并。
8. **证据回填**：把真实 raw 报文、VID/PID、usage、长度、时间戳写入 `docs/protocol-research/` 对应报告；只有完成真机验证的项才从 🟡 升 ✅。

---

# F. 来源、提交与许可证

> 证据强度：**强** = 官方文档 / Linux 内核 / SDL + 硬件或独立实现；**中** = 一手开源实现或单一 USBPcap + 硬件确认；**弱** = 单一社区实现/无许可证/未硬件验证。  
> 本报告只引用协议事实，不复制 GPL 代码。

### 一手/强证据

- **[S1] SDL3**，commit `c7b85f68820195b714cfcde1f2509456a833611a`，zlib license。  
  https://github.com/libsdl-org/SDL/tree/c7b85f68820195b714cfcde1f2509456a833611a  
  - PS5：`src/joystick/hidapi/SDL_hidapi_ps5.c#L129`、`#L1429-L1508`  
  - PS4：`src/joystick/hidapi/SDL_hidapi_ps4.c#L95`、`#L1149-L1169`  
  - Switch：`src/joystick/hidapi/SDL_hidapi_switch.c#L2688-L2714`  
  - 8BitDo：`src/joystick/hidapi/SDL_hidapi_8bitdo.c#L648-L674`  
  - Steam 2025：`src/joystick/hidapi/SDL_hidapi_steam_triton.c#L351-L372`、`steam/controller_structs.h#L678-L697`  
  - GIP：`src/joystick/hidapi/SDL_hidapi_gip.c#L1434-L1486`  
  - Xbox 360W：`src/joystick/hidapi/SDL_hidapi_xbox360w.c#L116-L119`  
  - 控制器 ID 表：`src/joystick/controller_list.h#L655-L673`

- **[S2] Linux kernel**，commit `d135ab453a7400a34930bdda1ef4bdc8acfac80e`，GPL-2.0。  
  https://github.com/torvalds/linux/tree/d135ab453a7400a34930bdda1ef4bdc8acfac80e  
  - `drivers/hid/hid-steam.c#L2297-L2322`（2015 电池）、`#L2499-L2527`（2025 电池）、`#L2622-L2640`、`#L2682-L2692`、`#L2736-L2770`  
  - `drivers/hid/hid-playstation.c#L122-L126`、`#L245-L267`、`#L1610-L1644`、`#L2548-L2589`  
  - `drivers/hid/hid-nintendo.c#L554-L568`、`#L1647-L1680`  
  - `drivers/hid/hid-ids.h#L1410-L1417`

- **[S11] Microsoft Learn**：  
  - `RawGameController.TryGetBatteryReport`：https://learn.microsoft.com/en-us/uwp/api/windows.gaming.input.rawgamecontroller.trygetbatteryreport  
  - `BatteryReport`（`Status`、`RemainingCapacityInMilliwattHours`、`FullChargeCapacityInMilliwattHours`）：https://learn.microsoft.com/en-us/uwp/api/windows.devices.power.batteryreport  
  - `BatteryStatus`：https://learn.microsoft.com/en-us/uwp/api/windows.system.power.batterystatus  
  - `XInputGetBatteryInformation`：https://learn.microsoft.com/en-us/windows/win32/api/xinput/nf-xinput-xinputgetbatteryinformation  
  - `XINPUT_BATTERY_INFORMATION`：https://learn.microsoft.com/en-us/windows/win32/api/xinput/ns-xinput-xinput_battery_information  
  - HID 读取语义：https://learn.microsoft.com/en-us/windows-hardware/drivers/hid/obtaining-hid-reports

### 中/强社区实现

- **[S3] HeadsetControl**，commit `8b4b711bdeacf91a867e68409e6668e83b49f62e`，**GPL-3.0**（不是 MIT）。  
  https://github.com/Sapd/HeadsetControl/tree/8b4b711bdeacf91a867e68409e6668e83b49f62e  
  - SteelSeries Nova7：`lib/devices/steelseries_arctis_nova_7.hpp#L130-L167`  
  - SteelSeries Nova5：`lib/devices/steelseries_arctis_nova_5.hpp#L171-L203`  
  - Corsair Void v2：`lib/devices/corsair_void_v2w.hpp#L95-L300`  
  - Corsair Virtuoso：`lib/devices/corsair_virtuoso_xt.hpp#L104-L135`  
  - HyperX Cloud II：`lib/devices/hyperx_cloud_2_wireless.hpp#L20-L115`  
  - HyperX Cloud II Kingston：`lib/devices/hyperx_cloud_2_wireless_kingston.hpp#L27-L105`  
  - HyperX Cloud Alpha 2：`lib/devices/hyperx_cloud_alpha_2_wireless.hpp#L50-L150`  
  - 许可证：`https://github.com/Sapd/HeadsetControl/blob/8b4b711bdeacf91a867e68409e6668e83b49f62e/license`

- **[S4] HaloBattery**，commit `2251f0534a8e886c9692ead42056059b4f69d8c0`，MIT。  
  https://github.com/HeyOkay/HaloBattery/tree/2251f0534a8e886c9692ead42056059b4f69d8c0  
  - 协议总表：`docs/protocols.md#L109-L131`（SteelSeries）、`#L141-L151`（ASUS/Corsair）、`#L225-L235`（SteelSeries 鼠标）、`#L249-L288`（NZXT/Keychron/Lofree）、`#L316-L350`（8BitDo/Switch/PS/Steam）  
  - 实现细节：`providers/steelseries.py#L1-L67`、`providers/corsair.py#L1-L60`、`providers/steam.py#L80-L113`、`providers/keychron.py`、`providers/lofree.py`、`providers/nzxt.py`、`providers/asus.py#L56-L90`

- **[S5] rivalcfg**，commit `779ea52c68cbf8d21234d9e0d81718225f680819`，WTFPL。  
  https://github.com/flozz/rivalcfg/blob/779ea52c68cbfd21234d9e0d81718225f680819/rivalcfg/devices/aerox3_wireless_wired.py#L165-L171  
  （正确 SHA：`779ea52c68cbf8d21234d9e0d81718225f680819`；上条 URL 的 SHA 以该值为准）

- **[S6] steel-mouse**，commit `eaa309669fd1818b0a63f57866db8dc02426b70b`，MIT。  
  https://github.com/yurtemre7/steel-mouse/blob/eaa309669fd1818b0a63f57866db8dc02426b70b/src/protocol/hid.rs#L332-L367  
  https://github.com/yurtemre7/steel-mouse/blob/eaa309669fd1818b0a63f57866db8dc02426b70b/src/protocol/devices.rs#L35-L70

- **[S7] G-Helper**，commit `0a1538ac4fb0337f6cf3d64a907acde10c91a3d9`，GPL-3.0。  
  https://github.com/seerge/g-helper/blob/0a1538ac4fb0337f6cf3d64a907acde10c91a3d9/app/Peripherals/Mouse/AsusMouse.cs#L782-L815  
  `Models/GladiusIIWireless.cs#L118-L126`、`Models/StrixCarry.cs#L84-L92`、`Models/Chakram.cs`、`Models/KerisWireless.cs`、`Models/PugioII.cs`、`Models/StrixImpactIIWireless.cs`

- **[S8] HyperHeadset**，commit `782cc445ead5caf6e4461c91cbcfa733665e1751`，MIT。  
  https://github.com/LennardKittner/HyperHeadset/blob/782cc445ead5caf6e4461c91cbcfa733665e1751/src/devices/cloud_iii_wireless.rs#L9-L62  
  `cloud_iii_wireless.rs#L177-L200`、`cloud_ii_core_wireless.rs#L12-L22`、`cloud_iii_s_wireless.rs#L30-L45`、`#L139-L143`

- **[S9] keychron-battery-dkms**，commit `7cd5149b5905b571f6bfe5ba75c4e805f78e04cb`，GPL-2.0-only。  
  https://github.com/csutcliff/keychron-battery-dkms/blob/7cd5149b5905b571f6bfe5ba75c4e805f78e04cb/keychron_battery.c#L20-L30  
  `#L116-L130`

### 弱/辅助证据

- **[S12] Lofree 官方 web driver**：`https://hyzen.lofree.tech`，无 git commit、代码混淆；本报告只按 HaloBattery 的转述记录，标“未验证”。
- **[S14] FerrisPulse**，commit `4917a10e238ed3dba2938969752886353c8ff5e9`，GPL-3.0。  
  https://github.com/Ferris-echo/FerrisPulse/blob/4917a10e238ed3dba2938969752886353c8ff5e9/src/Hardware/UniversalGamingDeviceHelper.cs#L452-L590  
  VAXEE 的 `×5`、充电页 `0x10` 来自这里；Endgame 的 Beken 16B 与 current `A1 B4` 冲突也来自这里。其测试是模拟报文，不能当硬件验证。
- **[S10] OpenRGB**，commit `da927bbf0e19493b407afce047f2e6156df86e2c`，GPL-2.0。  
  https://github.com/calcprogrammer1/OpenRGB/tree/da927bbf0e19493b407afce047f2e6156df86e2c  
  已检索 `battery`；只有 ASUS Aura 鼠标的 “Battery” **RGB 灯效模式**，没有电量遥测。因此 OpenRGB 不能作为这些设备的电量协议来源。
- **本地未授权工具**：`/root/Anybattery/.pi/research/2.4G-battery-viewer`（无 LICENSE）、`EZ2.4G`（无 LICENSE）中有 Endgame/Nordic/VAXEE 的帧，但**不能作为公共合并依据**；仅用于解释当前代码来源和冲突。
- **[S13] AnyBattery 工作树**：HEAD `70906c7`，工作树含未提交 UI/协议修改；本报告引用的行号均为工作树行号。`README.md:97` 的 HeadsetControl 许可证陈述与 S3 冲突。

### 许可证结论

| 项目 | 许可证 | 备注 |
|---|---|---|
| SDL3 | zlib | 可引用协议事实 |
| Linux kernel | GPL-2.0 | 只引用事实，不复制代码 |
| HeadsetControl | **GPL-3.0** | README 写 MIT 是错误；不要复制实现 |
| HaloBattery | MIT | 可参考，但仍应作为二次来源 |
| rivalcfg | WTFPL | 可参考 |
| steel-mouse | MIT | 可参考 |
| G-Helper | GPL-3.0 | 只引用协议常量/布局 |
| HyperHeadset | MIT | 可参考 |
| keychron-battery-dkms | GPL-2.0-only | 只引用协议事实 |
| OpenRGB | GPL-2.0 | 未发现电量遥测 |
| FerrisPulse | GPL-3.0 | 社区，未硬件验证 |

---

**Changed files:** 无。本次为只读研究，未修改 AnyBattery 代码/docs/README/release；工作树原本的未提交修改和 `docs/protocol-research/` 未触碰。

**Validation:** `cargo test -p anybattery-core` 12/12 通过；`git diff --check` 退出 0；`git diff --cached --name-only` 为空；未运行任何设备扫描/探针。

**Open risks/questions:** 所有 HID 结论仍需 Windows 真机；VAXEE/Endgame/Nordic 证据弱；Corsair Virtuoso 的 Windows 帧格式、SteelSeries Rival 3 回复形状、Steam 2015 Windows 报文仍需 fixture；RawGameController 与 HID 适配器是否重复需实测。

**Recommended next step:** 先做 D 表第 1–4 项（Steam 窗口、SteelSeries B0、Corsair parser、PS 状态机）的纯 parser 单测与 Windows 串行真机验收，再决定是否扩展 HyperX/Steam 2015。