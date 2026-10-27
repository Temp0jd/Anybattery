> 状态：子代理研究输入，主代理逐项核验后才会进入实施。该 agent 未能读取工作树，C 节不是代码审查结论。

研究完成。以下为 Lane 1 简报全文（本会话无文件系统/执行工具，无法读取工作树，故 C 节以"行为对照 + 待核对清单"形式给出，已在覆盖状态中标注）。

---

# Lane 1 研究简报：2.4G 键盘电量协议（3554:FA09 及同族）

> 证据等级说明：**[HW]** 作者声明真机验证；**[SRC]** 一手源码/文档直读；**[2SRC]** 两个独立实现一致；**[INF]** 本文推断；**[?]** 未解决/冲突。

## 0. 结论先行

1. `3554:FA09` 的 `13 05 …` **不是电池查询，而是型号识别查询**；你拿到的 `13 05 01 00 0A 03 00 00 00 00 6E …` 是一帧**校验和正确、设备真实返回的型号应答**，型号码 = 第 10 字节 `0x6E`。因此"等不到 `13 0A`"不是失败，而是**等错了包**。
2. `13 0A` 是 **CMD `0x0B` 的应答首两字节**（`0x0B` 的应答 command 回显为 `0x0A`），不是 `0x05` 的第二个包。[2SRC]
3. 在 AULA F75/F87/L65 平台，2.4G 电量命令是 **`0x4A`**，应答 `13 4A 01 00 02 <level> <state>`；两处独立实现一致，其中一处真机验证。[2SRC][HW]
4. 在你的 `0x6E` 设备上 `0x4A` 连续 15 次无响应，而 `0x05` 有响应 → 链路是醒的，**该固件大概率不实现 `0x4A`**（推断）；也可能是型号应答里 `byte11=0x00` 暗示键盘未与接收器在线（见 A3）。
5. **高危陷阱**：在 RK 的 `3554:FA09` 接收器上，`0x0B` 是 **SetScreenParam（写屏幕时钟）**，不是读。因此"直接发 `0x0B` 试试"不能无条件执行，必须先确认是 AULA/BY Tech 家族。
6. 同 VID/PID 多型号的可靠二次识别 = `0x05` 返回的 6 字节型号码；已收集 10 个已知码，`0x6E` 不在任何公开表中。

## A. 最重要新发现

### A1. `0x05` 是身份查询，应答布局已确定 [2SRC]

请求（20 字节，hidapi 需带 report ID 前缀）：

```
13 05 01 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 19
```

应答布局（本家族统一的多页帧）：

```
[0]=0x13  [1]=cmd  [2]=pages  [3]=page_index  [4]=len  [5..]=data  [19]=sum(bytes[0..18])&0xFF
```

`0x05` 应答：`[1]=0x05 [2]=0x01 [3]=0x00 [4]=0x0A`，`data[0..5]` = 6 字节 PSD（型号码），`data[6]` = 状态字节，其余含义未知。OpenRGB 的抓包给出同族实例：`13 05 01 00 0A 03 00 00 00 00 A4 01 00 00 00 00 00 00 00 CB`，并明确"第 10 字节是型号 ID，`0xA4`=F99，`0xCD`=F75"。 [1][2][3]

你的设备返回：

```
13 05 01 00 0A 03 00 00 00 00 6E 00 00 20 10 00 00 00 00 C4
```

- 校验：`0x13+0x05+0x01+0x0A + 0x03+0x6E+0x20+0x10 = 0xC4`，**完全吻合** → 真帧，不是回显。（若代码把请求缓冲当应答打印，不可能凭空出现未知型号码 `0x6E`，这是强推断 [INF]。）
- 已知型号码表见 B1；`0x6E` 未知。
- 与 F99 抓包相比：`data[6]`（原始 byte11）你的是 `0x00`，F99 是 `0x01`；`data[7..9]` 你的是 `00 20 10`，F99 是 `00 00 00`。**含义未知，不得解释为电量或版本号**。

### A2. `13 0A` 的真正来源是 `0x0B` [2SRC]

dnviti 的 F99 Pro 文档把 `0x0B` 记为 "Get Firmware Version"，应答 `byte[1]=0x0A`、`byte[5]=fw`、`byte[6]=电池/状态`、`byte[7]=连接类型(0x01 无线)` [2]；OpenKey 的适配器把常量写成 `VERSION: 0x0b, VERSION_REPLY: 0x0a`，并把数据区解析为 `data[0]=固件, data[1]=电量, data[2]==0x01 表示无线在线`：

