# OEM 鼠标三协议：源码复核与可实施方案（Wave D-1）

日期：2026-10-27
状态：实施指导稿。本文只新增本文件，不改产品代码、README、release；不访问真实设备。
基线：`70906c7` + 当前未提交工作树。凡本文与 `04-oem-mouse.md` 冲突处，以本文列出的源码为准；报告中的错误在 §5 集中列出。

## 0. 结论先行

本批只做三条独立路由，先 parser、后传输，全部先以 `Confidence::Community` 接入：

1. **PixArt/JM 64B Feature**：`usage_page=0xFFFF`、`usage=0x0000`、`feature_len=65`、report ID `0x00`。请求 `00 00 00 02 02 00 83` + 58 个 `00`；回复 `00 A1|A2 00 02 02 00 83 <chg> <level>`。充电位原始偏移 `raw[7]`，电量 `raw[8]`。同一句柄；一次 SET、最多 15 次 GET（50 ms）；A0/A3 帧跳过（A3 继续 GET，不重发）。
2. **MCHOSE 反码 Feature**：仅 `VID 0x5253/0x3837` 的已知鼠标 PID，`usage_page=0xFF01`、`usage=0x0001`。短报文 21 字节（report ID `0x11`），长报文 65 字节（report ID `0x12`）；发送 `[report_id, 0xF9, 0xFF...]`，回复 `[report_id, 0xF9, inverted payload...]`。解码后 `payload[9]` 电量、`payload[10]` 充电（0/1）。**每次 GET 前必须重发请求**；同一句柄。
3. **Delux M800 Pro 33B Feature**：仅 `VID 0x248A`、`PID 0x5B2F/0x5B2E`、Interface 1、`feature_len=33`、report ID `0x0C`。请求 `0C 01 20 00 <seq> 00...`；回复 `raw[18]` 电量、`raw[19]==1` 充电。源码用 libusb 直接控制传输，Windows 是否把 MI_01 枚举成 HID 尚未证实；**没有 HID 路径时不得发请求，也不得用 WinUSB 之外的猜测路径**。

**置信度纪律**：源码里的“实机确认”不能直接变成 AnyBattery 的 `Verified`。没有本机真机复现前，这三条路由一律 `Confidence::Community`；只有 §2.7/§3.6/§4.6 的验证通过后，才按型号逐个升级。报告里“Verified”的几处属于上游证据，不是本仓库的验收结果。

---

## 1. 共通传输层与现有代码边界

### 1.1 现有 Windows HID 层的关键事实

- `crates/anybattery-windows/src/hid.rs` 的 `feature_set()` 与 `feature_get()` 都是 `with_handle()` 包一次打开/关闭；`vendor.rs::feature_roundtrip()` 也是 SET 一次、关句柄、再 GET 一次。见 `hid.rs:279-305`、`vendor.rs:453-459`。
- 这三条协议都需要 **SET 与后续 GET 在同一句柄**，MCHOSE 还需要“每次 GET 前重发 SET”。因此**不要复用** `hid::feature_set`/`hid::feature_get`/`vendor::feature_roundtrip`。
- 建议在 `hid.rs` 增加同句柄助手，供 `extra.rs` 三条路由共用：

```rust
// hid.rs（建议签名；实现时用 HidD_SetFeature / HidD_GetFeature）
pub fn feature_set_get_on(
    handle: HANDLE,
    request: &[u8],
    read_len: usize,
    delay: std::time::Duration,
) -> Option<Vec<u8>> {
    if request.len() != read_len {
        return None;
    }
    let set_ok = unsafe {
        HidD_SetFeature(handle, request.as_ptr() as *const c_void, request.len() as u32)
    };
    if !set_ok {
        return None;
    }
    std::thread::sleep(delay);
    let mut reply = vec![0u8; read_len];
    let get_ok = unsafe {
        HidD_GetFeature(handle, reply.as_mut_ptr() as *mut c_void, reply.len() as u32)
    };
    get_ok.then_some(reply)
}

// PixArt 用这个：只 SET 一次，后续只 GET；A0/A3 由 parser 返回 None 后继续读。
pub fn feature_poll<T>(
    path: &str,
    request: &[u8],
    read_len: usize,
    attempts: u32,
    delay: std::time::Duration,
    mut parse: impl FnMut(&[u8]) -> Option<T>,
) -> Option<T> {
    with_handle(path, true, |handle| {
        if request.len() != read_len {
            return None;
        }
        let set_ok = unsafe {
            HidD_SetFeature(handle, request.as_ptr() as *const c_void, request.len() as u32)
        };
        if !set_ok {
            return None;
        }
        for _ in 0..attempts {
            std::thread::sleep(delay);
            let mut reply = vec![0u8; read_len];
            let get_ok = unsafe {
                HidD_GetFeature(handle, reply.as_mut_ptr() as *mut c_void, reply.len() as u32)
            };
            if get_ok {
                if let Some(value) = parse(&reply) {
                    return Some(value);
                }
            }
        }
        None
    })
}

// MCHOSE/Delux 用这个：每次 attempt 都 SET+GET。
pub fn feature_exchange<T>(
    path: &str,
    read_len: usize,
    attempts: u32,
    delay: std::time::Duration,
    mut build_request: impl FnMut(u32) -> Vec<u8>,
    mut parse: impl FnMut(&[u8]) -> Option<T>,
) -> Option<T> {
    with_handle(path, true, |handle| {
        for attempt in 0..attempts {
            let request = build_request(attempt);
            if let Some(reply) = feature_set_get_on(handle, &request, read_len, delay) {
                if let Some(value) = parse(&reply) {
                    return Some(value);
                }
            }
        }
        None
    })
}
```

约束：
- `read_len` 必须等于 `HidD_GetFeature` 的 `FeatureReportByteLength`（含 report ID）。不要用比 caps 短的 buffer 试探。
- **PixArt 用 `feature_poll`**（SET 一次）；**MCHOSE/Delux 用 `feature_exchange`**（每次 attempt 都 SET）。不要反过来。
- **MCHOSE 的短/长两个通道必须共用一个 `with_handle`**：用 `hid::with_handle(path, true, |handle| { for (report_id, len) in [(0x11,21),(0x12,65)] { ... feature_set_get_on(handle, ...) ... } })`，不要调用两次 `feature_exchange`（那会打开两次句柄）。
- `HidD_SetFeature` 失败时直接结束本次协议尝试，不要像 FerrisPulse 那样改发 64 字节；64 字节是 WebHID 侧的 payload 习惯，不是 Windows 的 caps 长度。
- `open_path(path, true)` 在写权限失败时会回退只读，随后 `HidD_SetFeature` 自然失败；这是可接受的保守行为。
- 扫描线程里每个设备只做一次有限预算；不要出现 `while(1)` 或每轮扫描反复 0.75 s 的无效写。

### 1.2 写入纪律（三条协议共同）

1. **先精确匹配，再写。** 任何 VID/PID、usage、interface、`feature_len` 不满足的接口，连 SET 都不发。
2. **只写目标 collection。** 同一接收器上可能有多个 `0xFF00+` collection；不要像 `wlmouse.py` 那样遍历所有 0xFFFF collection，避免每次失败 ~0.75 s，也避免误写。
3. **不写 0xFF0B、0xFFA0、FFA5 等旁路 collection。** 这些在 MCHOSE/A5 诊断里存在但不回答；`0xFF0B` 尤其不能访问。
4. **report ID 与长度必须成对**：PixArt `0x00/65`；MCHOSE `0x11/21`、`0x12/65`；Delux `0x0C/33`。
5. **离线/休眠返回 `None`，不合成 0%。** 缓存、灰显、后退由上层决定；本层只报告真实帧。
6. **所有新路由先 `Confidence::Community`**；不要把“编译通过”或“上游有实机”写成 `Verified`。

---

## 2. 协议 A：PixArt / JM WebHID 64B Feature

### 2.1 精确匹配条件

| 条件 | 值 |
|---|---|
| VID/PID | 见 §2.2 白名单，必须精确匹配 |
| `usage_page` | `0xFFFF` |
| `usage` | `0x0000` 优先；`2023` A5 允许同一接口的 `0xFFFF:0x0001` 作为第二候选 |
| `feature_len` | `65`（64 字节 Feature payload + 1 字节 report ID） |
| report ID | `0x00` |
| 接口 | LAMZU Maya X：Interface 2（`lamzu.py:40-54`）；WLmouse/A5/R5：以 usage 0 的 collection 为准，不按接口号写死 |
| 禁止 | 无 `0xFFFF`、`feature_len != 65`、未知 PID、键盘/鼠标标准 collection |

说明：`HaloBattery` 的 descriptor fingerprint（`06ffff0900a10109001500250175089540b102c0`）是 64 字节 Feature、usage page `0xFFFF`、usage 0；`attackshark-battery-bridge` 的 profile 也把 `report_length` 定为 65（TOML `:17`）。Windows 的 `HidD_*` 路径按 65 字节 buffer 实现；如果某个栈的 caps 报 64，先记录并单独处理，不要静默把 report ID 0 当作数据首字节。AnyBattery 的 HID 层不暴露原始 report descriptor，因此匹配用“已知 VID/PID + product 关键字 + caps”，不要试图在运行时比对 descriptor fingerprint。

### 2.2 首批支持的 VID/PID（严格白名单）

