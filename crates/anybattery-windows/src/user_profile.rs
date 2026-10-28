//! 用户自定义 profile 的 Windows 侧绑定：接口指纹、被动读数、快照。
//!
//! 这里只做三件事：把 `HidInterface` 映射成核心库的 `InterfaceBinding`、
//! 在只读句柄上被动等一份报文、把读到的电量包装成未验证快照。
//! 不构造 0%，不推断充电，也不接触文件或 UI。

use std::time::{Duration, Instant};

use anybattery_core::{
    BatteryReading, BatterySource, Confidence, DeviceId, DeviceKind, DeviceSnapshot,
    InterfaceBinding, UserProfileCatalog, UserProfileError, UserProtocolProfile,
};

use crate::hid::{self, HidInterface};
use crate::passive::{PassiveCaptureError, PassiveRead};

impl TryFrom<&HidInterface> for InterfaceBinding {
    type Error = UserProfileError;

    fn try_from(info: &HidInterface) -> Result<Self, Self::Error> {
        let container_id = info
            .container_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or(UserProfileError::MissingContainerId)?;
        let instance_id = info
            .instance_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or(UserProfileError::MissingInstanceId)?;
        let interface_number = info
            .interface_number
            .ok_or(UserProfileError::MissingInterfaceNumber)?;
        if info.input_len == 0 {
            return Err(UserProfileError::EmptyInputLength);
        }
        Ok(Self {
            container_id: container_id.to_owned(),
            instance_id: instance_id.to_owned(),
            interface_number: Some(interface_number),
            usage_page: info.usage_page,
            usage: info.usage,
            vendor_id: info.vendor_id,
            product_id: info.product_id,
            input_len: info.input_len,
        })
    }
}

/// 单个接口的绑定指纹；缺少身份信息时返回错误，绝不"猜"一个绑定。
pub fn binding_for(info: &HidInterface) -> Result<InterfaceBinding, UserProfileError> {
    InterfaceBinding::try_from(info)
}

/// 在配置集合里为接口找唯一启用的 profile。
pub fn find_profile<'a>(
    catalog: &'a UserProfileCatalog,
    info: &HidInterface,
) -> Result<Option<&'a UserProtocolProfile>, UserProfileError> {
    let binding = binding_for(info)?;
    catalog.find_for_interface(&binding)
}

/// 在 `timeout` 内被动等待一份能解析出电量的报文。
///
/// 命中即返回；超时或只有无关报文时返回 `Ok(None)`，绝不退化成 0%。
pub fn read_user_level_with<R: PassiveRead>(
    reader: &mut R,
    profile: &UserProtocolProfile,
    timeout: Duration,
) -> Result<Option<u8>, PassiveCaptureError> {
    let input_len = usize::from(profile.binding.input_len);
    if input_len == 0 {
        return Err(PassiveCaptureError::ReadFailed);
    }
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(None);
        }
        let slice_ms = crate::passive::slice_millis(remaining);
        match reader.read(input_len, slice_ms) {
            Some(report) => {
                if report.len() != input_len {
                    continue;
                }
                match profile.extract_level(&report) {
                    Ok(Some(level)) => return Ok(Some(level)),
                    // 报文不匹配或取值超范围：继续等下一份，不当作 0%。
                    Ok(None) => {}
                    Err(_) => {}
                }
            }
            // 真实 HID 读取会占满整个切片；脚本化来源会立刻返回 None，
            // 退让 1ms 以免在等窗口时空转。
            None => std::thread::sleep(Duration::from_millis(1)),
        }
    }
}

/// 把用户配置读到的电量包装成快照：来源固定为"未验证"，不伪造充电状态。
pub fn snapshot_from_user(
    info: &HidInterface,
    profile: &UserProtocolProfile,
    level: u8,
    now: u64,
) -> DeviceSnapshot {
    let detail = format!("{} · 被动输入", profile.spec.name);
    let id = info
        .container_id
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            format!(
                "{:04X}:{:04X}:{}",
                info.vendor_id,
                info.product_id,
                hid::short_hash(&info.path)
            )
        });
    let name = if info.product.trim().is_empty() {
        profile.spec.name.clone()
    } else {
        info.product.clone()
    };
    DeviceSnapshot::new(
        DeviceId::new("hid", &id),
        name.clone(),
        DeviceKind::infer_from_name(&name),
        hid::transport_for_product(&info.path, info.vendor_id, info.product_id),
        BatterySource::new("用户配置", Confidence::Unverified, detail.clone()),
    )
    .with_battery(BatteryReading::percent(level, None), now)
    .with_detail(detail)
    .with_path(&info.path)
}

/// 测试与 scan 共用的构造器：接口、绑定、profile。
#[cfg(test)]
pub(crate) mod fixtures {
    use super::*;
    use anybattery_core::protocol::{
        Extract, Field, FieldType, MatchByte, Matcher, ProtocolSpec, ProtocolVerification, Request,
        RequestKind, Response, ResponseKind,
    };