```ts
// packages/protocols/src/families/aula-receiver/protocol.ts
async version() {
  const [p] = await this.read(Cmd.VERSION, Cmd.VERSION_REPLY); // 0x0B -> 0x0A
  return { firmware: p[4]!, battery: p[6] === 0x01 ? p[5]! : null };
}
```
（`p` 已去掉 report ID，`p[4]=data[0]`，`p[5]=data[1]`，`p[6]=data[2]`，对应原始 byte5/6/7）[3]

请求应为：

```
13 0B 01 00 00 … 00 1F
```

**注意冲突 [?]**：dnviti 自己的 Python 实现把 `d[6]` 同时当作 `firmware_lo` 和 `battery`，而 OpenKey 把它当 `battery`、`d[5]` 当 firmware。三处文档一致指向"byte5=fw，byte6=电池，byte7=0x01 表示无线在线"，Python 的字段命名是 bug；但该命令**只有 dnviti 一个源头**，OpenKey 自己把电池标为 `INFERRED / one source`。**证据强度：中，未真机验证过电量数值。**

### A3. `0x4A` 是 AULA F75/F87/L65 平台的真机验证路径 [HW][2SRC]

- Womier L65 的 `l65ctl.py`：
  ```python
  def battery(self):
      self._write20(self._packet(0x4A, 0))
      ... if (r[1] & 0x7F) == 0x4A:
          return r[5], r[6]     # byte5 = level %, byte6 = status FLAGS
  ```
  注释实测："满电、未插线：`64 10` → 100%, 0x10"。 [1]
- `aula-f87-control`（Rust，0.5.3）独立实现：请求 `13 4A 00…`，应答 `13 4A 01 00 02 <level> <state> … sum`，并称 `state 0x01 = 电池供电，0x10 = 插线（此时 level 恒为 100）`。 [7]

**冲突 [?]**：同样是 `64 10`，L65 作者说是"满电未插线"，F87 作者说是"插线中"。两者无法同时成立。**结论：`byte6` 的 `0x10` 语义未定，绝不能据此显示"充电中"。**

若你的设备应答 `0x4A`，期望形态（可直接做单测向量）：

```
13 4A 01 00 02 64 10 … <sum>
        │  │  │  │  └ byte6 状态（未解码）
        │  │  │  └──── byte5 电量（1..100）
        │  │  └─────── len=2
        │  └────────── page 0
        └───────────── cmd 回显 0x4A
```

**为什么你的 15 次无响应值得重视**：`l65ctl` 重试 4 次 × 1.5 s，`aula-f87` 重试 2 次 × 1 s，OpenKey 1 s 超时；15 次远超正常丢包率。加上 `0x05` 能应答，**要么该固件不支持 `0x4A`，要么 `0x05` 是接收器缓存应答、键盘其实未在线**（`byte11=0x00` 支持后者）。 [INF]

### A4. 同 VID/PID 多型号识别：型号码表（已收集）

| 型号码（PSD 6 字节） | 机型 | 来源 |
|---|---|---|
| `03 00 00 00 00 CD` | AULA F75 | leisiji 厂商配置 `assets/index.json`（commit 2dd3480）[4] |
| `03 00 00 00 00 8F` | AULA F87 | 同上 [4] |
| `03 00 00 00 01 0B` | AULA F87 PRO | 同上 [4] |
| `03 00 00 00 00 A9` | AULA F87 PRO Wired | 同上 [4] |
| `03 00 00 00 00 A4` | AULA F99 | 同上 + OpenRGB 抓包 [4][1] |
| `03 00 00 00 01 58` | AULA F99 Pro | 同上 + dnviti + KB.ini [4][2] |
| `03 00 00 00 00 9A` | AULA F99 Wired | 同上 [4] |
| `03 00 00 00 03 7C` | AULA H108 | 同上 [4] |
| `03 00 00 00 01 CA` | Womier L65 | womier 协议文档（有线 `0x82` 返回 "Psd"）[1] |
| `173 / 0xAD` | LANGTU T99 | SignalRGB 补丁仓库的 firmware model ID [12] |
| **`03 00 00 00 00 6E`** | **未知（你的设备）** | 本次实测帧 |

机理：`0x05` 的 PSD 与有线协议 `0x82` 返回的是**同一套型号 ID**（OpenRGB 明确写 "the IDs are the same as used in the wired protocol"）[1]。所以"接线时读一次型号、无线时复用"是可行的降级方案。

### A5. `0x13` 命令空间不是"一个协议"，至少要分三套语义