| VID:PID | 型号 | 上游证据 | 本仓库初始等级 |
|---|---|---|---|
| `36A7:A880` | WLmouse Beast X Max（8K 接收器） | HaloBattery `wlmouse.py:30`，文档称实机确认 | Community（待本机验证） |
| `36A7:A887` | WLmouse Beast X | HaloBattery `wlmouse.py:28`，接收器表，未确认 | Community only |
| `36A7:A868` | WLmouse Beast X Mini Pro | HaloBattery `wlmouse.py:29`，未确认 | Community only |
| `373E:001E` | LAMZU Maya X 8K dongle | HaloBattery `lamzu.py:34-41`，文档称实机确认 | Community（待本机验证） |
| `373E:001C` | LAMZU Maya X cable | HaloBattery `lamzu.py:34-37` | Community |
| `373E:0046` | Attack Shark R5 Ultra wired | attackshark-battery-bridge TOML `:11-13` | Community only |
| `373E:0047` | Attack Shark R5 Ultra wireless | 同上 | Community only |
| `2023:F013` | MCHOSE A5 Pro Max 1K receiver | HaloBattery `mchose.py:107-112`，issue #149 硬件确认 | Community（待本机验证） |
| `2023:F015` | MCHOSE A5 Pro Max 4K receiver | 同上，未试 | Community only |
| `2023:F019` | MCHOSE A5 Pro Max wired | 同上，未试 | Community only |

同一 VID/PID 若出现多个产品型号，额外要求 product 字符串包含：`Attack Shark`（`373E:0046/0047`）、`LAMZU`（`373E:001E/001C`）、`WLmouse` 或已知 PID（`36A7`）、`MCHOSE`（`2023`/`3837`）。裸 VID/PID 匹配但 product 不匹配时，宁可 `None`，不要写。

**明确不在本批白名单**：
- `373E:0001..0005`（LAMZU Atlantis/Thorn/Maya 等）：`lamzu.py:16-18` 明确说只有 Maya X 的 ID 与来源一致，其他 Lamzu 用别的 VID/协议。报告把它们列为已验证是过度声明。
- `093A:0001/0002`（Incott GHero/HPC01M Pro）：只在 FerrisPulse 的产品表里出现，没有 PixArt Feature 的实测；`mouse-battery-tray` 对 Incott 的 `093A:522C/622C` 走的是被动 `0x03` 帧，不是本 0x83 Feature。不要把 Incott 塞进本条。
- `093A:522C/622C`：如要做，走单独的被动 `0x03` 路由；本批不做。
- `33E4:*`（G-Wolves）：与 WLmouse/LAMZU 同一固件家族，但 HaloBattery `gwolves.py` 明确“Unverified”；本批不纳入，后续单独任务。
- `2023` 的其他 PID（例如 Glorious Model D 接收器）：即使 VID 相同也不写。

### 2.3 传输步骤（同一句柄）

1. 用 `hid::enumerate()` 得到候选接口；只选 §2.1 的白名单 + usage 0（A5 第二候选 usage 1）。
2. 构造 65 字节请求：

```
00 00 00 02 02 00 83 00 ... 00   (65 字节)
```

等价写法：`[0x00, 0x00, 0x02, 0x02, 0x00, 0x83]` 作为 payload（64 字节，前置 report ID 0 后 65 字节）。注意报告表格里的 `00 00 02 02 00 83` 是 **不含 report ID 的 payload**；实际 `HidD_SetFeature` 要 65 字节。

3. `HidD_SetFeature(handle, request, 65)`。
4. 同一句柄循环 `HidD_GetFeature(handle, buf, 65)`：
   - 最多 15 次，间隔 50 ms（HaloBattery `wlmouse.py:95-107`）；或按 FerrisPulse 的 12×8 ms 作为更快上限。建议本仓库统一 15×50 ms，避免忙等。
   - 若 `buf[1] == 0xA0`（等待帧）或 `buf[1] == 0xA3`（重试/ACK 帧，attackshark-battery-bridge TOML `:22`）→ 跳过；A3 时继续 GET；bridge 的 profile 限制 2 次 follow-up（`max_follow_up_gets=2`，只做 `feature_get`，不重发 SET），本实现用 15 次总预算覆盖。
   - 只有 §2.4 的严格 parser 通过才返回；否则继续。
5. 全部失败返回 `None`。不要用 6 秒被动监听作为本批的兜底（WLmouse 的 heartbeat 是另一条输入通道，本批不扩大）。

**同句柄要求**：SET 和所有 GET 必须在同一 `HANDLE` 上。`hid::feature_set`/`feature_get` 每次重新打开，不能保证响应还在当前 collection 的 feature buffer 中；本协议按 HaloBattery/FerrisPulse 的同一句柄实现。

### 2.4 严格 parser 规范

```rust
#[derive(Debug, PartialEq, Eq)]
struct PixArtReading {
    level: u8,
    charging: bool,
}

fn parse_pixart_reply(raw: &[u8]) -> Option<PixArtReading> {
    // 只接受 Windows HidD_GetFeature 的 65 字节回复（含 report ID）。
    if raw.len() != 65 {
        return None;
    }
    if raw[0] != 0x00 {
        return None;
    }
    // 头必须固定在 offset 1，不能像报告示例那样全 buffer 搜索。
    if !matches!(raw[1], 0xA1 | 0xA2) {
        return None;
    }
    if raw[2] != 0x00 || raw[3] != 0x02 || raw[4] != 0x02 || raw[5] != 0x00 {
        return None;
    }
    if raw[6] != 0x83 {
        return None;
    }
    let charging = match raw[7] {
        0 => false,
        1 => true,
        _ => return None,
    };
    let level = raw[8];
    if level > 100 {
        return None;
    }
    Some(PixArtReading { level, charging })
}
```

偏移表（raw 65 字节）：

| offset | 含义 |
|---|---|
| 0 | report ID `0x00` |
| 1 | 响应标记 `0xA1`（主）/`0xA2`（HaloBattery、mouse-battery-tray 也接受） |
| 2..5 | `00 02 02 00` |
| 6 | `0x83` 命令回显 |
| 7 | 充电标志，严格 0/1 |
| 8 | 电量 0..100 |
| 9..64 | 不解释，保留原文用于诊断 |

**与报告的关键差异**：
- 报告说“payload[6] 充电、payload[7] 电量”，那是去掉 report ID 后的 64 字节视图；raw 65 字节是 `raw[7]/raw[8]`。实现只按 raw 65 写，避免两套偏移混用。
- 报告示例 parser 在 `0..=len-8` 全 buffer 搜索 `A1 00 02 02 00 83`。这会把别处的字节误认成头；本实现要求头固定在 offset 1。
- 报告只写 A1；HaloBattery `wlmouse.py:54` 与 `mouse-battery-tray/devices.py:94` 都接受 A2。本实现接受 A1/A2，A0/A3 一律拒绝。
- 报告未规定长度；本实现只接受 65。不要把 64 字节（无 report ID）当合法；如果某些栈只返回 64，应在传输层记录并单独讨论，不要静默接受。
- 报告示例 `lvl <= 100` 接受 0；FerrisPulse `UniversalGamingDeviceHelper.cs:434` 要求 `pct > 0`。本实现接受 `level == 0`，因为 0 是合法电量且头已证明是真帧；只有“无头/全零”才算离线。真机若发现 0 是休眠假帧，再收紧。

### 2.5 必测 fixtures（建议放在 `extra.rs` 的 `#[cfg(test)] mod tests`）

```rust
fn pixart_reply(marker: u8, charging: u8, level: u8) -> Vec<u8> {
    let mut raw = vec![0u8; 65];
    raw[..9].copy_from_slice(&[
        0x00, marker, 0x00, 0x02, 0x02, 0x00, 0x83, charging, level,
    ]);
    raw
}

#[test]
fn pixart_valid_charging() {
    assert_eq!(
        parse_pixart_reply(&pixart_reply(0xA1, 1, 75)),
        Some(PixArtReading { level: 75, charging: true })
    );
}

#[test]
fn pixart_valid_a2_and_zero_level() {
    assert_eq!(
        parse_pixart_reply(&pixart_reply(0xA2, 0, 0)),
        Some(PixArtReading { level: 0, charging: false })
    );
}

#[test]
fn pixart_wrong_header_is_rejected() {
    for marker in [0x00, 0x82, 0xA0, 0xA3, 0xFF] {
        assert_eq!(parse_pixart_reply(&pixart_reply(marker, 0, 50)), None);
    }
    // 头不在 offset 1 也必须拒绝
    let mut shifted = vec![0u8; 65];
    shifted[..10].copy_from_slice(&[
        0x00, 0x00, 0xA1, 0x00, 0x02, 0x02, 0x00, 0x83, 0x00, 0x50,
    ]);
    assert_eq!(parse_pixart_reply(&shifted), None);
}

#[test]
fn pixart_wrong_length_is_rejected() {
    assert_eq!(parse_pixart_reply(&pixart_reply(0xA1, 0, 50)[..64]), None);
    let mut long = pixart_reply(0xA1, 0, 50);
    long.push(0);
    assert_eq!(parse_pixart_reply(&long), None);
    assert_eq!(parse_pixart_reply(&[]), None);
}

#[test]
fn pixart_wrong_report_id_is_rejected() {
    let mut raw = pixart_reply(0xA1, 0, 50);
    raw[0] = 0x01;
    assert_eq!(parse_pixart_reply(&raw), None);
}

#[test]
fn pixart_out_of_range_and_bad_charge_are_rejected() {
    assert_eq!(parse_pixart_reply(&pixart_reply(0xA1, 0, 101)), None);
    for charging in [2, 0x65, 0xFF] {
        assert_eq!(parse_pixart_reply(&pixart_reply(0xA1, charging, 50)), None);
    }
}

#[test]
fn pixart_silent_or_zero_buffer_is_rejected() {
    assert_eq!(parse_pixart_reply(&vec![0u8; 65]), None);
    // A1 头但电量 0 仍合法，不能与“全零离线”混淆
    assert_eq!(
        parse_pixart_reply(&pixart_reply(0xA1, 0, 0)),
        Some(PixArtReading { level: 0, charging: false })
    );
}
```

### 2.6 最小文件修改建议

