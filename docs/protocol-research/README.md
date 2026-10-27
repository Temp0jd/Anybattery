# AnyBattery 协议扩展研究与集成台账

日期：2026-10-27

## 目标

基于公开开源项目系统扩展蓝牙、2.4G 鼠标、键盘、耳机与手柄的电量适配。研究必须追溯到一手源码/官方文档，不能把品牌宣传、搜索摘要或“同 VID/PID”当成协议证据。

## 五条研究线

| Lane | 模型 | 范围 | 预期报告 |
|---|---|---|---|
| 1 | `command-code/deepseek/deepseek-v4.1-flash` | 2.4G 键盘，优先当前 `3554:FA09` 与 Delux/AULA/Womier/MCHOSE 等 | `01-keyboards.md` |
| 2 | `command-code/deepseek/deepseek-v4.1-flash` | Razer 与 Logitech 型号表、事务与 HID++ | `02-razer-logitech.md` |
| 3 | `command-code/deepseek/deepseek-v4.1-flash` | 其他游戏外设、耳机与手柄 | `03-other-vendors-controllers.md` |
| 4 | `antigravity/gemini-3.8-flash` | 国产/OEM 鼠标与 Beken/CompX/PixArt/Telink/Nordic 方案 | `04-oem-mouse.md` |
| 5 | `antigravity/gemini-3.8-flash` | 通用项目横向审计、蓝牙 PnP/GATT 与标准 HID | `05-universal-bluetooth.md` |

研究阶段不并行修改产品代码，也不并发访问真实设备。主代理负责证据复核、实施分派、集成和串行真机验收。

## 当前代码基线

- `vendor.rs`：Razer、0x13 键盘实验路径、CompX、VAXEE、Endgame、Nordic、Logitech HID++。
- `extra.rs`：Steam/PlayStation/Nintendo/8BitDo、Razer PA、ASUS、Keychron、Lofree、NZXT、SteelSeries、HyperX、Corsair、`1D57:FA60` 被动状态。
- `hid.rs`：标准 HID Battery Strength / State of Charge、Feature/Output/Input 与 bounded overlapped I/O。
- `bluetooth.rs`：BTHENUM/BTHLE PnP 电量属性；尚无主动 BLE GATT Battery Service。
- `xbox.rs`：`Windows.Gaming.Input.RawGameController` 电池报告。

## 已确认的实机证据

- OPPO Enco X3：`BTHENUM\DEV_407218C8A7BC` 与 Hands-Free 服务节点按 ContainerId 合并为单卡；AnyBattery 实扫读到 90% / 100%（`.pi/diag-final-integration.txt`、`.pi/diag-hfp-merge.txt`），充电未知。
- Zero 1：`BTHLE\DEV_CC3D601F1B0C` 的 GATT 服务节点归并到根卡；AnyBattery 实扫读到 46%（`.pi/diag-final-integration.txt`），仍是 Windows PnP 属性，不是主动 GATT。
- DELUX M618Air：`1D57:FA60` / `MI_02` / `000A:0000` / input 5，被动报文 `03 24 40 01 <raw>`。官方 `0x4290b0 → 0x421b90` 映射 `1/2/3/4-5/6-7/8..100 → 10/20/40/60/80/100`；用户官方软件对 raw6 显示 80%，`.pi/diag-final-integration.txt` 实扫 raw7 显示 80%、`charging = Some(false)`，最终集成扫描 2.99 秒、30 秒硬超时内完成（`devices=4 bluetooth=2 hid=9`），当前 123 项 Windows 测试通过。state `02/03/80` 仍拒绝，名称保持中性。
- `3554:FA09`：`FF02:0002`、20 字节 Output/Input。最终集成扫描的 5 个 collection 仅生成一张 unknown 卡、无重复；`0x05` 只能拿到型号应答；L65 `0x4A` 原版命令多次无响应。键盘仍不能显示电量，保留实验状态。
- Razer 朋友设备型号未知；同句柄、transaction ID、接口和 Synapse 占用均待具体证据，不按“Razer 全系”承诺。

