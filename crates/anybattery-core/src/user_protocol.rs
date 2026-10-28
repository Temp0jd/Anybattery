//! 用户自定义 2.4G 协议 profile。
//!
//! 用户 profile 只能表达「被动监听 + 单字节电量」这一安全子集：
//! 不发请求报文、不读 Feature、不推断充电/电压，并且必须绑定到唯一接口。
//! 内置协议仍由 `protocol::ProtocolSpec` 表达，用户状态不写进内置协议。

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::protocol::{
    FieldType, MatchByte, ProtocolError, ProtocolSpec, ProtocolVerification, RequestKind,
    ResponseKind, hex_to_bytes,
};

/// 用户 profile 外壳 schema，与 `ProtocolSpec::schema` 独立。
pub const USER_PROFILE_SCHEMA: u32 = 1;

/// 用户 profile 绑定的具体 USB/HID 接口。
///
/// 身份按 `container_id` + `instance_id` + `interface_number` + usage + VID:PID 判等；
/// `input_len` 是读取时的长度约束，不参与身份判等。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterfaceBinding {
    pub container_id: String,
    pub instance_id: String,
    pub interface_number: Option<u16>,
    pub usage_page: u16,
    pub usage: u16,
    pub vendor_id: u16,
    pub product_id: u16,
    pub input_len: u16,
}

impl InterfaceBinding {
    /// 两个绑定是否指向同一个接口。
    pub fn same_interface(&self, other: &Self) -> bool {
        // Windows ContainerId and instance IDs are case-insensitive; report
        // length is part of the interface fingerprint, so a profile for a
        // 64-byte collection cannot bind to a 32-byte one.
        self.container_id.eq_ignore_ascii_case(&other.container_id)
            && self.instance_id.eq_ignore_ascii_case(&other.instance_id)
            && self.interface_number == other.interface_number
            && self.usage_page == other.usage_page
            && self.usage == other.usage
            && self.vendor_id == other.vendor_id
            && self.product_id == other.product_id
            && self.input_len == other.input_len
    }

    /// 绑定必须携带完整、可重新枚举的接口身份。
    pub fn validate(&self) -> Result<(), UserProfileError> {
        if self.container_id.trim().is_empty() {
            return Err(UserProfileError::MissingContainerId);
        }
        if self.instance_id.trim().is_empty() {
            return Err(UserProfileError::MissingInstanceId);
        }
        if self.interface_number.is_none() {
            return Err(UserProfileError::MissingInterfaceNumber);
        }
        if self.input_len == 0 {
            return Err(UserProfileError::EmptyInputLength);
        }
        Ok(())
    }
}

/// 一张用户配置卡：外壳 + 受限 `ProtocolSpec`。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UserProtocolProfile {
    pub schema: u32,
    pub enabled: bool,
    pub binding: InterfaceBinding,
    pub spec: ProtocolSpec,
}

/// 两点校准所需的人工样本。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CalibrationSample {
    pub raw: u8,
    pub shown_percent: u8,
}

/// 由两个样本推导出的 `level = raw * scale + bias`。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Calibration {
    pub scale: f32,
    pub bias: f32,
}

impl Calibration {
    /// 未校准：`level = raw`。
    pub const IDENTITY: Self = Self {
        scale: 1.0,
        bias: 0.0,
    };

    /// 把原始字节套用校准公式（不截断、不取整）。
    pub fn apply(self, raw: u8) -> f32 {
        f32::from(raw) * self.scale + self.bias
    }

    /// 两点校准。允许负 scale（反向校准），拒绝相同 raw 与零/非有限 scale。
    pub fn two_point(a: CalibrationSample, b: CalibrationSample) -> Result<Self, CalibrationError> {
        if a.raw == b.raw {
            return Err(CalibrationError::SameRaw);
        }
        let scale = (f32::from(b.shown_percent) - f32::from(a.shown_percent))
            / (f32::from(b.raw) - f32::from(a.raw));
        let bias = f32::from(a.shown_percent) - scale * f32::from(a.raw);
        if !scale.is_finite() || scale == 0.0 || !bias.is_finite() {
            return Err(CalibrationError::ZeroOrInvalidScale);
        }
        Ok(Self { scale, bias })
    }
}