| 命令 | AULA/BY Tech（3554:FA09 等） | RK（3554:FA09 的 L 系列 / 258A:0150 M87） | 备注 |
|---|---|---|---|
| `0x05` | 身份/型号码（读）[1][2][3] | 未见于公开资料 | |
| `0x07` | OEM 软件启动首包，含义未知 [1] | **GetDongleStatus（读，无副作用）**，payload byte1=1 表示键盘醒着 [6] | 同名不同义 |
| `0x0B` | 固件+电量（读，应答 `0x0A`）[2][3] | **SetScreenParam（写：音量/CPU/内存/日期时间）** [6] | **最危险的重名** |
| `0x0A` | 保存/提交（F87 写 `13 0A 01 00 02 04 07`）[7] | — | |
| `0x4A` | 电池（读）[1][7] | — | |
| `0x41/0x43/0x44/0x49` | 键位表 / FN 层 / 配置页 / 调色板（读，10~36 页）[2][7] | — | 写 = 读 & 0x0F（`01/03/04/09`）[2] |
| `0x88` | 实时灯效帧 [1][7] | — | **在 AKKO 家族 `05 88` 才是电池事件**，见 A7 |

### A6. `0x13` 帧的构造细节（可直接照做）

- 20 字节，report ID `0x13`，payload 19 字节。
- 请求：`[0]=0x13, [1]=cmd, [2]=0x01, [3]=0x00, [4..18]=data, [19]=sum(0..18)`。设备**不校验**请求校验和，但应正确计算。 [1][2][3]
- 应答：`[1]=cmd 回显（或 cmd-1 的 `0x0A`）, [2]=总页数, [3]=页号, [4]=本页长度(≤14), [5..]=数据, [19]=校验和`。大块数据（配置 128B=10 页、键位 504B=36 页）由设备**主动连续推送**，无需逐页请求。 [2]
- **同一接口混入非配置报告**：`0x03`(1B 系统键)、`0x04`(20B NKRO)、`0x05`(2B 媒体键)、`0x07`(7B 鼠标)。解析器必须校验 `len==20 && r[0]==0x13 && r[1] 匹配 && checksum`，否则会把媒体键当配置包。 [2]

### A7. 跨品牌电量语义对照（用于设计统一抽象）

| 家族 | 传输 | 触发 | 电量 | 状态位 | 证据 |
|---|---|---|---|---|---|
| AULA 2.4G | 20B `0x13` | `0x4A` 读 | `r[5]` % | `r[6]` 未解码（0x10 冲突） | [HW] [1][7] |
| AULA F99 Pro 2.4G | 20B `0x13` | `0x0B` 读 | `r[6]` % | `r[7]==1` 无线在线 | 单源 [2][3] |
| Ajazz AK820 Max Plus 2.4G | 64B out, report ID 0 | `0x0E` 读 | `resp[5]` %（≤100 才采用） | `resp[6]`: 0=无数据 1=充电 2=电池 3=满电 | **[HW]** `64 03` 插线、`61 01` 2.4G [9] |
| AKKO/MonsGeek 2.4G | 65B feature，report 0 | `0xF7` 读（dongle 本地） | status byte1 % | byte3 充电 0/1 | [SRC] [10] |
| AKKO/MonsGeek 事件 | EP2 中断 | 键盘主动推送 | `05 88 00 00 <level> <flags>` | flags bit0 在线、bit1 充电 | [SRC] [10] |
| ROYUAN（Redragon K736 PRO/Fantech/Attack Shark） | 接收器状态 | `0xF7` | status byte1 % | byte3：0=键盘在线；byte5=可转发 | **[HW]** [11] |
| Lofree Hyzen / Flow Lite84 | report 0x04, `0xFF1C:0x92` | 三段式 `00 00 01` → cmd → `00 00 02` | cmd `0x1A` 的 byte7 % | 无 | Hyzen 未验证；Flow Lite84 **[HW]** [13] |
| Keychron QMK + 2.4G | raw HID | `0xA4`（USB 拉取） | 百分比+mV | 充电状态+transport+model_id | 2.4G 下改为键盘**主动推送** [14] |
| Keychron Ultra-Link/M5 | 64B feature | `b3 06` → `b4 06` | byte20 % | — | 未验证 [13] |

**陷阱（务必写进代码注释）**：`0x88` 在 AULA 是"实时灯效流"，在 AKKO 家族 `05 88` 才是电池事件；`0x05` 在 AULA 是身份、在 AKKO 事件表里是"profile 变更"的事件类型。**跨厂商共用同一 report ID 时，绝不能共用解析器。**

## B. 协议/型号矩阵（本 lane 全量）