    pub(crate) const INPUT_LEN: u16 = 16;

    pub(crate) fn interface() -> HidInterface {
        HidInterface {
            path: r"\\?\hid#vid_1234&pid_5678&mi_02#7&1a2b3c&0&0000#{4d1e55b2-f16f-11cf-88cb-001111000030}"
                .to_owned(),
            vendor_id: 0x1234,
            product_id: 0x5678,
            version: 0x0100,
            product: "示例接收器".to_owned(),
            manufacturer: "示例厂商".to_owned(),
            usage_page: 0xFF05,
            usage: 0x0001,
            input_len: INPUT_LEN,
            output_len: 0,
            feature_len: 0,
            interface_number: Some(2),
            container_id: Some("container-a".to_owned()),
            instance_id: Some("instance-a".to_owned()),
        }
    }

    pub(crate) fn profile() -> UserProtocolProfile {
        let info = interface();
        let binding = binding_for(&info).expect("fixture interface has a full identity");
        UserProtocolProfile {
            schema: anybattery_core::user_protocol::USER_PROFILE_SCHEMA,
            enabled: true,
            binding,
            spec: ProtocolSpec {
                schema: 1,
                id: "user.test.receiver".to_owned(),
                name: "用户配置：示例接收器".to_owned(),
                matcher: Matcher {
                    vid: Some("1234".to_owned()),
                    pid: Some("5678".to_owned()),
                    usage_page: Some("FF05".to_owned()),
                    usage: Some("0001".to_owned()),
                    min_output_len: None,
                },
                request: Request {
                    kind: RequestKind::None,
                    report_id: None,
                    length: None,
                    payload_hex: String::new(),
                    pad: None,
                },
                response: Response {
                    kind: ResponseKind::Input,
                    timeout_ms: 300,
                    retries: 0,
                    match_bytes: vec![MatchByte(1, "72".to_owned())],
                },
                extract: Extract {
                    level: Field {
                        offset: 3,
                        value_type: FieldType::U8,
                        scale: 1.0,
                        bias: 0.0,
                        min: Some(0.0),
                        max: Some(100.0),
                    },
                    charging: None,
                    voltage_mv: None,
                },
                verification: ProtocolVerification::Unverified,
                notes: None,
            },
        }
    }

    pub(crate) fn catalog() -> UserProfileCatalog {
        UserProfileCatalog::new(vec![profile()]).expect("fixture profile is valid")
    }