1. `crates/anybattery-windows/src/hid.rs`
   - 新增 §1.1 的 `feature_poll`（PixArt 用）；`feature_set_get_on` 也由本文件提供。
   - 不修改现有 `feature_set`/`feature_get` 的语义，避免影响 VAXEE/Endgame。
2. `crates/anybattery-windows/src/extra.rs`
   - 新增 `parse_pixart_reply`、`read_pixart(info) -> Option<VendorReading>`。
   - 在 `read_vendor_extra()` 中，放在 1D57 分支之后、Razer 分支之前（VID 不冲突，顺序只为可读性）。
   - `read_pixart` 只对 §2.2 白名单 + §2.1 的 usage/feature_len 发请求；provider 建议 `"PixArt/JM 64B"`，`verification: Confidence::Community`，`kind: Some(DeviceKind::Mouse)`，`name` 用型号表，`detail` 写 `feature 0x83 · raw[7]=.. raw[8]=..`。

参考骨架（`pixart_name` 按 §2.2 表返回型号名，`pixart_product_ok` 实现 product 关键字过滤；`verification` 统一 `Confidence::Community`）：

```rust
fn read_pixart(info: &hid::HidInterface) -> Option<VendorReading> {
    if info.usage_page != 0xFFFF || info.feature_len != 65 {
        return None;
    }
    if info.usage != 0 && !(info.vendor_id == 0x2023 && info.usage == 1) {
        return None;
    }
    if !pixart_product_ok(info) {
        return None;
    }
    let name = pixart_name(info.vendor_id, info.product_id)?;
    let mut request = [0u8; 65];
    request[..7].copy_from_slice(&[0x00, 0x00, 0x00, 0x02, 0x02, 0x00, 0x83]);
    let parsed = hid::feature_poll(
        &info.path,
        &request,
        65,
        15,
        Duration::from_millis(50),
        parse_pixart_reply,
    )?;
    Some(reading(
        parsed.level,
        Some(parsed.charging),
        "PixArt/JM 64B",
        Confidence::Community,
        Some(name.to_owned()),
        Some(DeviceKind::Mouse),
        format!("pixart:{:04X}", info.product_id),
        format!("feature 0x83 · raw[7]={} raw[8]={}", parsed.charging as u8, parsed.level),
    ))
}
```
3. `crates/anybattery-windows/src/scan.rs` / `hid.rs`
   - 可选但建议：把 `0x36A7 | 0x373E | 0x2023` 加入 `is_wireless_vid()` 与 `likely_wireless()`，让 2.4G 设备在诊断里标成 `Usb24g`；不影响 parser。不要在 `transport_for` 里把 `0x248A` 一刀切，因为 Delux 同时有有线和无线 PID。
4. 测试只加在 `extra.rs` 的现有 test module，不新建测试文件。
5. 不改 README、release、docs 其他文件。

### 2.7 真机验证步骤（PixArt）

1. 插入接收器，运行现有扫描，记录诊断中的 `VID:PID`、`interface_number`、`usage_page:usage`、`in/out/feature`、`product`。
2. 确认目标 collection 的 `feature_len == 65`、usage page `0xFFFF`、usage 0；LAMZU 必须是 Interface 2 的 `FFFF:0000`。如果不是，停止，不要发 SET。
3. 用只读抓包（USBPcap/Wireshark）或临时独立探针发送一次 §2.3 请求，记录 15 次 GET 的原始 65 字节；确认：
   - 第一次可能是 A0/A3；最终出现 `00 A1 00 02 02 00 83 <chg> <level>`；
   - `level` 与官方软件（WLmouse HUB / LAMZU 软件 / Attack Shark 软件 / MCHOSE M HUB）一致；
   - 插线/放底座时 `raw[7]` 从 0 变 1，拔线恢复 0；
   - 鼠标休眠/关机时没有伪造 0% 帧。
4. 负向验证：同一 VID 下未列入白名单的 PID（如 `373E:0001`、`2023` 其他 PID）不得收到 SET；扫描日志应显示“未匹配/未写入”。
5. 扫描耗时：单设备最多 ~0.75 s（15×50 ms）；接收器睡眠时不得每轮无限重试。若超过，调整预算而不是加长 `while`。
6. 通过后按型号升级：先 `36A7:A880`、`373E:001E`、`2023:F013`（上游有硬件确认），其余保持 Community。

### 2.8 来源、commit、永久链接、许可证