### B1. 3554:FA09 家族（SinoWealth / Compx / "CX"）

| 机型 | 连接/ID | 通道 | 身份 | 电池 | 证据 |
|---|---|---|---|---|---|
| AULA F75 / F87 / F87 Pro / F99 / H108 | 有线 258A:010C | 消费级 `FF00:0001`，feature 6（部分固件 9），520B | `82 01` → PSD | 无线型号：`87` 读 2 字节 | [SRC] L65 文档；F87/F99 实测于 F75 [1][4][7] |
| Womier L65（F75 平台） | 有线 258A:010C / 无线 3554:FA09 | 无线 `FF02:0002`，`0x13` 19B | `05` | **`0x4A`** → `r[5]`, `r[6]` | **[HW]** 15 次/4 次重试逻辑 [1] |
| AULA F87 Pro | 3554:FA09 | 同上 | `05` | `0x4A` | **[HW]**（作者自述）[7] |
| AULA F99 / F99 Pro | 3554:FA09 | 同上 | `05` → `…A4` / `…01 58` | 声称 `0x0B`；但 F99 作者明确写 "Battery level … no read command is known" | **冲突** [2][3][5] |
| LANGTU T99 | 3554:FA09 | 同上 | model 0xAD | 未实现（仅灯效） | [SRC] [12] |
| RK L 系列 | 3554:FA09（OpenKey 明示） | 同上 | 未明 | `0x0B` 是写屏幕参数 | [SRC] [3][6] |
| **你的设备 0x6E** | 3554:FA09，`FF02:0002`，in/out 20B，feature 0 | 同上 | `05` → `…00 6E` | `0x4A` 15 次无应答 | 本次实测 |

### B2. 其他 2.4G 键盘（不同家族，不可混用）

| 品牌/型号 | VID:PID | 帧 | 电量 | 证据 |
|---|---|---|---|---|
| Ajazz AK820 Max Plus | 1a2c:a036 有线 / 1a2c:8fff dongle | 64B output，report ID 0，`AA 55 CC 33 <cmd>` | `0x0E` → `resp[5]` %，`resp[6]` 状态 | **[HW]** [9] |
| AKKO / MonsGeek M1 V5 HE 等 | 3151:5030 有线 / 3151:5038 dongle | 65B feature，report 0，checksum 为 255−sum | `0xF7` 9B 状态；EP2 `05 88` 事件 | [SRC]（F7 为 dongle 本地，无 RF）[10] |
| MCHOSE G75 / G75 Pro | 41E4:2001 | 20B output `0x13`，`13 02 1B idx 0E …` | 未公开 | [SRC] [12b] |
| MCHOSE G98 V2 | 41E4:2203 | 有线 feature report 9；无线 "report 19 分包+应答" 未实现 | 未公开 | [SRC] [12c] |
| RK M75 | 258A:0163 有线 / 258A:0148 无线 | 无线 `13 88 XX`，8/18 个 20B 包 | 未实现 | **[HW]**（灯效）[15] |
| RK M87（带屏） | 258A:0150 | 20B `0x13` | 无 | **[HW]** `0x07` 状态 [6] |
| ROYUAN 系（Redragon K736 PRO、Fantech、Attack Shark） | 3151 为主，另有 379a/374a/38a9/046a/2ea8/145f | 接收器 `0xF7` | status byte1 % | **[HW]** [11] |
| Lofree Hyzen / Flow Lite84 | 388D:0025（Flow Lite84 同族会话） | report 0x04，`0xFF1C:0x92` | `0x1A` byte7 % | Flow Lite84 **[HW]** [13] |
| Keychron QMK（K10 HE/Q1 HE/V1 Max） | 各机型 | raw HID `0xA4` | 百分比+mV+充电+型号 | 需刷补丁固件；2.4G 为推送 [14] |
| Keychron Ultra-Link 8K / M5 | 3434:D028 dongle / 3434:D048 线 | 64B feature `b3 06`→`b4 06` | byte20 % | 未验证 [13] |
| Delux 键盘 | — | **未找到任何公开 HID 协议** | — | 只有鼠标 M800 Pro 有开源驱动 [16] |

## C. 与当前代码逐项差距

> **限制**：本 session 无 bash/文件读取工具（`fabric_exec` 不可用，`fetch_content` 只支持 HTTP(S)），**未能读取 `/root/Anybattery` 工作树**。以下为"据任务描述的现象 + 已确证协议"推出的必查项，不是对现有代码的断言。