## 每条证据必须包含

1. 品牌/型号/连接、VID/PID、MI/collection、usage、report 类型和长度。
2. 完整查询或被动报文、report/transaction ID、checksum、多帧和会话要求。
3. 电量、充电、离线、stage/raw 的 offset 与缩放；未知位保持未知。
4. 同句柄、共享模式、超时、唤醒和厂商软件占用要求。
5. 仓库、commit、永久链接、文件/行号、许可证和证据强度。
6. 当前代码的已有/疑似错误/缺失分类，以及严格 parser 测试向量。

## 集成验收门

- 只实现有一手协议证据的只读路径；不安装驱动、不 fuzz、不写配置/固件。
- 同 VID/PID 不直接等同同型号或同缩放；优先按接口、report descriptor、device_id 或型号表收窄。
- 错误头、长度、checksum、离线、ACK 和越界值必须拒绝；不知道充电状态就保持 `None`。
- 扫描必须有界返回；新协议不得让同步 HID 读取永久阻塞。但 `HidD_GetFeature`/`HidD_SetFeature` 是同步且不可取消的系统调用，个别固件/驱动仍可能阻塞；2.99 秒只代表当前设备组合，不是所有设备的硬上界。
- 单接收器多 collection 不产生重复未知卡片，多设备 receiver 不破坏 slot 拆分。
- parser 单测、Windows 目标编译/测试、真实设备扫描分别验收；编译通过不等于真机支持。
- 真机未验证的项目标为“社区协议”或保留研究状态，不写成普遍兼容。
- MCHOSE caps65/A5 分组修复已确认，但完整 handle 级 `SetFailed`/`GetFailed` 路由仍缺 fake-HID 端到端测试，需真机；`1532:0528` 充电中电量仅粗略参考。

## 实施顺序

1. 当前键盘与朋友 Razer 的证据缺口及最安全修复。
2. 有精确 ID/报文且可串行验证的 OEM 与主流厂商缺口。
3. 其他外设和手柄的高置信度缺口。
4. BLE GATT 主动读取单独评估，避免影响音频、连接稳定性和扫描时长。

## README 支持矩阵维护契约

`README.md` 的“设备支持矩阵”是唯一面向用户的完整支持矩阵；`docs/protocol-research/` 只保存协议来源、fixtures、实测记录和未决项，不再复制一份容易漂移的完整表。

每次代码变更只要涉及以下任一项，必须在同一提交/同一轮修改中同步更新 README 矩阵的状态、限制、证据和“最后更新”日期：

1. 新增或修改 VID、PID、PID 白名单、usage page、interface、report ID、报文长度或传输门槛。
2. 新增或修改 parser、电量缩放、充电字节/状态、离线判定、重试/会话/同句柄行为。
3. 新增或改变 Verification/Confidence 等级，或把上游“Verified”改成本机证据。
4. 新增、修复或排除某个型号组，或发现同 VID/PID 的不同协议/型号。
5. release 前必须用 bounded grep/脚本核对当前工作树中的 PID、分支和 README 行，并复核本机实扫/官方校准证据；只编译通过或只有静态 fixtures 不得写成 ✅。
6. 任何新增/删除矩阵行、状态/证据等级变更或限制文字变更后，都必须同步矩阵末尾的按行统计；统计不是支持承诺。

维护规则：

- ✅ 只给本机真实扫描或官方软件校准；🟡 必须有代码路由和严格 fixtures，标“脚本传输测试”时只表示生产 session 有脚本驱动测试、没有对应真机；🧪 必须写明会话、传输、接口、硬件门或 parser/证据缺口；⚪ 必须说明依赖系统/设备实际暴露；❌ 只列已知重要排除，不承诺穷举。
- 能枚举不等于能读电量；未匹配默认隐藏；充电未知与未充电必须分开写。
- OEM 7 项代码审查与修复已确认，但不得把 OEM 行写成“已独立审查”或提升置信度；PixArt / MCHOSE / 0528 / VAXEE 等 fixtures-only 项保持社区等级，Delux M800 Pro 继续保留 MI_01 硬件门。

