# 2.4G 键盘 / 鼠标协议适配记录

日期：2026-10-27

## 1. 现场设备证据

通过临时 Windows 扫描器复用 AnyBattery 现有扫描路径得到：

| 设备 | VID:PID | 关键 HID collection | 当前问题 |
|---|---|---|---|
| 键盘接收器 | `3554:FA09` | `FF02:0002`，input/output 20，feature 0 | 被放进 CompX 鼠标协议路径，无有效电量 |
| 鼠标接收器 | `1D57:FA60` | `000B:0000`，input/output 0，feature 64 | 没有厂商适配，且 transport 原先显示为 USB |

两个接收器是不同物理设备，不应合并。具体商品型号不能仅凭 OEM 共用 VID/PID 武断确定。

## 2. 公开参考与许可证边界

- AULA Manager，commit `b98c0afad7ae57bbd4f073230c1ee1cb87006318`，MIT：
  `docs/PROTOCOL.md`，https://github.com/free-soldier28/Aula-Manager/blob/b98c0afad7ae57bbd4f073230c1ee1cb87006318/docs/PROTOCOL.md
- 2.4G wireless device battery level acquisition，commit `a3bdd2583a57cdea65ebc333e71c29b41feb10d0`，未发现明确许可证；只记录协议事实，不复制代码：
  `keyboard.py`，https://github.com/Rainbow132/2.4G-wireless-device-battery-level-acquisition/blob/a3bdd2583a57cdea65ebc333e71c29b41feb10d0/keyboard.py
- Attack Shark X11 community driver，commit `49ecf8e6d3bd83bf71e231209d28c5e0e8b71d12`，MIT：
  `docs/battery-status.md`，https://github.com/akawazak/attack-shark-x11-community-driver/blob/49ecf8e6d3bd83bf71e231209d28c5e0e8b71d12/docs/battery-status.md
- Attack Shark Linux，commit `ed2f8a599c835fd35b095e5b78fbfe3abb768350`，MIT：
  `docs/protocol-x6.md`，https://github.com/blak0p/attack-shark-linux/blob/ed2f8a599c835fd35b095e5b78fbfe3abb768350/docs/protocol-x6.md
- FerrisPulse，commit `4917a10e238ed3dba2938969752886353c8ff5e9`，GPL-3.0；仅作协议事实交叉印证，不复制代码。
- Delux M800 Pro 驱动使用其他 VID/PID，不能套到当前 `1D57:FA60` 鼠标。

## 3. 键盘：AULA 0x13 状态协议

- 精确匹配 `3554:FA09` + `FF02`，input/output 至少 20。
- 请求是 20 字节 Output Report：`13 05 01 00 ... 00 19`，不是 Feature Report，也不是现有 CompX 17 字节鼠标请求。
- 接收器可能先 echo 请求或返回其他分片，因此要在同一句柄上写后连续读，不能只读第一包。
- 电量状态包以 `13 0A` 开头；level 在 offset 6，连接状态在 offset 7，1 表示在线。
- 20 字节帧的 checksum 为 `sum(bytes[0..18]) & 0xFF`，存 offset 19。
- 实现必须校验长度、头、checksum、连接状态和 0–100 范围；不知道充电状态就保持 `None`。

## 4. 鼠标：1D57 被动状态报告

只监听，不发送配置命令，不安装 WinUSB：

- X11：`03 55 40 01 <level>`，level 直接为 0–100。
- X6 heartbeat：`03 10 40 <stage> <raw>`，raw 为 0–10，百分比为 raw×10。
- `03 10 50 ...` 是配置 ACK，不得当成电量。
- 两种报文至少 5 字节；其他头部和越界值一律拒绝。
- 现场 collection 的 `InputReportByteLength=0`，Windows HID ReadFile 能否收到被动 interrupt 报告必须实机验证。若不能，不引入 WinUSB、control transfer 或未文档化 GET_FEATURE。
- Microsoft《Obtaining HID Reports》说明连续输入报告应使用 ReadFile；`HidD_GetInputReport` 只适合获取当前状态，连续调用可能丢报告，部分设备还会无响应：https://learn.microsoft.com/en-us/windows-hardware/drivers/hid/obtaining-hid-reports
- 无数据监听必须有真实超时保证。当前传输层使用 OVERLAPPED 读取，因此打开句柄时也必须有 `FILE_FLAG_OVERLAPPED`；不能只缩短表面等待时间而允许同步 ReadFile 永久阻塞。

## 5. 决策记录

- 分类模型：`typesafe/jev-latest`；question version `2.4g-source-rank-v1`；阈值 0.65。
- 键盘首选 `aula_protocol`，confidence 0.88，采用。
- 鼠标首选 `attack_x6_hidraw`，confidence 0.44，低于阈值；未直接采用单一判断，回退为同时核对 X11、X6 与 FerrisPulse。

## 6. 雷蛇后续诊断假设（不属于本补丁）