#[derive(Debug, Error, PartialEq)]
pub enum CalibrationError {
    #[error("两点校准需要两个不同的原始值")]
    SameRaw,
    #[error("校准缩放必须为有限且非零数")]
    ZeroOrInvalidScale,
}

#[derive(Debug, Error, PartialEq)]
pub enum UserProfileError {
    #[error("JSON 解析失败: {0}")]
    Json(String),
    #[error("协议校验失败: {0}")]
    Protocol(#[from] ProtocolError),
    #[error("不支持的用户配置 schema: {0}")]
    Schema(u32),
    #[error("用户配置必须标记为未验证")]
    Verification,
    #[error("用户配置只能被动监听，请求类型必须为 none")]
    RequestKind,
    #[error("用户配置不允许发送报文")]
    RequestPayload,
    #[error("用户配置响应类型必须为 input")]
    ResponseKind,
    #[error("用户配置不允许重试")]
    Retries,
    #[error("用户配置必须包含非空匹配头")]
    EmptyMatchHeader,
    #[error("电量字段类型必须为 u8")]
    LevelType,
    #[error("用户配置不支持充电字段")]
    ChargingUnsupported,
    #[error("用户配置不支持电压字段")]
    VoltageUnsupported,
    #[error("电量字段范围必须为 0..=100")]
    LevelBounds,
    #[error("接口绑定缺少容器标识")]
    MissingContainerId,
    #[error("接口绑定缺少实例标识")]
    MissingInstanceId,
    #[error("接口绑定缺少接口号")]
    MissingInterfaceNumber,
    #[error("接口绑定输入长度必须大于 0")]
    EmptyInputLength,
    #[error("匹配器与接口绑定不一致")]
    MatcherBindingMismatch,
    #[error("用户配置匹配器不允许 min_output_len")]
    MatcherOutputLength,
    #[error("字段越界: offset={offset}, len={len}, input_len={input_len}")]
    OutOfRange {
        offset: usize,
        len: usize,
        input_len: usize,
    },
    #[error("多个启用配置绑定同一接口")]
    AmbiguousBinding,
}

impl UserProtocolProfile {
    /// 解析并校验用户 profile JSON（安全子集不合格即拒绝）。
    pub fn from_json(text: &str) -> Result<Self, UserProfileError> {
        let profile: Self =
            serde_json::from_str(text).map_err(|e| UserProfileError::Json(e.to_string()))?;
        profile.validate()?;
        Ok(profile)
    }

    /// 强制用户 profile 落在只读安全子集内。
    pub fn validate(&self) -> Result<(), UserProfileError> {
        if self.schema != USER_PROFILE_SCHEMA {
            return Err(UserProfileError::Schema(self.schema));
        }
        self.spec.validate()?;
        if self.spec.verification != ProtocolVerification::Unverified {
            return Err(UserProfileError::Verification);
        }
        if self.spec.request.kind != RequestKind::None {
            return Err(UserProfileError::RequestKind);
        }
        if !self.spec.request.payload_hex.trim().is_empty()
            || self.spec.request.report_id.is_some()
            || self.spec.request.length.is_some()
            || self.spec.request.pad.is_some()
        {
            return Err(UserProfileError::RequestPayload);
        }
        if self.spec.response.kind != ResponseKind::Input {
            return Err(UserProfileError::ResponseKind);
        }
        if self.spec.response.retries != 0 {
            return Err(UserProfileError::Retries);
        }
        if self.spec.response.match_bytes.is_empty() {
            return Err(UserProfileError::EmptyMatchHeader);
        }

        let level = &self.spec.extract.level;
        if level.value_type != FieldType::U8 {
            return Err(UserProfileError::LevelType);
        }
        if self.spec.extract.charging.is_some() {
            return Err(UserProfileError::ChargingUnsupported);
        }
        if self.spec.extract.voltage_mv.is_some() {
            return Err(UserProfileError::VoltageUnsupported);
        }
        if level.min != Some(0.0) || level.max != Some(100.0) {
            return Err(UserProfileError::LevelBounds);
        }

        self.binding.validate()?;
        self.validate_matcher()?;

        let input_len = usize::from(self.binding.input_len);
        if level.offset + 1 > input_len {
            return Err(UserProfileError::OutOfRange {
                offset: level.offset,
                len: 1,
                input_len,
            });
        }
        for MatchByte(offset, hex) in &self.spec.response.match_bytes {
            let bytes = hex_to_bytes(hex)?;
            if offset + bytes.len() > input_len {
                return Err(UserProfileError::OutOfRange {
                    offset: *offset,
                    len: bytes.len(),
                    input_len,
                });
            }
        }
        Ok(())
    }