| # | 应满足的行为 | 现状/风险 | 依据 |
|---|---|---|---|
| C1 | 打开 `FF02:0002`、report ID `0x13`、in/out 各 19B 的集合，`O_RDWR` | 若用 hidapi 的 `send_feature_report` 会失败——该接口 **feature=0** | [1][2][3] |
| C2 | 请求必须是 `13 05 01 00 00…00 19`（数据区全 0） | 现象里出现 `13 05 01 00 0A 03 00 00 00 00 6E…` 这种**应答形态**被当作"查询结果"，说明请求/应答缓冲曾混用或设备回显未被区分 | A1 [INF] |
| C3 | 解析前校验 `len==20 && r[0]==0x13 && checksum` | 未校验时，NKRO(0x04)/媒体键(0x05, 2B)/鼠标(0x07) 报告会污染解析 | [2] |
| C4 | `0x05` 的结论应是"识别成功，型号码=0x6E"，而不是"电池查询失败" | 当前把它当电池流程的一步，导致误判 | A1 |
| C5 | 期望 `13 0A` 必须挂在 `0x0B` 之后 | 挂在 `0x05` 之后 → 永远等不到 | A2 |
| C6 | `0x4A` 解析：`r[1]&0x7F==0x4A`，`r[4]==2`，`r[5]` 百分比，`r[6]` 原样保留 | 若按"0x05 的后续包"解析则永远不匹配 | [1][7] |
| C7 | 失败语义三分：休眠/离线 vs 不支持 vs 校验错 | 15 次无响应被记为"不支持"或"无电池"，但未区分 `byte11` 在线标志 | A3 [INF] |
| C8 | 型号分派表，禁止按 PID 写死 | 同 PID 下有 AULA/RK/LANGTU/Womier 多族 | [1][3][4][12] |
| C9 | 不得把 `byte6==0x10` 显示为"充电中" | 两处实测解释互相矛盾 | A3 [?] |
| C10 | 不得把灯效编号当电量（BatteryLightEffect） | AULA 灯效表 1–21 无电池项；AKKO 的 `0x88` 才是电池事件 | [1][10] |
| C11 | 写命令白名单：只允许 `0x05/0x0B*/0x4A/0x41/0x43/0x44/0x49` 等读 | 一旦发 `0x01–0x04/0x09` 会改配置；`0x0B` 在 RK 上是写 | A5 [3][6] |

## D. 实施候选（按"证据强 + 只读 + 收益高 + 风险低"排序）

1. **【最高】修正握手语义 + 型号码表**（纯解析/展示，零设备写）
   把 `0x05` 作为识别路径，输出 `model_code=0x6E`、`status=0x00`，并在 UI 上明确"已识别，未知型号"。同时建立 A1/B1 的型号码常量与测试向量。
   收益：把"无电池"变成"已知设备、未知型号"，为后续分派打基础。风险：0。

2. **【高】`0x4A` 读取器（按型号白名单）**
   对 F75/F87/L65 码（`CD/8F/010B/A9/01CA`）启用；`r[5]` 只接受 1..100，`r[6]` 只记录不解释。
   测试向量：`13 4A 01 00 02 64 10 …`。
   风险：只读；但注意"唤醒"语义（见 E3）。

3. **【高】被动监听电量快捷键**
   订阅 `0x13` 输入报告，提示用户按 Fn+B / Fn+Enter / Fn+Backspace（各厂商不同），保存 30 s 内所有 20B 帧。零写入、零风险，且能直接发现"主动推送型"电量包。

4. **【中】`0x0B` 读取（严格门控）**
   仅当 `0x05` 型号码命中 AULA 白名单时发送；`0x6E` 不在白名单 → 默认不发。若主代理决定对 `0x6E` 试验，必须先在 E4 里确认品牌，且接受"若为 RK 带屏机则会写屏幕参数"的副作用。
   解析：`r[1]==0x0A, r[4]=len, r[5]=fw, r[6]=电池, r[7]==0x01 无线在线`；`r[6]` 超出 1..100 一律丢弃。

5. **【中】跨厂商适配器（独立 lane，不混入 AULA 代码）**
   Ajazz `0x0E`、AKKO `0xF7`+`05 88`、ROYUAN `0xF7`、Lofree `0x1A`、Keychron `0xA4`。每个都要独立 VID/PID + 独立句柄 + 独立向量；证据最强者是 Ajazz（真机 `64 03` / `61 01`）。

6. **【明确不做】**
   - WinUSB 替换默认路径（本 lane 无任何证据支持）；
   - 发送 `0x01–0x04/0x09` 写命令做"探测"；
   - 把 `0x6E` 的 `00 20 10` 猜成版本/电量；
   - 对未知型号套用别的型号的缩放。