    pub(crate) fn report(level: u8) -> Vec<u8> {
        let mut bytes = vec![0u8; usize::from(INPUT_LEN)];
        bytes[1] = 0x72;
        bytes[3] = level;
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::{INPUT_LEN, catalog, interface, profile, report};
    use super::*;
    use anybattery_core::UserProfileCatalog;

    struct ScriptedReader {
        queue: Vec<Option<Vec<u8>>>,
        writes: usize,
    }

    impl ScriptedReader {
        fn new(queue: Vec<Option<Vec<u8>>>) -> Self {
            Self { queue, writes: 0 }
        }

        fn silent() -> Self {
            Self::new(Vec::new())
        }
    }

    impl PassiveRead for ScriptedReader {
        fn read(&mut self, _input_len: usize, _timeout_ms: u32) -> Option<Vec<u8>> {
            if self.queue.is_empty() {
                return None;
            }
            self.queue.remove(0)
        }
    }

    #[test]
    fn binding_maps_interface_identity() {
        let info = interface();
        let binding = binding_for(&info).expect("full identity");
        assert_eq!(binding.container_id, "container-a");
        assert_eq!(binding.instance_id, "instance-a");
        assert_eq!(binding.interface_number, Some(2));
        assert_eq!(binding.usage_page, 0xFF05);
        assert_eq!(binding.usage, 0x0001);
        assert_eq!(binding.vendor_id, 0x1234);
        assert_eq!(binding.product_id, 0x5678);
        assert_eq!(binding.input_len, INPUT_LEN);
    }

    #[test]
    fn binding_rejects_missing_container_id() {
        let mut info = interface();
        info.container_id = None;
        assert_eq!(
            binding_for(&info),
            Err(UserProfileError::MissingContainerId)
        );
        info.container_id = Some("   ".to_owned());
        assert_eq!(
            binding_for(&info),
            Err(UserProfileError::MissingContainerId)
        );
    }

    #[test]
    fn binding_rejects_missing_instance_id() {
        let mut info = interface();
        info.instance_id = Some(String::new());
        assert_eq!(binding_for(&info), Err(UserProfileError::MissingInstanceId));
    }

    #[test]
    fn binding_rejects_missing_interface_number() {
        let mut info = interface();
        info.interface_number = None;
        assert_eq!(
            binding_for(&info),
            Err(UserProfileError::MissingInterfaceNumber)
        );
    }

    #[test]
    fn binding_rejects_empty_input_length() {
        let mut info = interface();
        info.input_len = 0;
        assert_eq!(binding_for(&info), Err(UserProfileError::EmptyInputLength));
    }

    #[test]
    fn find_profile_matches_unique_binding() {
        let catalog = catalog();
        let found = find_profile(&catalog, &interface()).expect("binding ok");
        assert_eq!(
            found.map(|p| p.spec.id.as_str()),
            Some("user.test.receiver")
        );
    }

    #[test]
    fn find_profile_returns_none_without_match() {
        let catalog = catalog();
        let mut info = interface();
        info.product_id = 0x9999;
        assert!(find_profile(&catalog, &info).expect("binding ok").is_none());
    }

    #[test]
    fn find_profile_reports_missing_identity() {
        let catalog = catalog();
        let mut info = interface();
        info.container_id = None;
        assert_eq!(
            find_profile(&catalog, &info),
            Err(UserProfileError::MissingContainerId)
        );
    }

    #[test]
    fn find_profile_ignores_disabled_profiles() {
        let mut profile = profile();
        profile.enabled = false;
        let catalog = UserProfileCatalog::new(vec![profile]).expect("valid");
        assert!(
            find_profile(&catalog, &interface())
                .expect("binding ok")
                .is_none()
        );
    }

    #[test]
    fn read_user_level_returns_profile_level() {
        let profile = profile();
        let mut reader = ScriptedReader::new(vec![Some(report(64))]);
        let level = read_user_level_with(&mut reader, &profile, Duration::from_millis(50))
            .expect("no read error");
        assert_eq!(level, Some(64));
        assert_eq!(reader.writes, 0);
    }

    #[test]
    fn read_user_level_skips_unrelated_reports() {
        let profile = profile();
        let mut other = report(90);
        other[1] = 0x00;
        let mut reader = ScriptedReader::new(vec![Some(other), Some(report(21))]);
        let level = read_user_level_with(&mut reader, &profile, Duration::from_millis(50))
            .expect("no read error");
        assert_eq!(level, Some(21));
        assert_eq!(reader.writes, 0);
    }

    #[test]
    fn read_user_level_returns_none_on_bad_header() {
        let profile = profile();
        let mut other = report(42);
        other[1] = 0x99;
        let mut reader = ScriptedReader::new(vec![Some(other)]);
        let level = read_user_level_with(&mut reader, &profile, Duration::from_millis(30))
            .expect("no read error");
        assert_eq!(level, None);
        assert_eq!(reader.writes, 0);
    }

    #[test]
    fn read_user_level_discards_short_reports() {
        let profile = profile();
        let mut reader = ScriptedReader::new(vec![Some(vec![0x00, 0x72])]);
        let level = read_user_level_with(&mut reader, &profile, Duration::from_millis(30))
            .expect("no read error");
        assert_eq!(level, None);
        assert_eq!(reader.writes, 0);
    }

    #[test]
    fn read_user_level_times_out_without_reports() {
        let profile = profile();
        let mut reader = ScriptedReader::silent();
        let level = read_user_level_with(&mut reader, &profile, Duration::from_millis(20))
            .expect("no read error");
        assert_eq!(level, None);
        assert_eq!(reader.writes, 0);
    }

    #[test]
    fn read_user_level_rejects_empty_input_length() {
        let mut profile = profile();
        profile.binding.input_len = 0;
        let mut reader = ScriptedReader::silent();
        assert_eq!(
            read_user_level_with(&mut reader, &profile, Duration::from_millis(20)),
            Err(PassiveCaptureError::ReadFailed)
        );
        assert_eq!(reader.writes, 0);
    }

    #[test]
    fn user_snapshot_is_unverified() {
        let info = interface();
        let profile = profile();
        let snapshot = snapshot_from_user(&info, &profile, 55, 1_700_000_000);
        assert_eq!(snapshot.percent(), Some(55));
        assert_eq!(snapshot.source.provider, "用户配置");
        assert_eq!(snapshot.source.verification, Confidence::Unverified);
        assert!(snapshot.source.detail.contains("被动输入"));
        assert_eq!(snapshot.detail, snapshot.source.detail);
        assert_eq!(snapshot.battery.charging(), None);
        assert_eq!(snapshot.status, anybattery_core::BatteryStatus::Available);
        assert_eq!(snapshot.name, "示例接收器");
        assert_eq!(snapshot.id.0, "hid:container-a");
        assert_eq!(snapshot.path.as_deref(), Some(info.path.as_str()));
    }

    #[test]
    fn user_snapshot_falls_back_to_profile_name() {
        let mut info = interface();
        info.product = String::new();
        let snapshot = snapshot_from_user(&info, &profile(), 12, 1);
        assert_eq!(snapshot.name, "用户配置：示例接收器");
    }
}
