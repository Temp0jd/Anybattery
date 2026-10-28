# 2.4G 手动适配方案（草案）

日期：2026-10-28。状态：**MVP 已实现**（core + windows + app 向导），仍需 Windows 真机验收。

## 目标

当用户的 2.4G 接收器或设备不在内置 VID:PID/协议表中时，允许用户通过一个**只读、可预览、可撤销**的向导，把该接口的电量字段映射成一张用户配置卡，标记为“用户配置 / 未验证”。

## 非目标（MVP 明确不做）

- 不执行任意脚本，不允许用户提交 Rust/JS 代码。
- 不默认发送 Output 或 Feature Set 报文。现有内置适配器可以主动查询，但用户向导的 MVP 只做被动监听。
- 不自动猜测充电位、电压曲线、CRC/TLV、动态序号或多报告状态机。
- 不把用户配置自动上传、自动发布或声称“已验证”。
- 不把同一 VID:PID 的配置无条件套到其他设备上。

## 复用现有能力

anybattery-core/src/protocol.rs 已有：

- ProtocolSpec schema 1；
- Matcher、Request、Response、MatchByte；
- Field（offset/type/scale/min/max）与 Extract；
- 十六进制、长度、偏移、超时、重试、范围校验；
- 单元测试。

MVP 只接受安全子集：

~~~text
request.kind  = none
response.kind = input
verification  = unverified（保存时强制覆盖）
~~~

新增用户配置外壳，不把用户状态塞进内置协议：

~~~rust
struct UserProtocolProfile {
    schema: u32,
    id: String,
    enabled: bool,
    origin: UserProvided,
    interface_binding: InterfaceBinding,
    spec: ProtocolSpec,
}
~~~

现有 Field 只有乘法，没有加法校准项。若要支持两点校准，向后兼容地增加默认 0 的 bias：

~~~text
level = raw * scale + bias
~~~

两个不同原始值才能推导：

~~~text
scale = (shown2 - shown1) / (raw2 - raw1)
bias  = shown1 - scale * raw1
~~~

raw1 == raw2、样本不足两点、非有限 scale 时拒绝推导。

## 用户流程

1. **选择接口**
   列出 Product、VID:PID、MI、usage page/usage、输入长度、ContainerId、当前路径哈希。不要只显示同型号名称。

2. **暂停该接口的正常扫描并开始被动捕获**
   3–5 秒 overlapped input 监听，提示“请动一下设备或按一下按键”。不发 Output、不发 Feature Set。

3. **记录样本**
   用户输入厂商软件同期显示的电量；至少两个不同电量/原始值样本。允许手动修正样本。

4. **选择字段**
   可以列出发生变化的字节候选，但不得自动把“像 0–100 的字节”认定为电量。

5. **确认匹配字节**
   要求非空 report/header 匹配和合理报文长度；所有样本必须满足。

6. **预览**
   用新捕获报告实时显示推导结果、原始值和公式；超范围、短报文、匹配失败均不显示电量。

7. **保存并启用**
   强制标记“用户配置 / 未验证”，忽略导入 JSON 中自报的 verified。首次保存后仍需一次真实扫描确认。

## 绑定优先级

~~~text
ContainerId + InstanceId + interface number + usage page/usage
+ VID:PID + report length
~~~

规则：

- 不能只按 VID:PID 自动应用；
- 重新枚举后必须得到**唯一**匹配接口；
- 匹配到 0 个或多个候选时禁用并提示重新绑定；
- 两台同 VID:PID 设备不得共享实例绑定；
- 缺少稳定身份时只允许本次会话使用，或要求下次启动重新确认。

## 存储

- 用户配置目录：%APPDATA%/AnyBattery/protocols/*.json；
- 便携模式：exe 目录/data/protocols；
- 原子写入；损坏文件保留为 .broken-<unix> 并在诊断页报告；
- 导入默认禁用，必须重新绑定接口并预览成功；
- 导出包含 schema、匹配、校准和说明，默认不包含原始捕获；
- 删除只删除用户文件，不影响内置适配器。

## 安全边界

- 被动捕获阶段断言 write_count == 0；
- 请求模式（Feature Set/Output）作为**单独的高级功能**，需要显式风险提示，不能混在只读向导中；
- 用户配置只影响匹配到的唯一接口；扫描器在内置适配器之后尝试用户配置；
- 用户配置失败时只显示“用户配置读取失败”，不生成虚假电量；
- 不把用户配置计入“已验证”或低电量通知的 live 数据，除非用户显式启用并成功读取。

## 测试与 fixtures

- 两点正向/反向缩放、bias、四舍五入、超范围；
- 相同 raw、不足两点、非有限 scale；
- 空 match_bytes、错误 usage、短报文、偏移越界；
- 同 VID:PID、不同 ContainerId 不串用；
- 多候选/无候选时禁用；
- 禁用和删除后零 I/O；
- fake transport 端到端断言 write_count == 0；
- 导入 verified 被强制降为未验证；
- 捕获超时、取消和厂商软件同时运行；
- 随机截断/畸形报告不 panic。

## 实施顺序（建议）

1. core：用户配置 schema、两点校准、校验与单元测试；
2. windows：只读捕获器与 fake transport 测试；
3. app：向导 UI、预览、保存/禁用/删除；
4. 导入/导出与诊断页入口；
5. 真机验证：至少一个未知 2.4G 接收器和一个已知设备的回归。

## 实现映射（MVP）

- core：`crates/anybattery-core/src/protocol.rs` 增加 `bias`（旧 JSON 默认 0，允许负 scale）；`crates/anybattery-core/src/user_protocol.rs` 提供 `InterfaceBinding`、`UserProtocolProfile`、两点校准、唯一匹配与安全子集校验。
- windows：`passive.rs` 只读捕获；`user_profile.rs` 接口绑定、被动读数和 Unverified 快照；`scan.rs` 在内置 vendor 与标准 HID 之后才尝试用户 profile。
- app：`protocol_wizard.rs` 提供选择接口、4 秒被动捕获、样本、offset/匹配头、预览和保存；`storage.rs` 提供 `protocols/*.json`、损坏隔离、导入强制禁用/未验证。
- 测试：core 35、windows 209、app 50（GNU + wine）。仍需真机验证：只读句柄与厂商软件共存、真实 input 报文、取消/超时/拔插、高报告率、两台同 VID:PID 不串用。

## 与后续迭代的关系

MVP 已把身份、失败语义和只读边界固定下来。下一步是 Windows 真机验收，再决定是否增加导入/导出界面、per-device 调度和缓存；在真机证据前不宣称兼容。