| 来源 | commit | 文件/行 | 许可 | 用途 |
|---|---|---|---|---|
| HaloBattery | `2251f0534a8e886c9692ead42056059b4f69d8c0` | [`providers/wlmouse.py#L1-L140`](https://github.com/HeyOkay/HaloBattery/blob/2251f0534a8e886c9692ead42056059b4f69d8c0/providers/wlmouse.py#L1-L140) | MIT | 请求/回复、A1/A2、15×50 ms、心跳 |
| HaloBattery | 同上 | [`providers/lamzu.py#L1-L135`](https://github.com/HeyOkay/HaloBattery/blob/2251f0534a8e886c9692ead42056059b4f69d8c0/providers/lamzu.py#L1-L135) | MIT | Maya X 001E/001C、Interface 2 |
| HaloBattery | 同上 | [`providers/mchose.py#L54-L67`](https://github.com/HeyOkay/HaloBattery/blob/2251f0534a8e886c9692ead42056059b4f69d8c0/providers/mchose.py#L54-L67) | MIT | A5 Pro Max 0xFFFF 0x83（PixArt 帧） |
| HaloBattery | 同上 | [`docs/protocols.md#L171-L181`](https://github.com/HeyOkay/HaloBattery/blob/2251f0534a8e886c9692ead42056059b4f69d8c0/docs/protocols.md#L171-L181) | MIT | LAMZU/A5 的形状与确认范围 |
| HaloBattery | 同上 | [`tests/test_wlmouse.py#L35-L40`](https://github.com/HeyOkay/HaloBattery/blob/2251f0534a8e886c9692ead42056059b4f69d8c0/tests/test_wlmouse.py#L35-L40) | MIT | 65 字节 fixture |
| HaloBattery | 同上 | [`tests/test_lamzu.py#L22-L60`](https://github.com/HeyOkay/HaloBattery/blob/2251f0534a8e886c9692ead42056059b4f69d8c0/tests/test_lamzu.py#L22-L60) | MIT | 65 字节回复/仅 Interface 2 |
| HaloBattery | 同上 | [`tests/test_mchose.py#L84-L98`](https://github.com/HeyOkay/HaloBattery/blob/2251f0534a8e886c9692ead42056059b4f69d8c0/tests/test_mchose.py#L84-L98) | MIT | A5 Pro Max 的 0x83 请求/解析（PixArt 帧，不是反码） |
| FerrisPulse | `4917a10e238ed3dba2938969752886353c8ff5e9` | [`src/Hardware/UniversalGamingDeviceHelper.cs#L376-L448`](https://github.com/Ferris-echo/FerrisPulse/blob/4917a10e238ed3dba2938969752886353c8ff5e9/src/Hardware/UniversalGamingDeviceHelper.cs#L376-L448) | GPL-3.0（只取事实） | 65 字节 tx、12×8 ms、A0 跳过、chg 0/1 |
| attackshark-battery-bridge | `2d77aeb5cd8a9fa3408416200cc86de61221f145` | [`profiles/attack_shark_r5_ultra.toml#L5-L36`](https://github.com/maxboeer/attackshark-battery-bridge/blob/2d77aeb5cd8a9fa3408416200cc86de61221f145/src/attackshark_battery_bridge/profiles/attack_shark_r5_ultra.toml#L5-L36) | MIT | R5 Ultra 65B、raw[7]/[8]、A3 重试 |
| mouse-battery-tray | `c1c6059472dab366c700a7f7516e0dbffde68fd7` | [`devices.py#L80-L131`](https://github.com/incconutwo/mouse-battery-tray/blob/c1c6059472dab366c700a7f7516e0dbffde68fd7/devices.py#L80-L131) | MIT | A1/A2、15×50 ms、passive 兜底 |

---

## 3. 协议 B：MCHOSE（迈从）反码 Feature

### 3.1 精确匹配条件

| 条件 | 值 |
|---|---|
| VID/PID | 仅 §3.2 白名单；**不要按 VID 泛匹配** |
| `usage_page` | `0xFF01` |
| `usage` | `0x0001` |
| `feature_len` | ≥`21`；短报文按 `21` 发/读，长报文按 `65`（caps 常报最大值 65） |
| report ID | `0x11` 短 / `0x12` 长；均为 Feature |
| 接口 | 优先 Interface 2（诊断显示 `0xFF0B:0x104` 与 `0xFF01:0x01`）；只写 `0xFF01`，若不在 2 先抓包再决定 |
| 禁止 | `0xFF0B`、`0xFFA0`、`FFA5`、输出报告、`3837:600A` 耳机 |

**安全重点**：`0x3837` 同时被 MCHOSE V9 Turbo+ 耳机（`3837:600A`）使用，耳机是 Input `0x55` 事件协议（`battery-hub/src/main/hid/drivers/mchose.js`，MIT），与本反码协议无关。若按 VID 泛匹配，会把 Feature 写操作发给耳机，必须用 PID 白名单排除。

### 3.2 首批支持的 VID/PID

| VID:PID | 型号 | 上游证据 | 本仓库初始等级 |
|---|---|---|---|
| `5253:1020` | MCHOSE M7 Ultra 2.4G | HaloBattery `mchose.py:1-33`，固件 5.2.7.0 实测 | Community（待本机验证） |
| `5253:0031` | MCHOSE M7 Ultra USB | HaloBattery `mchose.py:31-33`，实测有线 | Community |
| `3837:100C` | MCHOSE A5 V2 Ultra 2.4G receiver | FerrisPulse `MchoseMouseHelper.cs:29-32` | Community only |
| `3837:1101` | MCHOSE A5 V2 Ultra USB | 同上 | Community only |
| `3837:100B` | MCHOSE A7 V2 Ultra 2.4G receiver | HaloBattery `docs/protocols.md:183-187` + issue #4 | Community only |

不在白名单：`5253` 其他 PID、`3837` 其他 PID（尤其 `600A` 耳机）、MCHOSE A5 Pro Max `2023:F0xx`（那是 PixArt 0x83，走 §2）、MCHOSE G7 `A8A5:2255`（Output 65B + Input `AA 30`，本批不做）。

### 3.3 传输步骤（同一句柄）

1. 只对白名单 VID/PID 且 `usage_page==0xFF01 && usage==1` 的接口操作。
2. 先短报文 `0x11`（21 字节），若接口 `feature_len >= 21`：

```
11 F9 FF FF FF FF FF FF FF FF FF FF FF FF FF FF FF FF FF FF FF   (21 字节)
```

即 `[0x11] + invert([0x06] + [0x00;19])`。`F9 = 0x06 ^ 0xFF`；其余 payload 全 `FF`。

3. 同一句柄 `HidD_SetFeature(handle, req, 21)`，等待 ~120 ms，`HidD_GetFeature(handle, buf, 21)`；解析见 §3.4。
4. **每次 GET 前重新 SET**。HaloBattery `mchose.py:19-22` 明确：一次请求后反复读只会拿到请求回显/中间态；真实值通常在第 2 次交换。每通道最多 4 次，间隔 120 ms（`mchose.py:137-141`）。
5. 短通道无有效帧且 `feature_len >= 65` 时，再试长报文 `0x12`（65 字节）：

```
12 F9 FF...FF   (65 字节)
```

同样每次 GET 前重发，最多 4 次。`HidD_GetFeature` 读 65 字节。
6. 同一句柄贯穿短/长两个通道；不要在通道之间重新 `open_path`。
7. 全部失败返回 `None`；不要合成 0%。

**为什么必须重发**：HaloBattery `mchose.py:19-22` 与 `253-281` 的行为是“re-ask rather than re-read”；FerrisPulse `MchoseMouseHelper.cs:274-315` 的 `SendCommand` 也是每次 attempt 重新 `SendOnly`，并检查 `response[1] == body[0]^0xFF`。不要照搬报告里“发一次、连续读”的简化描述。

### 3.4 严格 parser 规范

```rust
#[derive(Debug, PartialEq, Eq)]
struct MchoseReading {
    level: u8,
    charging: bool,
    model: u16,
    flags: u8,
    firmware: u32,
}

fn parse_mchose_reply(raw: &[u8], report_id: u8, device_vid: u16) -> Option<MchoseReading> {
    let expected_len = match report_id {
        0x11 => 21,
        0x12 => 65,
        _ => return None,
    };
    if raw.len() != expected_len {
        return None;
    }
    if raw[0] != report_id || raw[1] != 0xF9 {
        return None;
    }
    let payload: Vec<u8> = raw[2..].iter().map(|b| b ^ 0xFF).collect();
    if payload.len() < 11 {
        return None;
    }
    let vid = u16::from_le_bytes([payload[0], payload[1]]);
    if vid != device_vid || !matches!(vid, 0x5253 | 0x3837) {
        return None;
    }
    let model = u16::from_le_bytes([payload[2], payload[3]]);
    let firmware = u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
    let flags = payload[8];
    let level = payload[9];
    let charging = match payload[10] {
        0 => false,
        1 => true,
        _ => return None,
    };
    if level > 100 {
        return None;
    }
    Some(MchoseReading { level, charging, model, flags, firmware })
}
```

偏移表（解码后的 payload，`raw[2..] ^ 0xFF`）：

| payload offset | 含义 |
|---|---|
| 0..1 | VID 小端（`0x5253`/`0x3837`） |
| 2..3 | model 小端（M7 Ultra 常见 `0x0031`） |
| 4..7 | firmware u32 小端 |
| 8 | flags（`0x09` 2.4G，`0x00` 有线；只记录，不参与接受/拒绝） |
| 9 | 电量 0..100（**0 合法**，只要 VID 有效） |
| 10 | 充电 0/1 |
| 11.. | 第二组 level/charge（当前不解释） |

**与报告的关键差异**：
- 报告写“短 20 字节 / 长 64 字节”。那是 payload；`HidD_SetFeature`/`GetFeature` 的 on-wire 长度是 **21/65**（含 report ID）。HaloBattery `mchose.py:10-11,127-128` 与 FerrisPulse `MchoseMouseHelper.cs:34-35` 都按 payload 20/64 描述，实际 buffer `payloadLength + 1`。
- 报告要求“`payload[9] > 0` 且 `vid != 0`”才算有效。前半句错误：真实 0% 且 VID 有效是合法读数；休眠帧是 **all-zero payload**，解码后 `vid == 0`，用 VID/已知型号拒绝即可。HaloBattery `parse_status` 只要求 `vid in MCHOSE_VIDS` 且 `level <= 100`，允许 0；FerrisPulse `ReadIdentity` 也允许 0。不要用 `level > 0` 当离线判据。
- 报告说“0xFF0B 永远不响应，切勿访问”，这点正确；FerrisPulse 的 `FindControlPath` 却把 `0xFF0B` 当 fallback（`MchoseMouseHelper.cs:317-414`）。本实现只认 `0xFF01`，不要照抄 fallback。
- 报告把型号写成 “A7 V2”；HaloBattery 的实测/诊断是 **A7 V2 Ultra**（`docs/protocols.md:183-187`）。名称按后者。
- 报告未强调“每次 GET 前重发”；这是本协议最容易写错的地方，必须实现。

### 3.5 必测 fixtures

短报文，100% 放电（解码 payload `53 52 31 00 02 05 07 00 09 64 00 64 00` + 6 个 0）：

```rust
const MCHOSE_OK_100_DISCHARGING: [u8; 21] = [
    0x11, 0xF9, 0xAC, 0xAD, 0xCE, 0xFF, 0xFD, 0xFA, 0xF8, 0xFF, 0xF6,
    0x9B, 0xFF, 0x9B, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
];
const MCHOSE_OK_100_CHARGING: [u8; 21] = [
    0x11, 0xF9, 0xAC, 0xAD, 0xCE, 0xFF, 0xFD, 0xFA, 0xF8, 0xFF, 0xF6,
    0x9B, 0xFE, 0x9B, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
];

#[test]
fn mchose_valid_short_discharging() {
    assert_eq!(
        parse_mchose_reply(&MCHOSE_OK_100_DISCHARGING, 0x11, 0x5253),
        Some(MchoseReading {
            level: 100,
            charging: false,
            model: 0x0031,
            flags: 0x09,
            firmware: 0x0007_0502,
        })
    );
}

#[test]
fn mchose_valid_short_charging() {
    assert_eq!(
        parse_mchose_reply(&MCHOSE_OK_100_CHARGING, 0x11, 0x5253),
        Some(MchoseReading {
            level: 100,
            charging: true,
            model: 0x0031,
            flags: 0x09,
            firmware: 0x0007_0502,
        })
    );
}

#[test]
fn mchose_long_report_is_accepted_and_short_id_is_rejected() {
    let mut raw = vec![0xFFu8; 65];
    raw[0] = 0x12;
    raw[1] = 0xF9;
    let payload = [0x53, 0x52, 0x31, 0x00, 0x02, 0x05, 0x07, 0x00, 0x09, 0x50, 0x01];
    for (i, byte) in payload.iter().enumerate() {
        raw[2 + i] = *byte ^ 0xFF;
    }
    assert_eq!(
        parse_mchose_reply(&raw, 0x12, 0x5253).map(|r| (r.level, r.charging)),
        Some((80, true))
    );
    assert_eq!(parse_mchose_reply(&raw, 0x11, 0x5253), None);
}

#[test]
fn mchose_zero_level_with_identity_is_valid() {
    let mut raw = MCHOSE_OK_100_DISCHARGING;
    raw[11] = 0xFF; // payload[9] = 0x00 -> level 0
    assert_eq!(
        parse_mchose_reply(&raw, 0x11, 0x5253).map(|r| (r.level, r.charging)),
        Some((0, false))
    );
}

#[test]
fn mchose_sleep_payload_is_rejected() {
    // receiver answers with all-zero payload: raw[2..] = FF, decoded VID = 0
    let mut raw = [0x11u8; 21];
    raw[1] = 0xF9;
    for b in &mut raw[2..] {
        *b = 0xFF;
    }
    assert_eq!(parse_mchose_reply(&raw, 0x11, 0x5253), None);
}

#[test]
fn mchose_wrong_headers_lengths_and_ranges_are_rejected() {
    let mut wrong_cmd = MCHOSE_OK_100_DISCHARGING;
    wrong_cmd[1] = 0xF8;
    assert_eq!(parse_mchose_reply(&wrong_cmd, 0x11, 0x5253), None);

    let mut wrong_report = MCHOSE_OK_100_DISCHARGING;
    wrong_report[0] = 0x12;
    assert_eq!(parse_mchose_reply(&wrong_report, 0x11, 0x5253), None);
    assert_eq!(parse_mchose_reply(&MCHOSE_OK_100_DISCHARGING[..20], 0x11, 0x5253), None);

    let mut out_of_range = MCHOSE_OK_100_DISCHARGING;
    out_of_range[11] = 0x9A; // payload[9] = 0x65 = 101
    assert_eq!(parse_mchose_reply(&out_of_range, 0x11, 0x5253), None);

    let mut bad_charge = MCHOSE_OK_100_DISCHARGING;
    bad_charge[12] = 0xFD; // payload[10] = 0x02
    assert_eq!(parse_mchose_reply(&bad_charge, 0x11, 0x5253), None);

    // 3837 设备却报 5253 VID：严格相等，拒绝；若真机发现跨 VID，再按抓包放宽
    assert_eq!(parse_mchose_reply(&MCHOSE_OK_100_DISCHARGING, 0x11, 0x3837), None);
}
```

长报文 fixture：把同一 19 字节 payload 后补 44 个 `0x00`（共 63 字节），整体反码后放在 `[0x12, 0xF9]` 之后，长度 65；测试应断言 `report_id=0x12` 可解析、`report_id=0x11` 因长度/ID 不匹配被拒绝。

### 3.6 最小文件修改建议

1. `crates/anybattery-windows/src/hid.rs`
   - 复用 §1.1 的 `feature_set_get_on` 与 `with_handle`；MCHOSE 的两个通道共用一个 handle。
2. `crates/anybattery-windows/src/vendor.rs`
   - **必须移除 0x5253 的 CompX 路由**：`is_compX_vid()` 当前是 `matches!(vid, ... | 0x5253)`（`vendor.rs:89-91`），`compX_name()` 还有两条 `(0x5253, 0x1020)` / `(0x5253, 0x0031)`（`vendor.rs:355-356`）。不移除的话，每次扫描先打 3×250 ms 的 17B 请求，MCHOSE 永远不响应，既慢又误导诊断。删除 `0x5253` 和这两条名称即可；不要动其他 CompX VID。
   - 如果 B2 lane 正在改 `vendor.rs`，把这两处删除作为最小冲突面单独提交；不要顺手改 Razer/Logitech。
3. `crates/anybattery-windows/src/extra.rs`
   - 新增 `parse_mchose_reply`、`read_mchose_inverse(info)`。
   - `read_mchose_inverse` 用**一个** `hid::with_handle(path, true, ...)` 包住 `0x11`→`0x12` 两个通道，每个 attempt 调 `hid::feature_set_get_on(handle, ...)`；不要两次 `feature_exchange`（会换句柄）。
   - 在 `read_vendor_extra()` 中、1D57 之后、Razer 之前调用；只对 §3.1/§3.2 匹配。
   - `provider: "MCHOSE 反码 Feature"`，`verification: Confidence::Community`，`kind: Some(DeviceKind::Mouse)`，`name` 按 PID/model 表，`detail: "report 0x11 · vid .... model .... flags ...."`。

参考骨架（`mchose_name` 按 §3.2 表返回型号名；未知 PID 返回 `None`；`verification` 统一 `Confidence::Community`）：

```rust
fn read_mchose_inverse(info: &hid::HidInterface) -> Option<VendorReading> {
    if !matches!(info.vendor_id, 0x5253 | 0x3837)
        || info.usage_page != 0xFF01
        || info.usage != 1
    {
        return None;
    }
    let name = mchose_name(info.vendor_id, info.product_id)?;
    hid::with_handle(&info.path, true, |handle| {
        for (report_id, length) in [(0x11u8, 21usize), (0x12u8, 65usize)] {
            if (info.feature_len as usize) < length {
                continue;
            }
            let mut request = vec![0xFFu8; length];
            request[0] = report_id;
            request[1] = 0xF9;
            for _ in 0..4 {
                let Some(reply) = hid::feature_set_get_on(
                    handle,
                    &request,
                    length,
                    Duration::from_millis(120),
                ) else {
                    continue;
                };
                if let Some(parsed) = parse_mchose_reply(&reply, report_id, info.vendor_id) {
                    return Some(reading(
                        parsed.level,
                        Some(parsed.charging),
                        "MCHOSE 反码 Feature",
                        Confidence::Community,
                        Some(name.to_owned()),
                        Some(DeviceKind::Mouse),
                        format!("mchose:{:04X}", info.vendor_id),
                        format!(
                            "report {:#04X} · vid {:04X} model {:04X} flags {:02X}",
                            report_id, info.vendor_id, parsed.model, parsed.flags
                        ),
                    ));
                }
            }
        }
        None
    })
}
```
4. 测试放 `extra.rs` 现有 test module。
5. 不改 `scan.rs` 的 0x5253 wireless 标记；它仍是无线 VID。

### 3.7 真机验证步骤（MCHOSE）

1. 插入 M7 Ultra 接收器（`5253:1020`）或 A5 V2/A7 V2 Ultra 接收器（`3837:100C/100B`），确认 `0xFF01:0x01`、`feature_len` 21/65、Interface 2；同时确认存在 `0xFF0B:0x104` 但**不访问**。
2. 发送短请求 `11 F9 FF...`，记录每次 SET/GET 的 21 字节；确认：
   - 第一次可能回显请求或全零；
   - 重发后出现 `11 F9 ...`，解码 VID 与接口 VID 一致；
   - `payload[9]` 与 M HUB 显示一致；
   - 插线时 `payload[10] == 1`，拔线为 0；
   - 休眠 8 秒后回全零 payload，移动鼠标后恢复真实值——实现应返回 `None` 而非 0%。
3. 在 M7 Ultra 上验证 `5253:0031` 有线：接收器静默、鼠标自身回答；两者只产生一个有效读数（图标合并是上层策略，本层至少不要同时写两个）。
4. 对 `3837:100B/100C/1101` 只做只读抓包；没有设备就不要标 Verified。
5. 负向：`3837:600A` 耳机不得收到任何 Feature 写；`0xFF0B` 不得出现在任何 SET 日志；`A5 Pro Max 2023:F013` 应走 PixArt，不应走本路由。
6. 预算：短+长最多 8×120 ms ≈ 1 s；接收器睡眠时不要无限重试。

### 3.8 来源、commit、永久链接、许可证

| 来源 | commit | 文件/行 | 许可 | 用途 |
|---|---|---|---|---|
| HaloBattery | `2251f0534a8e886c9692ead42056059b4f69d8c0` | [`providers/mchose.py#L1-L71`](https://github.com/HeyOkay/HaloBattery/blob/2251f0534a8e886c9692ead42056059b4f69d8c0/providers/mchose.py#L1-L71) | MIT | M7 实测、0xFF01、重发、休眠 |
| HaloBattery | 同上 | [`providers/mchose.py#L83-L179`](https://github.com/HeyOkay/HaloBattery/blob/2251f0534a8e886c9692ead42056059b4f69d8c0/providers/mchose.py#L83-L179) | MIT | VID/PID、make_request、parse_status |
| HaloBattery | 同上 | [`providers/mchose.py#L253-L281`](https://github.com/HeyOkay/HaloBattery/blob/2251f0534a8e886c9692ead42056059b4f69d8c0/providers/mchose.py#L253-L281) | MIT | 每次 SET、4×120 ms、短后长 |
| HaloBattery | 同上 | [`docs/protocols.md#L183-L199`](https://github.com/HeyOkay/HaloBattery/blob/2251f0534a8e886c9692ead42056059b4f69d8c0/docs/protocols.md#L183-L199) | MIT | A7 V2 Ultra/M7 的接口与行为 |
| FerrisPulse | `4917a10e238ed3dba2938969752886353c8ff5e9` | [`src/Hardware/MchoseMouseHelper.cs#L24-L45`](https://github.com/Ferris-echo/FerrisPulse/blob/4917a10e238ed3dba2938969752886353c8ff5e9/src/Hardware/MchoseMouseHelper.cs#L24-L45) | GPL-3.0（只取事实） | VID/PID、report payload 长度 |
| FerrisPulse | 同上 | [`MchoseMouseHelper.cs#L255-L315`](https://github.com/Ferris-echo/FerrisPulse/blob/4917a10e238ed3dba2938969752886353c8ff5e9/src/Hardware/MchoseMouseHelper.cs#L255-L315) | GPL-3.0 | 重发、0xF9 校验、payload 解码 |
| FerrisPulse | 同上 | [`MchoseMouseHelper.cs#L442-L455`](https://github.com/Ferris-echo/FerrisPulse/blob/4917a10e238ed3dba2938969752886353c8ff5e9/src/Hardware/MchoseMouseHelper.cs#L442-L455) | GPL-3.0 | payload[8]/[9]/[10] 偏移 |
| battery-hub | `fb8abfbe97d1e284ae2206547cfd3daaa5d98a91` | [`src/main/hid/drivers/mchose.js`](https://github.com/DORON177/battery-hub/blob/fb8abfbe97d1e284ae2206547cfd3daaa5d98a91/src/main/hid/drivers/mchose.js) | MIT | **反例**：3837:600A 耳机是 0x55 Input，不是本协议 |

---

## 4. 协议 C：Delux M800 Pro 33B Feature

### 4.1 精确匹配条件

| 条件 | 值 |
|---|---|
| VID/PID | `248A:5B2F`（2.4G 无线）/ `248A:5B2E`（有线）；只这两个 |
| Interface | `1`（`main.c:12`、`claim_interface(INTERFACE)` `:373`） |
| `feature_len` | `33`（32 字节 payload + report ID `0x0C`） |
| report ID | `0x0C` |
| usage_page | 源码未给出；**不要**用 usage page 猜。只要求 Windows HID 枚举出 MI_01 且 `feature_len == 33` |
| 禁止 | `258A:1001/1002`（FerrisPulse 的 Delux 映射，无本协议证据）；其他 MI；其他 report ID；raw USB 猜测路径 |

**实现前提（重要）**：`m800-pro-driver` 是 Linux libusb 程序，直接对 Interface 1 发 HID class control transfer（`bmRequestType=0x21/0xA1`、`bRequest=0x09/0x01`、`wValue=0x030C`、`wIndex=1`、33 字节）。这证明协议本身，但**没有证明 Windows 会把 MI_01 暴露成 HID collection**。AnyBattery 只有 `SetupDiGetClassDevs(HID)` 枚举路径；如果 MI_01 是 vendor-specific interface，`hid::enumerate()` 不会返回它，任何 `HidD_SetFeature` 都无从谈起。实施时必须先满足：
- `hid::enumerate()` 中出现 `248A:5B2F` 或 `248A:5B2E`、`interface_number == Some(1)`、`feature_len == 33` 的接口；
- 否则**不实现写路径**，只保留文档/Community 状态，不要用 `CreateFile` 对非 HID 接口盲发 control transfer（这会把范围扩大到 WinUSB/raw USB，且未经验证）。

### 4.2 传输步骤（同一句柄）

1. 只选 `VID/PID` 精确、`interface_number == Some(1)`、`feature_len == 33` 的 HID 接口。
2. 构造 33 字节请求：

```
0C 01 20 00 <seq> 00 ... 00
```

`seq` 从 1 开始，每次尝试递增（`main.c:311-317`；成功后 `packet_id = i - 1`，后续命令用 `packet_id + 1`）。对电池读取，单次查询用 `seq=1` 即可；重试时递增。
3. 同一句柄 `HidD_SetFeature(handle, req, 33)`，等待 20–50 ms（源码无显式 sleep；建议 30 ms），`HidD_GetFeature(handle, buf, 33)`。
4. 最多 8 次尝试（源码是 `while(1)`，必须改成有界）；每次重试递增 `seq`。若 `buf[0] != 0x0C`、`raw[18] == 0`、或 `raw[19]` 不是 0/1，继续下一次；全部失败返回 `None`。
5. 不跨 Interface 写；不写 MI_00；不写 `258A` 设备。

### 4.3 严格 parser 规范

```rust
#[derive(Debug, PartialEq, Eq)]
struct DeluxReading {
    level: u8,
    charging: bool,
}

fn parse_delux_reply(raw: &[u8]) -> Option<DeluxReading> {
    if raw.len() != 33 {
        return None;
    }
    if raw[0] != 0x0C {
        return None;
    }
    let level = raw[18];
    // 源码用 if (buf[18]) 判定“有值”；0 视为未就绪/离线，而不是 0%
    if level == 0 || level > 100 {
        return None;
    }
    let charging = match raw[19] {
        0 => false,
        1 => true,
        _ => return None,
    };
    Some(DeluxReading { level, charging })
}
```

偏移表（raw 33）：

| offset | 含义 |
|---|---|
| 0 | report ID `0x0C` |
| 1 | `0x01`（命令簇；源码不校验） |
| 2 | `0x20`（命令；源码不校验） |
| 3 | `0x00` |
| 4 | 序列号 `seq`（1,2,...） |
| 5..17 | 0 / 未用 |
| 18 | 电量 1..100；0 = 未就绪/离线 |
| 19 | 充电：`1` 充电，`0` 放电；其他值拒绝 |
| 20..32 | 未用 |

**与报告的关键差异**：
- 报告说 `byte[19]` 为 `1 或 0x65` 表示充电。源码 `main.c:308` 的 `BATTERY_CHARGING 0x65` 是驱动内部返回值（`initial_query` 返回它，`main` 用 `charge == BATTERY_CHARGING` 打印 "charging"），**不是线上字节**。线上判断只有 `buf[19] == 1`（`main.c:326-329`）。`0x65` 在线上应视为未知/异常，不能当充电。
- 报告说“Verified (C 语言驱动实机)”。这只证明 Linux libusb 路径；Windows HID 枚举与 `HidD_*` 路径未验证，且可能不存在。初始只能 Community；若 MI_01 不是 HID，则本批不支持。
- 报告未提 `while(1)` 的风险。源码在无响应或 `buf[18] == 0` 时会无限循环；AnyBattery 必须改成 8 次上限，避免扫描线程卡死。
- 报告说 `248A:5B2F/5B2E`，与 `main.c:9-11` 一致；FerrisPulse `UniversalGamingDeviceHelper.cs:275-276` 的 `258A:1001/1002` 是另一套映射，不能当 M800 Pro 本协议。

### 4.4 必测 fixtures

```rust
fn delux_reply(level: u8, charging: u8) -> Vec<u8> {
    let mut raw = vec![0u8; 33];
    raw[0] = 0x0C;
    raw[1] = 0x01;
    raw[2] = 0x20;
    raw[4] = 0x01;
    raw[18] = level;
    raw[19] = charging;
    raw
}

#[test]
fn delux_valid_discharging() {
    assert_eq!(
        parse_delux_reply(&delux_reply(80, 0)),
        Some(DeluxReading { level: 80, charging: false })
    );
}

#[test]
fn delux_valid_charging_and_full() {
    assert_eq!(
        parse_delux_reply(&delux_reply(80, 1)),
        Some(DeluxReading { level: 80, charging: true })
    );
    assert_eq!(
        parse_delux_reply(&delux_reply(100, 0)),
        Some(DeluxReading { level: 100, charging: false })
    );
}

#[test]
fn delux_wrong_report_id_and_length_are_rejected() {
    let mut wrong = delux_reply(80, 0);
    wrong[0] = 0x0B;
    assert_eq!(parse_delux_reply(&wrong), None);
    assert_eq!(parse_delux_reply(&delux_reply(80, 0)[..32]), None);
    let mut long = delux_reply(80, 0);
    long.push(0);
    assert_eq!(parse_delux_reply(&long), None);
    assert_eq!(parse_delux_reply(&[]), None);
}

#[test]
fn delux_offline_and_out_of_range_are_rejected() {
    assert_eq!(parse_delux_reply(&delux_reply(0, 0)), None);   // 源码 if(buf[18]) 的未就绪
    assert_eq!(parse_delux_reply(&delux_reply(101, 0)), None);
    assert_eq!(parse_delux_reply(&delux_reply(80, 2)), None);
    assert_eq!(parse_delux_reply(&delux_reply(80, 0x65)), None); // 0x65 不是线上充电值
}
```

### 4.5 最小文件修改建议

1. `crates/anybattery-windows/src/hid.rs`
   - 复用 §1.1 的 `feature_exchange`；Delux 的 `build_request` 每次递增 `seq`。
2. `crates/anybattery-windows/src/extra.rs`
   - 新增 `parse_delux_reply`、`read_delux_m800(info)`。
   - `read_vendor_extra()` 中放在 1D57 之后、Razer 之前。
   - 匹配条件严格按 §4.1；如果 `hid::enumerate()` 没有 MI_01/33，直接 `return None`，不要尝试别的接口。
   - `provider: "Delux M800 Pro 33B"`，`verification: Confidence::Community`，`kind: Some(DeviceKind::Mouse)`，`name: Some("Delux M800 Pro")`，`detail: "report 0x0C · seq ... raw[18]=.. raw[19]=.."`。

参考骨架：

```rust
fn read_delux_m800(info: &hid::HidInterface) -> Option<VendorReading> {
    if info.vendor_id != 0x248A
        || !matches!(info.product_id, 0x5B2F | 0x5B2E)
        || info.interface_number != Some(1)
        || info.feature_len != 33
    {
        return None;
    }
    let parsed = hid::feature_exchange(
        &info.path,
        33,
        8,
        Duration::from_millis(30),
        |attempt| {
            let mut request = [0u8; 33];
            request[0] = 0x0C;
            request[1] = 0x01;
            request[2] = 0x20;
            request[4] = (attempt + 1) as u8;
            request
        },
        parse_delux_reply,
    )?;
    Some(reading(
        parsed.level,
        Some(parsed.charging),
        "Delux M800 Pro 33B",
        Confidence::Community,
        Some("Delux M800 Pro".to_owned()),
        Some(DeviceKind::Mouse),
        "delux".to_owned(),
        format!("report 0x0C · raw[18]={} raw[19]={}", parsed.level, parsed.charging as u8),
    ))
}
```
3. `crates/anybattery-windows/src/vendor.rs`
   - 不需要改；`0x248A` 不在任何现有 CompX/Logitech 分支。
4. 测试放 `extra.rs` 现有 test module。
5. 不改 release/README；不加 raw USB 后端。

### 4.6 真机验证步骤（Delux）

1. 插入 `248A:5B2F`（无线）或 `5B2E`（有线），运行现有扫描，**先确认** `hid::enumerate()` 是否列出 MI_01、`feature_len == 33`、report ID `0x0C`。
2. 如果没有 MI_01/33：记录 Windows 设备树/USB descriptor，停止实现；这属于“Windows 无 HID 路径”的未决项，不要用 WinUSB 猜。
3. 如果有：发送 `0C 01 20 00 01 00...`，记录 33 字节回复；确认：
   - `raw[0] == 0x0C`；
   - `raw[18]` 与 Delux 官方软件/ Linux `m800pro-drv charge` 一致；
   - 插线时 `raw[19] == 1`，拔线 0；如果观察到 `0x65`，记录并重新评估（当前严格 parser 会拒绝，不要按报告直接当充电）；
   - 0% / 自动关机时 `raw[18] == 0`，实现返回 `None`，不显示 0%。
4. 重试：拔掉鼠标/让接收器无响应，确认 8 次后返回，不卡扫描；每次 `seq` 递增。
5. 负向：`258A:1001/1002` 不得被本条匹配；其他 MI 不得收到 SET。
6. 通过后再把 `248A:5B2F/5B2E` 从 Community 升级；若 MI_01 不是 HID，则保持“仅文档/Community”，不宣称支持。

### 4.7 来源、commit、永久链接、许可证

| 来源 | commit | 文件/行 | 许可 | 用途 |
|---|---|---|---|---|
| m800-pro-driver | `024b3787450685b7bda6ee329a7e8e5cf6cf2e15` | [`main.c#L9-L12`](https://github.com/xb-bx/m800-pro-driver/blob/024b3787450685b7bda6ee329a7e8e5cf6cf2e15/main.c#L9-L12) | **GPL-2.0**（只取协议事实） | VID/PID、Interface 1 |
| m800-pro-driver | 同上 | [`main.c#L308-L331`](https://github.com/xb-bx/m800-pro-driver/blob/024b3787450685b7bda6ee329a7e8e5cf6cf2e15/main.c#L308-L331) | GPL-2.0 | 33B 请求、seq、raw[18]/[19]、0x65 内部哨兵 |
| m800-pro-driver | 同上 | [`main.c#L370-L390`](https://github.com/xb-bx/m800-pro-driver/blob/024b3787450685b7bda6ee329a7e8e5cf6cf2e15/main.c#L370-L390) | GPL-2.0 | claim Interface 1、charge 输出 |
| m800-pro-driver | 同上 | [`LICENSE#L1-L12`](https://github.com/xb-bx/m800-pro-driver/blob/024b3787450685b7bda6ee329a7e8e5cf6cf2e15/LICENSE#L1-L12) | GPL-2.0 | 许可证核对 |
| FerrisPulse | `4917a10e238ed3dba2938969752886353c8ff5e9` | [`UniversalGamingDeviceHelper.cs#L274-L276`](https://github.com/Ferris-echo/FerrisPulse/blob/4917a10e238ed3dba2938969752886353c8ff5e9/src/Hardware/UniversalGamingDeviceHelper.cs#L274-L276) | GPL-3.0（只取事实） | **反例**：258A:1001/1002 映射，非本协议 |

---

## 5. 报告 `04-oem-mouse.md` 的错误与冲突清单

以下按“报告位置 → 源码事实 → 本文处理”列出，实施时不要照抄报告。

1. **Delux 充电字节 0x65**：报告 `04:157`/表 `:185` 说 `byte[19]` 为 `1 或 0x65` 表示充电。源码 `main.c:308,326-329` 只有 `buf[19] == 1`；`BATTERY_CHARGING 0x65` 是驱动内部返回值（`return BATTERY_CHARGING`），`main` 用它打印 "charging"，不是线上字节。本文 §4.3 只接受 0/1，0x65 拒绝。
2. **Delux 许可证**：报告来源表 `04:500` 写 GPL-3.0；实际 `LICENSE` 第 2 行是 GPL Version 2。本文按 GPL-2.0，只取事实，不复制代码。
3. **Delux VID 冲突**：报告 `04:150`/表 `:185` 用 `248A:5B2F/5B2E`，与 `main.c:9-11` 一致；FerrisPulse `UniversalGamingDeviceHelper.cs:275-276` 却把 Delux M800 Pro/Ultra 写成 `258A:1001/1002`。本文只支持 `248A:5B2F/5B2E`；`258A` 不纳入，避免把另一套设备/协议混进来。
4. **Delux Windows 可行性**：报告说“Verified (C 语言驱动实机)”。源码是 Linux libusb 直接 control transfer + claim Interface 1，不是 Windows HID。本文要求先确认 MI_01 是 `feature_len=33` 的 HID collection；否则不实现写路径。初始等级 Community。
5. **Delux 无限循环**：报告未提 `initial_query` 的 `while(1)`（`main.c:311-331`）。本文改为 8 次有界重试，避免扫描线程卡死。
6. **PixArt 充电/电量偏移歧义**：报告 `04:122-123` 同时给“payload[6]/[7]”和“raw[7]/[8]”。两者是同一布局的 64/65 字节视图；本文统一按 raw 65（`raw[7]` 充电、`raw[8]` 电量），避免实现混用。
7. **PixArt parser 过度宽松**：报告 `04:427-438` 在 `0..=len-8` 全 buffer 搜索 `A1 00 02 02 00 83`。本文要求头固定在 offset 1；否则任意数据都可能被误认。
8. **PixArt A2 被漏掉**：报告只写 `A1`；HaloBattery `wlmouse.py:54`、`mouse-battery-tray/devices.py:94` 接受 `A1|A2`。本文接受 A1/A2，拒绝 A0/A3。
9. **PixArt 0% 与离线**：报告示例 parser 接受 0；FerrisPulse `UniversalGamingDeviceHelper.cs:434` 要求 `pct > 0`。本文接受“头有效 + level 0”，只有全零/无头/无回复算离线。若真机发现 0 是休眠假帧，再收紧。
10. **PixArt 型号过度声明**：报告表 `04:178-182` 把 WLmouse 全系、LAMZU `0001..0005`、Incott `0001/0002` 等都写成 Verified。HaloBattery 只确认 `36A7:A880`、`373E:001E`、`2023:F013`（且 A5 是 issue #149 报告者硬件）；`lamzu.py:16-18` 明确其他 LAMZU 用别的 VID/协议；`mouse-battery-tray/devices.py:62-66` 的 Incott 是 `093A:522C/622C` 的被动 `0x03`，不是 PixArt Feature。本文只纳入 §2.2 白名单，其余 Community only/不做。
11. **PixArt “GetFeature(0)”**：Windows 的 `HidD_GetFeature` 不接受 report id 参数，而是把 report ID 放在 buffer 第 0 字节。报告用 WebHID 语义写 `GetFeature(0)`；实现要按 Windows buffer 语义。
12. **PixArt A3/A0 重试**：报告只说“跳过中间过渡帧”，没有次数/重发规则。本文按 HaloBattery 15×50 ms；A0 跳过、A3 继续 GET（bridge TOML `:22-23` 的 `feature_get`，不重发 SET）。
13. **MCHOSE 长度**：报告 `04:133`/表 `:183` 写短 20 / 长 64；这是 payload。实际 on-wire 是 21/65（HaloBattery `mchose.py:10-11,127-128`；FerrisPulse `MchoseMouseHelper.cs:34-35` 也注明 payload）。本文按 21/65。
14. **MCHOSE 休眠判据**：报告 `04:183` 要求 `payload[9] > 0` 且 `vid != 0`。`level > 0` 是错的：真实 0% 可以合法；休眠帧是 all-zero payload，解码后 `vid == 0`。HaloBattery `parse_status` 允许 level 0；FerrisPulse `ReadIdentity` 也允许 0。本文用 `vid == device_vid && vid in {5253,3837}` 拒绝休眠。
15. **MCHOSE 0xFF0B**：报告说“永远不响应，切勿访问”，正确；但 FerrisPulse `FindControlPath` 把 0xFF0B 当 fallback（`MchoseMouseHelper.cs:317-414`）。本文只认 0xFF01，不照抄 fallback。
16. **MCHOSE 型号名**：报告表写 “A7 V2”；HaloBattery `docs/protocols.md:183-187` 是 **A7 V2 Ultra**（`3837:100B`）。按后者。
17. **MCHOSE PID 泛匹配风险**：报告没有强调 `3837` 还用于 V9 Turbo+ 耳机 `3837:600A`。`battery-hub/src/main/hid/drivers/mchose.js` 证明耳机是 0x55 Input、不是本协议。本文要求 PID 白名单，绝不按 VID 泛写。
18. **MCHOSE A5 Pro Max 归属**：报告把 `2023:F013/F015/F019` 放在 PixArt 表（正确），但 MCHOSE 反码段不要把它纳入；`HaloBattery/mchose.py:54-67` 的 A5 是 0x83 路由，不是 0x06 反码。
19. **MCHOSE 报告未强调重发**：HaloBattery `mchose.py:19-22,253-281` 明确“re-ask rather than re-read”；FerrisPulse `SendCommand` 也是每 attempt 重发。本文把它列为硬要求。
20. **报告来源表许可证**：`04:500` 的 m800-pro-driver 许可证错误（见第 2 条）；其余 HaloBattery/attack bridge/mouse-battery-tray 的 MIT、FerrisPulse GPL-3.0 核对一致。
21. **报告来源表的 battery-hub `mchose.js`**：`04:498` 把它列为“MCHOSE 反码/鼠标”参考，但该文件实际是 V9 Turbo+ 耳机（VID `3837`、PID `600A`）的 Input `0x55` 协议。不要用它推导鼠标反码。
22. **报告 `PixArt` 的 `JM`/`PixArt` 命名**：源码把同族称为 JM_ 固件（HaloBattery `gwolves.py:1-15`），路由名用 “PixArt/JM” 可以，但不要据此声明芯片型号；芯片归属不是本协议匹配条件。
23. **报告来源表的 attackshark-x11-linux commit**：报告 `04:490` 写 `49ecf8e`，本地 checkout 的 HEAD 是 `6622b800a0a0346d950c7b751d7a53cc82fff26b`，且 `49ecf8e` 对象不存在。该仓库只作背景，本文按本地实际 HEAD 记录；不要用旧 commit 做永久链接。
24. **MCHOSE parser 过宽**：HaloBattery `parse_status`（`providers/mchose.py:157-179`）只检查 `r[0]` 是否属于 `{0x11,0x12}`、`r[1]==0xF9`、VID 和 `level<=100`，不要求响应 report ID 与本次请求一致，也不校验 `charge` 只能是 0/1；报告只写“`response[1] == 0x06^0xFF`”。本文要求 `raw[0] == requested_report_id`、`raw[1]==0xF9`、`charge in {0,1}`，否则拒绝。
25. **Delux 行号引用**：报告矩阵/来源表把 M800 Pro 电量握手写成 `main.c:280`（`04:185`/`:500`）；实际 `initial_query()` 在 `main.c:308-331`，280 附近是 `set()` 的尾部。本文以 308-331 为准。

---

## 6. 实施顺序、文件边界与验收命令

### 6.1 建议顺序（单 writer 串行）

1. `hid.rs`：加 §1.1 的 `feature_set_get_on` / `feature_poll` / `feature_exchange`，先编译通过；不接线。
2. `vendor.rs`：删除 `0x5253` 的 CompX 路由（两处），确认其他 CompX PID 不变。
3. `extra.rs`：先加 PixArt parser + tests（纯函数），再接 `read_pixart`；只对白名单写。
4. `extra.rs`：加 MCHOSE parser + tests，再接 `read_mchose_inverse`；短→长、每次重发。
5. `extra.rs`：加 Delux parser + tests，再接 `read_delux_m800`；先只做“有 HID MI_01 才写”的 gate。
6. `scan.rs`/`hid.rs`：可选补 `is_wireless_vid`/`likely_wireless` 的 VID；不影响 parser。
7. 全量验证后再交主代理决定是否升级 Confidence/文档。

### 6.2 文件边界（只允许这些）

- `crates/anybattery-windows/src/hid.rs`（新增 helper；可选 wireless VID）
- `crates/anybattery-windows/src/vendor.rs`（仅删除 0x5253 CompX 两处）
- `crates/anybattery-windows/src/extra.rs`（三条 parser/route/tests）
- `crates/anybattery-windows/src/scan.rs`（可选 wireless 标记；不要动扫描并发/UI）
- 不改 `README.md`、`release/`、`docs/` 其他文件、`Cargo.toml`、`anybattery-core`。

### 6.3 验收命令

```bash
git diff --check
cargo test -p anybattery-core
cargo xwin check -p anybattery-app --target x86_64-pc-windows-msvc --all-targets
cargo xwin test -p anybattery-windows --target x86_64-pc-windows-msvc --no-run
# 运行生成的 test exe（路径以实际为准）
ls target/x86_64-pc-windows-msvc/debug/deps/anybattery_windows-*.exe
```

验收门：
- 纯 parser fixtures 全过；
- `0x5253` 不再进入 CompX 17B（诊断/时序验证）；
- 未匹配设备没有任何 SET（负向测试/日志）；
- 扫描耗时有界；
- 真机验证前，所有新路由仍是 `Confidence::Community`。

### 6.4 真机验证记录模板

每个型号记录：
```
型号 / VID:PID / 连接方式 / interface / usage_page:usage / feature_len
请求 hex（含 report ID）
原始回复 hex（前 3 次 + 稳定帧）
官方软件读数 / 本程序读数
充电插拔前后 raw 充电位
休眠/关机时回复（是否全零/无回复）
负向：同 VID 其他 PID 是否被写入（应为否）
结论：Community / Verified / 不支持
```

---

## 7. 型号支持等级总表

| 协议 | 型号 / VID:PID | 上游证据 | 本批是否实现 | 初始等级 | 备注 |
|---|---|---|---|---|---|
| PixArt | WLmouse Beast X Max `36A7:A880` | HaloBattery 实机确认 | 是 | Community | 真机后可升 |
| PixArt | WLmouse Beast X `36A7:A887` | 接收器表，未确认 | 是 | Community only | 不升 Verified |
| PixArt | WLmouse Beast X Mini Pro `36A7:A868` | 接收器表，未确认 | 是 | Community only | 不升 Verified |
| PixArt | LAMZU Maya X 8K `373E:001E` | HaloBattery 实机确认 | 是 | Community | 真机后可升 |
| PixArt | LAMZU Maya X cable `373E:001C` | 源码 | 是 | Community | 与 dongle 同协议 |
| PixArt | Attack Shark R5 Ultra `373E:0046/0047` | bridge profile | 是 | Community only | TOML 非硬件验证 |
| PixArt | MCHOSE A5 Pro Max `2023:F013` | issue #149 硬件确认 | 是 | Community | 真机后可升 |
| PixArt | MCHOSE A5 Pro Max `2023:F015/F019` | 同族，未试 | 是 | Community only | 只读抓包后评估 |
| PixArt | LAMZU `373E:0001..0005` | FerrisPulse 产品表，无协议实测 | **否** | Community only | 报告过度声明 |
| PixArt | Incott `093A:0001/0002` | FerrisPulse 产品表 | **否** | Community only | 非本 0x83 证据 |
| PixArt | Incott `093A:522C/622C` | mouse-battery-tray 被动 0x03 | **否** | Community only | 另做被动路由 |
| PixArt | G-Wolves `33E4:*` | HaloBattery 未验证 | **否** | Community only | 后续任务 |
| MCHOSE | M7 Ultra `5253:1020` | HaloBattery 固件 5.2.7.0 实测 | 是 | Community | 真机后可升 |
| MCHOSE | M7 Ultra USB `5253:0031` | HaloBattery 实测 | 是 | Community | 与 dongle 同协议 |
| MCHOSE | A5 V2 Ultra `3837:100C` | FerrisPulse 常量 | 是 | Community only | 未实测 |
| MCHOSE | A5 V2 Ultra USB `3837:1101` | FerrisPulse 常量 | 是 | Community only | 未实测 |
| MCHOSE | A7 V2 Ultra `3837:100B` | HaloBattery issue #4 | 是 | Community only | 未实测 |
| MCHOSE | 其他 `3837`（含 `600A` 耳机） | 不同协议 | **否** | 不支持 | 绝不泛写 |
| MCHOSE | A5 Pro Max `2023:F0xx` | PixArt 0x83 | 走 PixArt | Community | 不进反码路由 |
| MCHOSE | G7 `A8A5:2255` | Output 65B/Input AA30 | **否** | 不支持 | 另做 |
| Delux | M800 Pro `248A:5B2F` | Linux libusb 实机 | 是（有 HID gate） | Community only | Windows HID 未证实 |
| Delux | M800 Pro wired `248A:5B2E` | 同上 | 是（有 HID gate） | Community only | 同上 |
| Delux | `258A:1001/1002` | FerrisPulse 映射，无本协议证据 | **否** | 不支持 | 不要混入 |

---

## 8. 参考来源总表（实际核对）

| # | 仓库 | 实际 commit | 许可证 | 核对文件 |
|---|---|---|---|---|
| 1 | https://github.com/HeyOkay/HaloBattery | `2251f0534a8e886c9692ead42056059b4f69d8c0` | MIT | `providers/wlmouse.py`, `providers/lamzu.py`, `providers/gwolves.py`, `providers/mchose.py`, `docs/protocols.md`, `tests/test_wlmouse.py`, `tests/test_lamzu.py`, `tests/test_mchose.py` |
| 2 | https://github.com/Ferris-echo/FerrisPulse | `4917a10e238ed3dba2938969752886353c8ff5e9` | GPL-3.0（只取事实） | `src/Hardware/UniversalGamingDeviceHelper.cs`, `src/Hardware/MchoseMouseHelper.cs` |
| 3 | https://github.com/maxboeer/attackshark-battery-bridge | `2d77aeb5cd8a9fa3408416200cc86de61221f145` | MIT | `src/attackshark_battery_bridge/profiles/attack_shark_r5_ultra.toml`, `drivers/profile_driver.py`, `transport/hidraw.py`, `tests/test_driver.py`, `tests/test_profiles.py` |
| 4 | https://github.com/incconutwo/mouse-battery-tray | `c1c6059472dab366c700a7f7516e0dbffde68fd7` | MIT | `devices.py`, `battery_tray.pyw`, `CHANGELOG.md` |
| 5 | https://github.com/xb-bx/m800-pro-driver | `024b3787450685b7bda6ee329a7e8e5cf6cf2e15` | **GPL-2.0** | `main.c`, `LICENSE`, `README.md` |
| 6 | https://github.com/DORON177/battery-hub | `fb8abfbe97d1e284ae2206547cfd3daaa5d98a91` | MIT | `src/main/hid/drivers/mchose.js`（耳机反例）, `kysona.js` |
| 7 | https://github.com/MiguelSanzBr/attackshark-x11-linux | `6622b800a0a0346d950c7b751d7a53cc82fff26b` | MIT | `docs/PROTOCOL.md`（仅背景，不用于本三条） |
| 8 | https://github.com/yszyszysz233/2.4G-battery-viewer | `f0e983e88900592237c7df9ae6b80b9dbeb3020b` | 未注明（All Rights Reserved） | `battery.py`（仅背景，不复制） |

---

## 9. 未决项与风险

1. **Delux Windows HID 路径未证实**：这是最大阻塞。若 MI_01 不是 HID，本批只能文档化，不能写设备；不要临时扩 WinUSB 后端。
2. **MCHOSE 3837 跨 VID 是否一致**：`parse_mchose_reply` 严格要求 `payload_vid == device_vid`。如果 A7 V2 Ultra 接收器（3837）回包里 VID 仍是 5253，严格 parser 会拒绝；需真机抓包后决定是否允许 `{5253,3837}` 集合。当前按最保守实现。
3. **PixArt A2**：接受 A2 来自 HaloBattery/mouse-battery-tray，但 Attack Shark bridge 只写 A1；如果 R5 Ultra 的 A2 是别的含义，需在 R5 上单独收紧。
4. **PixArt 0%**：接受 level 0 可能把某些休眠假帧当 0%；需要真机休眠测试确认。若确认，改成“0 且无其他有效字段”拒绝。
5. **Delux 0x65**：当前严格拒绝；如果真机抓到 0x65 且官方软件确实显示充电，需要重新评估（可能源码之外的固件版本）。
6. **图标/缓存**：三条协议都只返回真实读数；上层是否灰显/缓存/合并 dongle 与有线图标不在本批范围。不要为了“一个图标”在 parser 层伪造。
7. **并发单 writer**：`vendor.rs` 与 Wave B2 可能冲突；只删 0x5253 两处，其他不动。`extra.rs` 与 Wave B1 也可能冲突；按函数块插入，不重排现有代码。
8. **许可证**：FerrisPulse GPL-3.0、m800-pro-driver GPL-2.0，只可取协议事实，不复制实现/注释/常量表结构；HaloBattery/attack bridge/mouse-battery-tray MIT，仍建议独立实现。