当前 `razer_query` 在 SetFeature 后关闭句柄，再用新句柄 GetFeature；对照实现通常在同一句柄上完成 SetFeature/GetFeature。该差异可能影响部分雷蛇设备，但型号、接口、transaction ID、Synapse 占用和协议不匹配也都可能造成失败。需要具体型号和诊断日志后才能确定，不在本补丁中猜改。

## 7. 验收门

- parser 单测覆盖有效报文、错误头、坏 checksum、离线、ACK 和越界值。
- Windows 目标编译与测试通过。
- 使用现场两个接收器实跑，只有严格 parser 接受时才显示百分比。
- 鼠标若被动 HID 无数据，明确报告传输限制，不宣称已支持，不安装驱动。

## 8. 参考项目实跑与二次审查（2026-10-27）

### 键盘

- 已在隔离的 Windows Python + hidapi 环境中运行参考项目原封不动的 `battery-guide/keyboard.py`，没有复用 AnyBattery 的 Rust 读取代码。
- 原始脚本同样只收到 `13 05 01 00 0A 03 00 00 00 00 6E 00 00 20 10 00 00 00 00 C4`，随后报告“未能在限定时间内捕获到以 13 0A 开头的电量数据包”。证据：`.pi/reference-aula-output.txt`。
- 因此当前失败不是 AnyBattery 独有的读取时序问题。该 VID/PID 也被不同品牌/方案复用，不能只凭 ID 把设备命名为 AULA；需要具体键盘型号、RF 唤醒/会话证据。

### 鼠标与其他项目的方法

- MouseBatteryTray（commit `c1c6059472dab366c700a7f7516e0dbffde68fd7`）明确把 `interface 2 + usage_page 10 (0x000A)` 作为 Attack Shark X11/X6/X3/R1 的优先状态接口。当前实机的 `03 24 40 01 06` 正好出现在 0x000A collection，因此只监听 0x000B 不完整。
- 该项目还按报文 `device_id` 动态识别固件，并且只允许 X6 (`0x85`) 使用 1–10 ×10 缩放，修复过普通鼠标低电量被错误放大到 100% 的问题。
- R1 驱动（commit `2f46655d4e53c417ecd35089ad8bdea1d7798699`）确实使用第 5 字节 ×10，但 MouseBatteryTray 的通用规则并不对 `0x24` 乘 10，且 `0x24` 不在其已命名设备表中。公开实现对不同固件存在差异，不能盲套。
- 当前 `03 24 40 01 06` 的 `06` 是 6% 还是 60% 仍未确定。需要商品型号和一次官方软件或蓝牙电量读数进行校准，不能把参考代码中的不同规则随机选一个。

### 当前实现状态

- 严格 parser 单测通过，overlapped 读写超时已修复，扫描可以在 bounded time 返回。
- 鼠标已完成参考应用与本应用的双重实机验证；键盘仍未产生可展示电量，不更新键盘支持声明。

## 9. 最终实机结果与后续阻塞

### 鼠标：已验证（社区协议）

- MouseBatteryTray 原应用在本机返回 `BatteryHistory=[[1793069842.4037724, 6]]`，证明其接口选择与解析路径可直接借鉴。
- AnyBattery 现在严格匹配 `1D57:FA60` + interface 2 + `000A:0000` + input 5，实机输出 `1D57 2.4G 无线鼠标 | Usb24g | Available | Percent { level: 6 }`。
- `key_suffix=None` 使同 container 的 unknown 与成功候选合并；show_unknown=true 时不再重复显示同一接收器。
- 主代理独立复跑：14/14 Windows 协议测试通过，真实扫描有界完成，6% 与去重结果均复现。
- 该 6% 与参考应用一致，属于社区协议结果；如官方软件/蓝牙显示不同，应按具体固件继续校准，不能据此推断所有 1D57 设备都使用同一缩放。

### 键盘：仍未解决

- Aula-Manager 当前源码构建的原版 CLI 能读取 9/10 个无线配置分片，但之后的原始电量脚本仍只有 `13 05` 回执，没有 `13 0A`。
- 配置中的 `BatteryLightEffect` 是低电量灯效，不是剩余百分比，不能用来代替电量。
- Womier L65（commit `28d0b25c9d562ddf5f5b5466d45dc3ad9e9b7808`，MIT）提供另一套已验证协议：`13 4A ...` 请求，匹配 command `0x4A`，byte 5 为百分比，byte 6 为未解码状态标志。参考：`linux/l65ctl.py` 与 `windows/l65w.ps1`。
- 已直接运行原版 `l65w.ps1 battery` 并连续进行 15 次唤醒窗口尝试，全部返回 `no response (press a key to wake)`。因此当前键盘不是简单缺少 0x4A 调用，可能是不同固件/型号或当前 RF 状态不支持该命令。
- 下一步需要具体键盘品牌型号，或官方驱动运行时的只读 HID 交换记录；在此之前不展示键盘电量，不把 0x05 或 0x4A 的实验路径写成已支持。