    /// 只对匹配头命中的报文提取电量；短报文、越界或超范围返回 `None`。
    pub fn extract_level(&self, report: &[u8]) -> Result<Option<u8>, UserProfileError> {
        // A profile is bound to one input length. A report from another
        // collection (or a truncated read) must never be scored as a level.
        if report.len() != usize::from(self.binding.input_len)
            || !self.spec.response_matches(report)
        {
            return Ok(None);
        }
        match self.spec.extract_level(report) {
            Ok(level) => Ok(level),
            Err(ProtocolError::OutOfRange { .. }) => Ok(None),
            Err(err) => Err(UserProfileError::Protocol(err)),
        }
    }

    fn validate_matcher(&self) -> Result<(), UserProfileError> {
        let matcher = &self.spec.matcher;
        let hex_matches = |value: &Option<String>, expected: u16| {
            value
                .as_ref()
                .is_some_and(|v| v.eq_ignore_ascii_case(&format!("{expected:04X}")))
        };
        if !hex_matches(&matcher.vid, self.binding.vendor_id)
            || !hex_matches(&matcher.pid, self.binding.product_id)
            || !hex_matches(&matcher.usage_page, self.binding.usage_page)
            || !hex_matches(&matcher.usage, self.binding.usage)
        {
            return Err(UserProfileError::MatcherBindingMismatch);
        }
        if matcher.min_output_len.is_some() {
            return Err(UserProfileError::MatcherOutputLength);
        }
        Ok(())
    }
}

/// 用户 profile 集合。禁用项保留用于展示，但不参与接口解析。
#[derive(Clone, Debug, Default)]
pub struct UserProfileCatalog {
    profiles: Vec<UserProtocolProfile>,
}

impl UserProfileCatalog {
    /// 校验全部 profile，并拒绝两个启用 profile 绑定同一接口。
    pub fn new(profiles: Vec<UserProtocolProfile>) -> Result<Self, UserProfileError> {
        for profile in &profiles {
            profile.validate()?;
        }
        for (index, profile) in profiles.iter().enumerate() {
            if !profile.enabled {
                continue;
            }
            let duplicate = profiles[index + 1..]
                .iter()
                .any(|other| other.enabled && profile.binding.same_interface(&other.binding));
            if duplicate {
                return Err(UserProfileError::AmbiguousBinding);
            }
        }
        Ok(Self { profiles })
    }

    pub fn profiles(&self) -> &[UserProtocolProfile] {
        &self.profiles
    }

    pub fn enabled_profiles(&self) -> impl Iterator<Item = &UserProtocolProfile> {
        self.profiles.iter().filter(|profile| profile.enabled)
    }