### 当前不变量（2026-10-27）

| 项目 | 期望值 |
|---|---|
| 矩阵数据行 | 110（102 个六列表格行 + 8 个四列排除表行） |
| 状态统计 | ✅ 3、🟡 87、🧪 4、⚪ 5、❌ 11 |
| 代码显式产品 PID | 191（含仅排除的 `1532:0557`；不含 usage/collection 常量） |
| Razer PID | 81 = `razer_name` 76 个 90B PID + 3 个 PA PID + `0528` + `0557` |
| Razer 型号口径 | `razer_name` 45 个唯一型号名 = 43 个 90B + 2 个 PA；含 PID 的 Razer 行 47（其中 `0557` 仅排除），另有 1 个无 PID 兜底行；不要与 81 PID 混用 |
| Logitech 接收器 PID | 23 |
| 当前 Windows 测试 | 123/123（脚本传输与 parser 测试也不等于真机） |

### 发布前检查

- 运行 `git diff --check`；未跟踪文档另跑 `git diff --no-index --check /dev/null docs/protocol-research/README.md`。两者都不得有空白错误。
- 用 bounded Python/脚本统计矩阵：数据行必须为 110，状态统计必须与上表一致；Markdown 每张表列数一致、链接非空。
- 用 bounded grep/脚本对照 `vendor.rs` / `extra.rs`：191 个显式产品 PID 必须全部出现在 README，Razer 81 个 PID、Logitech 23 个接收器 PID 不得缺项；输出缺失列表必须为空。
- 重跑 Windows 测试与对应真机诊断；当前测试数为 123/123，但脚本传输测试、编译通过和 fixture 通过都不得写成真机验证。发布前复核 M618Air 80%、OPPO 90%、Zero 46% 及 `.pi/diag-final-integration.txt` 证据路径。

<details>
<summary>有界矩阵审计脚本（只读；复制到仓库根目录运行）</summary>

