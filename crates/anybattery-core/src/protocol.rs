use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolVerification {
    Verified,
    Community,
    Unverified,
}

impl ProtocolVerification {
    pub fn label_zh(self) -> &'static str {
        match self {
            Self::Verified => "已验证",
            Self::Community => "社区协议",
            Self::Unverified => "未验证",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Matcher {
    pub vid: Option<String>,
    pub pid: Option<String>,
    pub usage_page: Option<String>,
    pub usage: Option<String>,
    pub min_output_len: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestKind {
    Output,
    FeatureSet,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponseKind {
    Input,
    FeatureGet,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pad {
    Zero,
    None,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub kind: RequestKind,
    pub report_id: Option<u8>,
    pub length: Option<u32>,
    pub payload_hex: String,
    #[serde(default)]
    pub pad: Option<Pad>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Response {
    pub kind: ResponseKind,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default)]
    pub retries: u8,
    #[serde(default, rename = "match")]
    pub match_bytes: Vec<MatchByte>,
}

impl Default for Response {
    fn default() -> Self {
        Self {
            kind: ResponseKind::Input,
            timeout_ms: default_timeout_ms(),
            retries: 0,
            match_bytes: Vec::new(),
        }
    }
}

fn default_timeout_ms() -> u64 {
    600
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchByte(pub usize, pub String);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    U8,
    U16Le,
    U16Be,
    U24Le,
    I8,
    I16Le,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Field {
    pub offset: usize,
    #[serde(rename = "type")]
    pub value_type: FieldType,
    #[serde(default = "default_scale")]
    pub scale: f32,
    pub min: Option<f32>,
    pub max: Option<f32>,
}

fn default_scale() -> f32 {
    1.0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Extract {
    pub level: Field,
    pub charging: Option<Field>,
    pub voltage_mv: Option<Field>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProtocolSpec {
    pub schema: u32,
    pub id: String,
    pub name: String,
    #[serde(rename = "match")]
    pub matcher: Matcher,
    pub request: Request,
    pub response: Response,
    pub extract: Extract,
    pub verification: ProtocolVerification,
    pub notes: Option<String>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProtocolError {
    #[error("JSON 解析失败: {0}")]
    Json(String),
    #[error("不支持的 schema: {0}")]
    Schema(u32),
    #[error("协议 id 非法: {0}")]
    Id(String),
    #[error("十六进制报文非法: {0}")]
    Hex(String),
    #[error("报文字段越界: offset={offset}, len={len}")]
    OutOfRange { offset: usize, len: usize },
    #[error("请求长度超过 128 字节")]
    RequestTooLong,
    #[error("超时必须为 1..=5000 ms")]
    Timeout,
    #[error("重试次数必须 <= 5")]
    Retries,
    #[error("字段缩放必须为有限正数")]
    Scale,
    #[error("字段 min/max 非法")]
    Bounds,
}

impl ProtocolSpec {
    pub fn from_json(text: &str) -> Result<Self, ProtocolError> {
        let spec: Self =
            serde_json::from_str(text).map_err(|e| ProtocolError::Json(e.to_string()))?;
        spec.validate()?;
        Ok(spec)
    }

    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.schema != 1 {
            return Err(ProtocolError::Schema(self.schema));
        }
        if self.id.is_empty()
            || self.id.len() > 64
            || !self
                .id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        {
            return Err(ProtocolError::Id(self.id.clone()));
        }
        let payload = hex_to_bytes(&self.request.payload_hex)?;
        if payload.len() > 128 {
            return Err(ProtocolError::RequestTooLong);
        }
        if let Some(length) = self.request.length {
            if length > 128 || payload.len() > length as usize {
                return Err(ProtocolError::RequestTooLong);
            }
        }
        if !(1..=5000).contains(&self.response.timeout_ms) {
            return Err(ProtocolError::Timeout);
        }
        if self.response.retries > 5 {
            return Err(ProtocolError::Retries);
        }
        for MatchByte(offset, hex) in &self.response.match_bytes {
            let bytes = hex_to_bytes(hex)?;
            if bytes.is_empty() || *offset > 255 {
                return Err(ProtocolError::OutOfRange {
                    offset: *offset,
                    len: bytes.len(),
                });
            }
        }
        validate_field(&self.extract.level)?;
        if let Some(field) = self.extract.charging.as_ref() {
            validate_field(field)?;
        }
        if let Some(field) = self.extract.voltage_mv.as_ref() {
            validate_field(field)?;
        }
        Ok(())
    }

    pub fn matches_vid_pid(&self, vid: u16, pid: u16) -> bool {
        let hex = |v: u16| format!("{:04X}", v);
        let ok = |expected: &Option<String>, actual: String| {
            expected
                .as_ref()
                .is_none_or(|e| e.eq_ignore_ascii_case(&actual))
        };
        ok(&self.matcher.vid, hex(vid)) && ok(&self.matcher.pid, hex(pid))
    }

    pub fn response_matches(&self, report: &[u8]) -> bool {
        self.response
            .match_bytes
            .iter()
            .all(|MatchByte(offset, hex)| {
                let Ok(expected) = hex_to_bytes(hex) else {
                    return false;
                };
                report.get(*offset..*offset + expected.len()) == Some(expected.as_slice())
            })
    }

    pub fn extract_level(&self, report: &[u8]) -> Result<Option<u8>, ProtocolError> {
        let Some(raw) = read_field(report, &self.extract.level)? else {
            return Ok(None);
        };
        let value = raw * self.extract.level.scale;
        if !value.is_finite() {
            return Ok(None);
        }
        if let Some(min) = self.extract.level.min {
            if value < min {
                return Ok(None);
            }
        }
        if let Some(max) = self.extract.level.max {
            if value > max {
                return Ok(None);
            }
        }
        if !(0.0..=100.0).contains(&value) {
            return Ok(None);
        }
        Ok(Some(value.round() as u8))
    }

    pub fn extract_charging(&self, report: &[u8]) -> Result<Option<bool>, ProtocolError> {
        let Some(field) = self.extract.charging.as_ref() else {
            return Ok(None);
        };
        let Some(raw) = read_field(report, field)? else {
            return Ok(None);
        };
        Ok(Some(raw != 0.0))
    }
}

fn validate_field(field: &Field) -> Result<(), ProtocolError> {
    if !field.scale.is_finite() || field.scale <= 0.0 {
        return Err(ProtocolError::Scale);
    }
    if let (Some(min), Some(max)) = (field.min, field.max) {
        if min > max {
            return Err(ProtocolError::Bounds);
        }
    }
    Ok(())
}

fn read_field(report: &[u8], field: &Field) -> Result<Option<f32>, ProtocolError> {
    let offset = field.offset;
    let read = |len: usize| -> Result<&[u8], ProtocolError> {
        report
            .get(offset..offset + len)
            .ok_or(ProtocolError::OutOfRange { offset, len })
    };
    let value = match field.value_type {
        FieldType::U8 => read(1)?[0] as f32,
        FieldType::I8 => read(1)?[0] as i8 as f32,
        FieldType::U16Le => u16::from_le_bytes(read(2)?.try_into().unwrap()) as f32,
        FieldType::U16Be => u16::from_be_bytes(read(2)?.try_into().unwrap()) as f32,
        FieldType::I16Le => i16::from_le_bytes(read(2)?.try_into().unwrap()) as f32,
        FieldType::U24Le => {
            let b = read(3)?;
            (b[0] as u32 | (b[1] as u32) << 8 | (b[2] as u32) << 16) as f32
        }
    };
    Ok(Some(value))
}

pub fn hex_to_bytes(hex: &str) -> Result<Vec<u8>, ProtocolError> {
    let cleaned: String = hex
        .chars()
        .filter(|c| !c.is_whitespace() && *c != ':' && *c != '-')
        .collect();
    if cleaned.len() % 2 != 0 {
        return Err(ProtocolError::Hex(hex.to_owned()));
    }
    let mut out = Vec::with_capacity(cleaned.len() / 2);
    for i in (0..cleaned.len()).step_by(2) {
        let byte = u8::from_str_radix(&cleaned[i..i + 2], 16)
            .map_err(|_| ProtocolError::Hex(hex.to_owned()))?;
        out.push(byte);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "schema": 1,
      "id": "example.atk64",
      "name": "ATK/VXE 64-byte",
      "match": { "vid": "373B", "pid": null, "usage_page": "FF05", "usage": null, "min_output_len": 64 },
      "request": { "kind": "output", "report_id": 8, "length": 64, "payload_hex": "7D720200010701", "pad": "zero" },
      "response": { "kind": "input", "timeout_ms": 600, "retries": 3, "match": [[1, "72"], [5, "07"]] },
      "extract": {
        "level": { "offset": 7, "type": "u8", "scale": 1.0, "min": 0, "max": 100 },
        "charging": null,
        "voltage_mv": null
      },
      "verification": "verified",
      "notes": "电量在回复第 7 字节"
    }"#;

    #[test]
    fn parses_and_validates_sample() {
        let spec = ProtocolSpec::from_json(SAMPLE).unwrap();
        assert_eq!(spec.id, "example.atk64");
        assert!(spec.matches_vid_pid(0x373B, 0x1234));
        assert!(!spec.matches_vid_pid(0x046D, 0x1234));
    }

    #[test]
    fn matches_response_and_extracts_level() {
        let spec = ProtocolSpec::from_json(SAMPLE).unwrap();
        let mut report = [0u8; 16];
        report[1] = 0x72;
        report[5] = 0x07;
        report[7] = 62;
        assert!(spec.response_matches(&report));
        assert_eq!(spec.extract_level(&report).unwrap(), Some(62));
    }

    #[test]
    fn rejects_out_of_range_level() {
        let spec = ProtocolSpec::from_json(SAMPLE).unwrap();
        let mut report = [0u8; 16];
        report[1] = 0x72;
        report[5] = 0x07;
        report[7] = 101;
        assert_eq!(spec.extract_level(&report).unwrap(), None);
    }

    #[test]
    fn rejects_bad_hex_and_schema() {
        let bad = SAMPLE.replace("7D720200010701", "7D72020001070");
        assert!(matches!(
            ProtocolSpec::from_json(&bad),
            Err(ProtocolError::Hex(_))
        ));
        let bad = SAMPLE.replace(r#""schema": 1"#, r#""schema": 2"#);
        assert!(matches!(
            ProtocolSpec::from_json(&bad),
            Err(ProtocolError::Schema(2))
        ));
    }

    #[test]
    fn reads_u16_and_scales() {
        let mut spec = ProtocolSpec::from_json(SAMPLE).unwrap();
        spec.extract.level.offset = 8;
        spec.extract.level.value_type = FieldType::U16Le;
        spec.extract.level.scale = 0.5;
        spec.extract.level.min = None;
        spec.extract.level.max = None;
        let mut report = [0u8; 16];
        report[8] = 100;
        report[9] = 0;
        assert_eq!(spec.extract_level(&report).unwrap(), Some(50));
    }
}