    /// 为接口找到唯一启用的 profile；0 个返回 `Ok(None)`，多个返回 `AmbiguousBinding`。
    pub fn find_for_interface(
        &self,
        info: &InterfaceBinding,
    ) -> Result<Option<&UserProtocolProfile>, UserProfileError> {
        let mut found: Option<&UserProtocolProfile> = None;
        for profile in self.enabled_profiles() {
            if !profile.binding.same_interface(info) {
                continue;
            }
            if found.is_some() {
                return Err(UserProfileError::AmbiguousBinding);
            }
            found = Some(profile);
        }
        Ok(found)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Field;
    use serde_json::json;

    fn binding() -> InterfaceBinding {
        InterfaceBinding {
            container_id: "container-a".to_owned(),
            instance_id: "instance-a".to_owned(),
            interface_number: Some(2),
            usage_page: 0xFF05,
            usage: 0x0001,
            vendor_id: 0x1234,
            product_id: 0x5678,
            input_len: 16,
        }
    }

    fn profile_json() -> serde_json::Value {
        let binding = binding();
        json!({
            "schema": 1,
            "enabled": true,
            "binding": {
                "container_id": binding.container_id,
                "instance_id": binding.instance_id,
                "interface_number": binding.interface_number,
                "usage_page": binding.usage_page,
                "usage": binding.usage,
                "vendor_id": binding.vendor_id,
                "product_id": binding.product_id,
                "input_len": binding.input_len,
            },
            "spec": {
                "schema": 1,
                "id": "user.2g4.example",
                "name": "用户配置：示例 2.4G 接收器",
                "match": {
                    "vid": format!("{:04X}", binding.vendor_id),
                    "pid": format!("{:04X}", binding.product_id),
                    "usage_page": format!("{:04X}", binding.usage_page),
                    "usage": format!("{:04X}", binding.usage),
                    "min_output_len": null,
                },
                "request": {
                    "kind": "none",
                    "report_id": null,
                    "length": null,
                    "payload_hex": "",
                    "pad": null,
                },
                "response": {
                    "kind": "input",
                    "timeout_ms": 600,
                    "retries": 0,
                    "match": [[1, "72"]],
                },
                "extract": {
                    "level": {
                        "offset": 4,
                        "type": "u8",
                        "scale": 1.0,
                        "bias": 0.0,
                        "min": 0.0,
                        "max": 100.0,
                    },
                    "charging": null,
                    "voltage_mv": null,
                },
                "verification": "unverified",
                "notes": null,
            },
        })
    }

    fn profile() -> UserProtocolProfile {
        UserProtocolProfile::from_json(&profile_json().to_string()).unwrap()
    }

    fn report(level: u8) -> [u8; 16] {
        let mut report = [0u8; 16];
        report[1] = 0x72;
        report[4] = level;
        report
    }

    #[test]
    fn two_point_calibration_forward() {
        let calibration = Calibration::two_point(
            CalibrationSample {
                raw: 10,
                shown_percent: 20,
            },
            CalibrationSample {
                raw: 60,
                shown_percent: 70,
            },
        )
        .unwrap();
        assert!((calibration.scale - 1.0).abs() < 1e-6);
        assert!((calibration.bias - 10.0).abs() < 1e-6);
        assert_eq!(calibration.apply(35), 45.0);
    }

    #[test]
    fn two_point_calibration_reverse() {
        let calibration = Calibration::two_point(
            CalibrationSample {
                raw: 20,
                shown_percent: 100,
            },
            CalibrationSample {
                raw: 120,
                shown_percent: 0,
            },
        )
        .unwrap();
        assert!((calibration.scale + 1.0).abs() < 1e-6);
        assert!((calibration.bias - 120.0).abs() < 1e-6);
        assert_eq!(calibration.apply(20), 100.0);
        assert_eq!(calibration.apply(70), 50.0);
    }

    #[test]
    fn two_point_calibration_rejects_same_raw() {
        let sample = CalibrationSample {
            raw: 10,
            shown_percent: 50,
        };
        assert_eq!(
            Calibration::two_point(
                sample,
                CalibrationSample {
                    raw: 10,
                    shown_percent: 60,
                },
            ),
            Err(CalibrationError::SameRaw)
        );
    }

    #[test]
    fn two_point_calibration_rejects_zero_scale() {
        assert_eq!(
            Calibration::two_point(
                CalibrationSample {
                    raw: 10,
                    shown_percent: 50,
                },
                CalibrationSample {
                    raw: 20,
                    shown_percent: 50,
                },
            ),
            Err(CalibrationError::ZeroOrInvalidScale)
        );
    }

    #[test]
    fn user_profile_round_trip_preserves_bias() {
        let mut profile = profile();
        profile.spec.extract.level.scale = -0.5;
        profile.spec.extract.level.bias = 7.5;
        let text = serde_json::to_string(&profile).unwrap();
        let parsed = UserProtocolProfile::from_json(&text).unwrap();
        assert_eq!(parsed.spec.extract.level.bias, 7.5);
        assert_eq!(parsed.spec.extract.level.scale, -0.5);
        assert_eq!(parsed, profile);
    }

    #[test]
    fn user_profile_rejects_output_feature_set_and_feature_get() {
        let mut output = profile();
        output.spec.request.kind = RequestKind::Output;
        assert_eq!(output.validate(), Err(UserProfileError::RequestKind));

        let mut feature_set = profile();
        feature_set.spec.request.kind = RequestKind::FeatureSet;
        assert_eq!(feature_set.validate(), Err(UserProfileError::RequestKind));

        let mut feature_get = profile();
        feature_get.spec.response.kind = ResponseKind::FeatureGet;
        assert_eq!(feature_get.validate(), Err(UserProfileError::ResponseKind));

        let mut payload = profile();
        payload.spec.request.payload_hex = "7D72".to_owned();
        assert_eq!(payload.validate(), Err(UserProfileError::RequestPayload));

        let mut report_id = profile();
        report_id.spec.request.report_id = Some(8);
        assert_eq!(report_id.validate(), Err(UserProfileError::RequestPayload));
    }

    #[test]
    fn user_profile_requires_non_empty_match_header() {
        let mut profile = profile();
        profile.spec.response.match_bytes.clear();
        assert_eq!(profile.validate(), Err(UserProfileError::EmptyMatchHeader));
    }

    #[test]
    fn user_profile_requires_complete_interface_binding() {
        let mut missing_interface = profile();
        missing_interface.binding.interface_number = None;
        assert_eq!(
            missing_interface.validate(),
            Err(UserProfileError::MissingInterfaceNumber)
        );

        let mut missing_container = profile();
        missing_container.binding.container_id = "   ".to_owned();
        assert_eq!(
            missing_container.validate(),
            Err(UserProfileError::MissingContainerId)
        );

        let mut missing_instance = profile();
        missing_instance.binding.instance_id = String::new();
        assert_eq!(
            missing_instance.validate(),
            Err(UserProfileError::MissingInstanceId)
        );

        let mut empty_len = profile();
        empty_len.binding.input_len = 0;
        assert_eq!(
            empty_len.validate(),
            Err(UserProfileError::EmptyInputLength)
        );
    }

    #[test]
    fn user_profile_rejects_matcher_binding_disagreement() {
        let mut other_vid = profile();
        other_vid.binding.vendor_id = 0x046D;
        assert_eq!(
            other_vid.validate(),
            Err(UserProfileError::MatcherBindingMismatch)
        );

        let mut missing_usage = profile();
        missing_usage.spec.matcher.usage = None;
        assert_eq!(
            missing_usage.validate(),
            Err(UserProfileError::MatcherBindingMismatch)
        );

        let mut output_len = profile();
        output_len.spec.matcher.min_output_len = Some(64);
        assert_eq!(
            output_len.validate(),
            Err(UserProfileError::MatcherOutputLength)
        );
    }

    #[test]
    fn user_profile_requires_u8_and_no_charging() {
        let mut u16_level = profile();
        u16_level.spec.extract.level.value_type = FieldType::U16Le;
        assert_eq!(u16_level.validate(), Err(UserProfileError::LevelType));

        let mut charging = profile();
        charging.spec.extract.charging = Some(Field {
            offset: 5,
            value_type: FieldType::U8,
            scale: 1.0,
            bias: 0.0,
            min: None,
            max: None,
        });
        assert_eq!(
            charging.validate(),
            Err(UserProfileError::ChargingUnsupported)
        );

        let mut voltage = profile();
        voltage.spec.extract.voltage_mv = Some(Field {
            offset: 6,
            value_type: FieldType::U16Le,
            scale: 1.0,
            bias: 0.0,
            min: None,
            max: None,
        });
        assert_eq!(
            voltage.validate(),
            Err(UserProfileError::VoltageUnsupported)
        );

        let mut bounds = profile();
        bounds.spec.extract.level.max = Some(200.0);
        assert_eq!(bounds.validate(), Err(UserProfileError::LevelBounds));
    }

    #[test]
    fn user_profile_requires_exact_report_length() {
        let mut level_past_end = profile();
        level_past_end.spec.extract.level.offset = 16;
        assert_eq!(
            level_past_end.validate(),
            Err(UserProfileError::OutOfRange {
                offset: 16,
                len: 1,
                input_len: 16,
            })
        );

        let mut header_past_end = profile();
        header_past_end.spec.response.match_bytes = vec![MatchByte(15, "72AA".to_owned())];
        assert_eq!(
            header_past_end.validate(),
            Err(UserProfileError::OutOfRange {
                offset: 15,
                len: 2,
                input_len: 16,
            })
        );
    }

    #[test]
    fn same_vid_pid_different_container_does_not_match() {
        let catalog = UserProfileCatalog::new(vec![profile()]).unwrap();
        let mut info = binding();
        info.container_id = "container-b".to_owned();
        assert!(catalog.find_for_interface(&info).unwrap().is_none());
        assert!(catalog.find_for_interface(&binding()).unwrap().is_some());
    }

    #[test]
    fn same_container_different_interface_does_not_match() {
        let catalog = UserProfileCatalog::new(vec![profile()]).unwrap();

        let mut other_interface = binding();
        other_interface.interface_number = Some(3);
        assert!(
            catalog
                .find_for_interface(&other_interface)
                .unwrap()
                .is_none()
        );

        let mut other_usage = binding();
        other_usage.usage = 0x0002;
        assert!(catalog.find_for_interface(&other_usage).unwrap().is_none());
    }

    #[test]
    fn disabled_profile_is_not_a_resolution_candidate() {
        let mut profile = profile();
        profile.enabled = false;
        let catalog = UserProfileCatalog::new(vec![profile]).unwrap();
        assert_eq!(catalog.profiles().len(), 1);
        assert_eq!(catalog.enabled_profiles().count(), 0);
        assert!(catalog.find_for_interface(&binding()).unwrap().is_none());
    }

    #[test]
    fn ambiguous_enabled_profiles_rejected() {
        let mut second = profile();
        second.spec.id = "user.2g4.second".to_owned();
        assert!(matches!(
            UserProfileCatalog::new(vec![profile(), second.clone()]),
            Err(UserProfileError::AmbiguousBinding)
        ));

        second.enabled = false;
        assert!(UserProfileCatalog::new(vec![profile(), second]).is_ok());
    }

    #[test]
    fn binding_identity_is_case_insensitive_and_includes_input_len() {
        let mut upper = binding();
        upper.container_id = upper.container_id.to_uppercase();
        upper.instance_id = upper.instance_id.to_uppercase();
        assert!(binding().same_interface(&upper));

        let mut longer = binding();
        longer.input_len = 32;
        assert!(!binding().same_interface(&longer));
    }

    #[test]
    fn different_input_length_is_not_a_binding_candidate() {
        let catalog = UserProfileCatalog::new(vec![profile()]).unwrap();
        let mut info = binding();
        info.input_len = 32;
        assert!(catalog.find_for_interface(&info).unwrap().is_none());
    }

    #[test]
    fn short_and_malformed_reports_return_none_without_panic() {
        let profile = profile();
        assert_eq!(profile.extract_level(&[]).unwrap(), None);
        assert_eq!(profile.extract_level(&[0x00]).unwrap(), None);
        assert_eq!(profile.extract_level(&[0x00, 0x72]).unwrap(), None);
        assert_eq!(
            profile
                .extract_level(&[0x00, 0x00, 0x00, 0x00, 0x32])
                .unwrap(),
            None
        );
        assert_eq!(profile.extract_level(&report(55)).unwrap(), Some(55));

        let mut over_range = report(0);
        over_range[4] = 200;
        assert_eq!(profile.extract_level(&over_range).unwrap(), None);

        // The profile is bound to a 16-byte report; a longer collection must
        // not be scored even when the match header happens to line up.
        let mut long = vec![0u8; 64];
        long[1] = 0x72;
        long[4] = 88;
        assert_eq!(profile.extract_level(&long).unwrap(), None);

        let mut one_short = report(88).to_vec();
        one_short.truncate(15);
        assert_eq!(profile.extract_level(&one_short).unwrap(), None);
    }

    #[test]
    fn corrupt_profile_json_is_rejected() {
        assert!(matches!(
            UserProtocolProfile::from_json("{"),
            Err(UserProfileError::Json(_))
        ));
        assert!(matches!(
            UserProtocolProfile::from_json(r#"{"schema":1}"#),
            Err(UserProfileError::Json(_))
        ));

        let mut wrong_schema = profile_json();
        wrong_schema["schema"] = json!(2);
        assert_eq!(
            UserProtocolProfile::from_json(&wrong_schema.to_string()),
            Err(UserProfileError::Schema(2))
        );

        let mut verified = profile_json();
        verified["spec"]["verification"] = json!("verified");
        assert_eq!(
            UserProtocolProfile::from_json(&verified.to_string()),
            Err(UserProfileError::Verification)
        );
    }
}