```python
#!/usr/bin/env python3
"""Bounded README matrix audit. Run from the repository root."""
import re
from collections import Counter
from pathlib import Path

readme = Path("README.md").read_text()
vendor = Path("crates/anybattery-windows/src/vendor.rs").read_text()
extra = Path("crates/anybattery-windows/src/extra.rs").read_text()

def add(out, vid, pids):
    out.update(f"{vid.upper()}:{pid.upper()}" for pid in pids)

expected = set()
razer = vendor.split("fn razer_name")[1].split("fn build_razer_request")[0]
add(expected, "1532", re.findall(r'0x([0-9A-Fa-f]{4})\s*=>\s*\("', razer) + ["0528", "0557"])
comp = vendor.split("fn compX_name")[1].split("fn compX_17_frame")[0]
for vid, pid in re.findall(r'\(0x([0-9A-Fa-f]{4}),\s*0x([0-9A-Fa-f]{4})\)', comp):
    add(expected, vid, [pid])
pix = vendor.split("fn pixart_name")[1].split("fn pixart_product_ok")[0]
for vid, pid in re.findall(r'\(0x([0-9A-Fa-f]{4}),\s*0x([0-9A-Fa-f]{4})\)', pix):
    add(expected, vid, [pid])
mch = vendor.split("fn mchose_name")[1].split("fn mchose_candidate")[0]
for vid, pid in re.findall(r'\(0x([0-9A-Fa-f]{4}),\s*0x([0-9A-Fa-f]{4})\)', mch):
    add(expected, vid, [pid])
add(expected, "248A", ["5B2F", "5B2E"])
logi_block = vendor.split("LOGITECH_RECEIVERS: [u16; 23] = [")[1].split("];")[0]
add(expected, "046D", re.findall(r"0x([0-9A-Fa-f]{4})", logi_block))
steam = extra.split("fn read_steam")[1].split("fn parse_dualsense")[0]
add(expected, "28DE", re.findall(r"0x(13[0-9A-Fa-f]{2})\s*=>", steam))
play = extra.split("fn read_playstation")[1].split("fn read_nintendo")[0]
add(expected, "054C", re.findall(r"0x([0-9A-Fa-f]{4})", play))
nin = extra.split("fn read_nintendo")[1].split("fn eightbitdo_name")[0]
add(expected, "057E", re.findall(r"0x([0-9A-Fa-f]{4})", nin))
eight = extra.split("fn eightbitdo_name")[1].split("fn eightbitdo_confidence")[0]
for left, right in re.findall(r"0x([0-9A-Fa-f]{4})(?:\s*\|\s*0x([0-9A-Fa-f]{4}))?\s*=>", eight):
    add(expected, "2DC8", [pid for pid in (left, right) if pid])
for name in ("STEELSERIES_NOVA7_DISCRETE", "STEELSERIES_NOVA7_PERCENT", "STEELSERIES_NOVA5"):
    add(expected, "1038", re.findall(r"0x([0-9A-Fa-f]{4})", extra.split(name)[1].split("];")[0]))
hyper = extra.split("fn read_hyperx")[1].split("const CORSAIR_LEVEL_MAX")[0]
add(expected, "03F0", re.findall(r"0x([0-9A-Fa-f]{4})", hyper.split("let mut request")[0]))
add(expected, "1B1C", ["1B7F", "0A40", "0A3E", "0A64"])
asus = extra.split("let steps = matches!")[1].split(");")[0]
add(expected, "0B05", re.findall(r"0x([0-9A-Fa-f]{4})", asus))
key = extra.split("fn read_keychron")[1].split("fn read_lofree")[0]
add(expected, "3434", re.findall(r"0x([0-9A-Fa-f]{4})", key.split("let mut request")[0]))
add(expected, "1E71", ["2101", "2129"])
add(expected, "1D57", ["FA60"])
add(expected, "3554", ["FA09"])
add(expected, "388D", ["0025"])
assert len(expected) == 191, f"expected 191 explicit PIDs, got {len(expected)}"
missing = sorted(pid for pid in expected if pid not in readme.upper())
assert not missing, f"README is missing code PIDs: {missing}"

raz_doc = readme[readme.index("#### 90 字节"):readme.index("### Logitech")]
raz_doc = {f"1532:{pid.upper()}" for pid in re.findall(r"1532:([0-9A-Fa-f]{4})", raz_doc)}
assert len(raz_doc) == 81 and raz_doc == {p for p in expected if p.startswith("1532:")}
logi_doc = {p for p in expected if p.startswith("046D:")}
assert len(logi_doc) == 23 and all(pid in readme.upper() for pid in logi_doc)

start, end = readme.index("## 设备支持矩阵"), readme.index("按行统计")
counts, rows = Counter(), 0
for line in readme[start:end].splitlines():
    if not line.startswith("|"):
        continue
    cells = [cell.strip() for cell in line.strip("|").split("|")]
    if len(cells) not in (4, 6):
        continue
    status = cells[4] if len(cells) == 6 else cells[3]
    for key in ("✅ 本机验证", "🟡 社区协议", "🧪 实验/运行时修复", "⚪ 系统/标准", "❌"):
        if status.startswith(key):
            counts[key] += 1
            rows += 1
            break
assert rows == 110, f"expected 110 matrix data rows, got {rows}"
want = {"✅ 本机验证": 3, "🟡 社区协议": 87, "🧪 实验/运行时修复": 4, "⚪ 系统/标准": 5, "❌": 11}
assert dict(counts) == want, f"status counts differ: {dict(counts)} != {want}"
print(f"matrix OK: rows={rows} statuses={dict(counts)} PIDs={len(expected)} Razer=81 Logitech=23")
```
</details>