## E. 需要主代理串行执行的实机验证（全部只读，除 E4 明确标注）

**E0. 环境固定**：每次试验前重新插拔 dongle；同一时刻只允许一个进程持有 hidraw；记录 `lsusb -v`、`/sys/class/hidraw/*/device/report_descriptor`、uevent。

**E1. 身份复现（0 风险）**
发 `13 05 01 00 00…00 19`，保存原始 20B 与校验结果；确认 `r[10]=0x6E`、`r[11]=0x00`、`r[12..14]=00 20 10`。

**E2. 在线标志判别（0 风险，关键）**
分别在「刚按键后 1 秒内」与「静置 5 分钟后」各读一次 `0x05`，比较 `r[11]`：
- 若 `1 ↔ 0` 翻转 → `r[11]` 是"键盘在线"标志，则 `0x4A` 静默很可能只是**键盘没连上/在睡**，先解决唤醒（见 E3），不要急着判定"不支持"。
- 若恒为 `0` → 该字段不是在线标志，`0x4A` 静默更可能是"命令不存在"。

**E3. 唤醒后单次 `0x4A`（0 风险）**
按键唤醒后立刻发一次 `13 4A 01 00 00…`，监听 2 s；再静置后发一次。两次都空 → 判定该固件不支持 `0x4A`（不再做 15 连发）。

**E4. 被动捕获电量快捷键（0 风险，优先于任何新命令）**
只读监听 `0x13` 输入 30 s，期间依次按 Fn+B、Fn+Enter、Fn+Backspace、Fn+Right Ctrl；保存全部 20B 帧并标注时间。目标：找到形如 `13 xx 01 00 02 <1..100> <flag>` 的帧。

**E5. 有线对照（0 风险，需插线）**
插 USB 线 → `258A:010C`（或该机型的 wired VID/PID），feature report 6/9，发 `06 82 01 00 01 00 06 00` 读 PSD，再发 `06 87 …` 读 2 字节电量。注意：插线时电量常被钳到 100。
（这一步还能确认型号码是否与无线一致。）

**E6. `0x0B` 试验（有副作用，需你拍板）**
前置：E5 或机身铭牌确认是 AULA/BY Tech 家族，**且不是 RK 带屏机**。发包 `13 0B 01 00 … 1F`，等 `13 0A`。
- 若返回 `13 0A 01 00 04 xx yy 01 …` → 记录 `yy` 并与系统电量/指示灯比对。
- 若返回 `13 8B`（错误位）或无响应 → 记录后放弃。
- 若设备是 RK：**不要发**（`0x0B` 会写音量/时间）。

**E7. 最终判定表**
| 结果 | 结论 |
|---|---|
| E6 有 `13 0A` 且电量合理 | 该型号走 `0x0B` 路径，加入白名单 |
| E3 有 `13 4A` | 走 `0x4A` 路径 |
| E4 捕获到推送帧 | 走被动监听路径（最佳，零查询） |
| 全部为空且 E2 显示离线 | 问题在配对/唤醒，不是协议 |
| 全部为空且 E2 恒 0 | 必须拿到**具体型号**或官方驱动抓包，否则无法继续 |

**必须外部信息才能继续的情况（明确）**：`0x6E` 在 AULA/RK/LANGTU/Womier 的任何公开表中都不存在。要继续，只能 (a) 用户告知具体品牌型号，或 (b) 提供厂商 Windows 驱动的 USBPcap / WebHID 抓包。**没有这两者之一，任何"0x6E 应该读哪个字节"的说法都是编造。**

## F. 证据表

| # | 来源 | URL | 关键主张 | 类型 | 置信度 |
|---|------|-----|---------|------|--------|
| 1 | deepan-alve/womier-l65-linux（commit 28d0b25, MIT） | https://github.com/deepan-alve/womier-l65-linux/blob/28d0b25c9d562ddf5f5b5466d45dc3ad9e9b7808/linux/l65ctl.py | 3554:FA09 帧格式；`0x4A` 电池 `r[5]/r[6]`；`0x4A` 静默=休眠；多命令表 | 一手源码 + 自述真机 | 高 |
| 2 | 同上 docs/L65-protocol.md | https://github.com/deepan-alve/womier-l65-linux/blob/28d0b25c9d562ddf5f5b5466d45dc3ad9e9b7808/docs/L65-protocol.md | 有线 `0x82` 返回 PSD `…01 CA`；`0x87` 为无线机型电池 | 一手文档 | 高 |
| 3 | dnviti/aula-x99-pro-driver-linux（commit 7ec12ad，无许可证） | https://github.com/dnviti/aula-x99-pro-driver-linux/blob/7ec12ad1f2bb642a46ae91b0c3bce327de96f4ce/PROTOCOL.md | `0x05` 身份 / `0x0B` 固件+电池（应答 `0x0A`）/ 多页格式 / PSD 表 | 一手文档 + 自述 USB 抓包 | 中 |
| 4 | 同上 aula_f99pro.py | https://github.com/dnviti/aula-x99-pro-driver-linux/blob/7ec12ad1f2bb642a46ae91b0c3bce327de96f4ce/aula_f99pro.py | `CMD_GET_PSD=0x05`、`CMD_GET_VERSION=0x0B`；`d[6]` 被同时当 fw_lo 与电量（内部矛盾） | 一手源码 | 中（字段命名可疑） |
| 5 | BakulBd/OpenKey（commit 0b59e6e, MIT） | https://github.com/BakulBd/OpenKey/blob/0b59e6e103bfcdbf8a7c800129f9a0b638ea5a04/docs/AULA_F99.md | 型号码表；`0x0B` 仅单源；写/重置 opcode 拒绝；RK 共用 3554:FA09 | 一手文档 | 高（作为"证据分级"的元证据） |
| 6 | 同上 protocol.ts | https://github.com/BakulBd/OpenKey/blob/0b59e6e103bfcdbf8a7c800129f9a0b638ea5a04/packages/protocols/src/families/aula-receiver/protocol.ts | `version()` 解析：`data[0]=fw, data[1]=电池, data[2]==1 在线`；请求构造与校验 | 一手源码 | 高 |
| 7 | 同上 models.ts | https://github.com/BakulBd/OpenKey/blob/0b59e6e103bfcdbf8a7c800129f9a0b638ea5a04/packages/protocols/src/families/by-tech/models.ts | 8 个 AULA 型号码 + RK R75/M87 型号码 | 一手源码 | 高 |
| 8 | 同上 rk-m87.ts | https://github.com/BakulBd/OpenKey/blob/0b59e6e103bfcdbf8a7c800129f9a0b638ea5a04/packages/protocols/src/families/aula-receiver/rk-m87.ts | RK 接收器 `0x07` 状态、`0x0B` = SetScreenParam（写）；`01–04/09/12` 写、`06` 复位 | 一手源码（转述 ElXreno，MIT） | 高 |
| 9 | pradyumnac/aula-f99-controller（commit 21f3411, MIT） | https://github.com/pradyumnac/aula-f99-controller/blob/21f341183d4ca2d5652f77f83c8cb1aac2721d73/docs/reference/f99/protocol.md | 3554:FA09 = `ff02:0002`；`0x05` 型号在应答 byte10；**"电量无已知读取命令"** | 一手文档 + 真机 | 高 |
| 10 | leisiji/AulaSofwareLinux（commit 2dd3480, MIT） | https://github.com/leisiji/AulaSofwareLinux/blob/2dd34805682e3dba45ecd7fc322961a18aeddf9b/assets/index.json | 厂商配置中的 8 个 PSD 型号码 | 一手厂商配置 | 高 |
| 11 | OpenRGB GitLab issue #5166 | https://gitlab.com/CalcProgrammer1/OpenRGB/-/issues/5166 | `13 05 01 00…19` 请求与应答；byte10 = 型号 ID；20B 帧/pcap | 一手抓包 | 高 |
| 12 | aula-f87-control 0.5.3（crate；许可证未核对） | https://docs.rs/aula-f87-control/latest/src/aula_f87/device.rs.html | `0x4A` 解析；`state 0x10 = 插线`（与 L65 冲突）；2.4G 帧构造 | 一手源码 | 中高 |
| 13 | Rainbow132/2.4G-wireless-device-battery-level-acquisition（许可证未核对） | https://github.com/Rainbow132/2.4G-wireless-device-battery-level-acquisition/blob/main/keyboard.py | `13 05 01` → 期待 `13 0A`，电量取 `r[6]`；自述 "gemini 辅助生成" | 社区指南（AI 辅助） | **低**（校验和只覆盖 3 字节，且与 A2 冲突） |
| 14 | shrey715/ak820ctl PROTOCOL.md | https://github.com/shrey715/ak820ctl/blob/main/PROTOCOL.md | Ajazz `1a2c:8fff`；`0x0E` → `resp[5]` %，`resp[6]` 状态；真机 `64 03` / `61 01` | 一手文档 + 真机 | 高 |
| 15 | echtzeit-solutions/monsgeek-akko-linux PROTOCOL.md | https://github.com/echtzeit-solutions/monsgeek-akko-linux/blob/master/docs/PROTOCOL.md | `0xF7` 9B dongle 状态（byte1 电量、byte3 充电）；EP2 `05 88` 电池事件 | 一手文档 | 高 |
| 16 | HeyOkay/HaloBattery docs/protocols.md | https://github.com/HeyOkay/HaloBattery/blob/main/docs/protocols.md | Lofree Hyzen `0x1A` byte7；Keychron 3434；Razer 等 | 社区汇总（逐条标注来源） | 中高 |
| 17 | dnminen/sharkfin PROTOCOL.md | https://github.com/dniminenn/sharkfin/blob/master/docs/PROTOCOL.md | ROYUAN `0xF7`：byte1 电量、byte3 在线；[HW] | 一手文档 + 真机 | 高 |
| 18 | Qyzom/MchooseG75-OpenRGB | https://github.com/Qyzom/MchooseG75-OpenRGB | MCHOSE G75 `41E4:2001`，20B `0x13`，27 分片 | 一手源码 | 中 |
| 19 | DX-Cloud/MchoseG98V2AutoLight | https://github.com/DX-Cloud/MchoseG98V2AutoLight | G98 V2 `41E4:2203`；无线 "Report ID 19 分包应答" 未实现 | 一手文档 | 中 |
| 20 | Lightning-13/rk-m75 | https://github.com/Lightning-13/rk-m75 | RK M75 无线 258A:0148，`13 88 XX` 20B 帧；无电量 | 一手文档 + 真机 | 高 |
| 21 | OutersGIT/k10he-battery-firmware + Keychron PR #504 | https://github.com/OutersGIT/k10he-battery-firmware ；https://github.com/Keychron/qmk_firmware/pull/504 | QMK `0xA4`；2.4G 下键盘主动推送 | 一手文档 | 高 |
| 22 | David-Lzy/signalrgb-langtu-t99 | https://github.com/David-Lzy/signalrgb-langtu-t99 | LANGTU T99 用 3554:FA09；model ID 173/0xAD | 一手文档 | 中 |
| 23 | xb-bx/m800-pro-driver | https://github.com/xb-bx/m800-pro-driver | Delux 仅鼠标有开源驱动（键盘无） | 一手源码 | 中 |
| 24 | linux-hardware.org 3554:fa09 | https://linux-hardware.org/?id=usb%3A3554-fa09 | 该 PID 枚举为 "Compx 2.4G Wireless Receiver" | 数据库 | 中 |

## Coverage Status

**直接核对（读了一手源码/文档）**：womier-l65-linux（4 个文件）、aula-x99-pro-driver-linux（PROTOCOL.md / aula_f99pro.py / KB.ini / index.json）、OpenKey（AULA_F99.md / protocol.ts / models.ts / rk-m87.ts / MODEL_GUIDE 片段）、pradyumnac spec、leisiji index.json + f75 device.ini、OpenRGB #5166、aula-f87-control device.rs、Rainbow132 keyboard.py、ak820ctl PROTOCOL.md、MonsGeek PROTOCOL.md、HaloBattery protocols.md + issue #165、sharkfin PROTOCOL.md、MCHOSE G75/G98 README、RK M75 README。

**未能完成**：
- 无法读取 `/root/Anybattery` 工作树（本会话无 bash/文件系统工具），因此 C 节是"行为对照清单"而非逐行 diff；`controllers.rs`、`vendor.rs` 是否存在、如何分派均未核对。
- 未接触真实硬件，所有 `[HW]` 均转述原作者。
- Delux 键盘：搜索 4 轮（品牌名 + HID/协议/linux/258a/3554）均只得到鼠标驱动与旧 T 系列驱动页，**无任何键盘协议证据**，故不列入支持矩阵。
- Dareu、Machenike、Epomaker、Yunzii、VGN、Keychron 非 QMK 机型的**键盘侧 2.4G 电量协议**均未找到可引用的一手实现；仅确认部分机型用"指示灯/Fn 快捷键"显示电量，这不构成 HID 协议证据。
- VGN/ATK 多为 Nordic 鼠标方案（HaloBattery 有），与键盘 lane 不同，未深入。

**未解决的关键问题**：`0x6E` 属于哪个品牌/型号；`0x05` 应答 `byte11` 是否为在线标志；`0x4A` 的 `state=0x10` 到底表示满电还是插线；`0x0B` 的 `data[1]` 是否真是电量（仅单源）。