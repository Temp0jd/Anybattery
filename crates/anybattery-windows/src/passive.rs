//! 只读被动输入捕获。
//!
//! 这里只允许读取 HID 输入报文：不发请求报文、不读 Feature、不写任何内容。
//! 用户自定义 2.4G 协议建立在"只能被动监听"这一安全子集上，因此本模块
//! 刻意不提供任何写入/Feature 入口，避免上层误用。

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::HANDLE;

use crate::hid;

/// 单次读取的时间切片（毫秒）。取消延迟不会超过一个切片。
pub const READ_SLICE_MS: u32 = 150;

/// 只读报文来源。
pub trait PassiveRead {
    /// 在 `timeout_ms` 内读取一份输入报文；没有报文时返回 `None`。
    fn read(&mut self, input_len: usize, timeout_ms: u32) -> Option<Vec<u8>>;
}

/// 只读 HID 句柄：始终以 `write = false` 打开，且不走同步零访问路径。
pub struct ReadOnlyHid {
    handle: HANDLE,
}

impl ReadOnlyHid {
    /// 以只读方式打开接口。打开失败返回 `None`，由调用方记录诊断。
    pub fn open(path: &str) -> Option<Self> {
        hid::open_path(path, false).map(|handle| Self { handle })
    }
}

impl PassiveRead for ReadOnlyHid {
    fn read(&mut self, input_len: usize, timeout_ms: u32) -> Option<Vec<u8>> {
        hid::read_report(self.handle, input_len, timeout_ms)
    }
}

impl Drop for ReadOnlyHid {
    fn drop(&mut self) {
        hid::close(self.handle);
    }
}

impl std::fmt::Debug for ReadOnlyHid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReadOnlyHid").finish_non_exhaustive()
    }
}

/// 一次被动捕获的统计。只报告数量，不保存原始报文。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CaptureSummary {
    pub report_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PassiveCaptureError {
    #[error("无法打开接口")]
    OpenFailed,
    #[error("捕获窗口内没有收到报文")]
    NoReports,
    #[error("捕获已取消")]
    Cancelled,
    #[error("读取失败")]
    ReadFailed,
}

impl PassiveCaptureError {
    /// 用户可见文案。
    pub fn label_zh(&self) -> &'static str {
        match self {
            Self::OpenFailed => "无法打开接口",
            Self::NoReports => "捕获窗口内没有收到报文",
            Self::Cancelled => "捕获已取消",
            Self::ReadFailed => "读取失败",
        }
    }
}

/// 在 `window` 内持续被动读取，把长度等于 `input_len` 的报文交给 `on_report`。
///
/// 短报文、空报文一律丢弃（绝不补零伪装成完整报文）。每片读取后检查
/// `cancel`，取消立即返回 `Cancelled`。
pub fn capture_with<R: PassiveRead>(
    reader: &mut R,
    input_len: usize,
    window: Duration,
    cancel: &AtomicBool,
    on_report: &mut dyn FnMut(Vec<u8>),
) -> Result<CaptureSummary, PassiveCaptureError> {
    if input_len == 0 {
        return Err(PassiveCaptureError::ReadFailed);
    }
    let deadline = Instant::now() + window;
    let mut report_count = 0usize;
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err(PassiveCaptureError::Cancelled);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        let slice_ms = slice_millis(remaining);
        match reader.read(input_len, slice_ms) {
            Some(report) => {
                // A truncated or empty read is not a report: dropping it is the
                // only way to keep profiles from scoring fabricated bytes.
                if report.len() == input_len {
                    report_count += 1;
                    on_report(report);
                }
            }
            // A real HID read blocks for the whole slice; scripted readers
            // return at once, so yield briefly to avoid a busy spin.
            None => std::thread::sleep(Duration::from_millis(1)),
        }
    }
    if cancel.load(Ordering::SeqCst) {
        return Err(PassiveCaptureError::Cancelled);
    }
    if report_count == 0 {
        Err(PassiveCaptureError::NoReports)
    } else {
        Ok(CaptureSummary { report_count })
    }
}

/// 把剩余时间压到 1..=150ms 的读取切片。
pub(crate) fn slice_millis(remaining: Duration) -> u32 {
    let slice = remaining.min(Duration::from_millis(u64::from(READ_SLICE_MS)));
    u32::try_from(slice.as_millis())
        .unwrap_or(READ_SLICE_MS)
        .max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    const INPUT_LEN: usize = 16;

    fn report(level: u8) -> Vec<u8> {
        let mut bytes = vec![0u8; INPUT_LEN];
        bytes[1] = 0x72;
        bytes[3] = level;
        bytes
    }

    /// 脚本化只读来源：记录被要求写入的次数（必须始终为 0）。
    struct ScriptedReader {
        queue: Vec<Option<Vec<u8>>>,
        fallback: Option<Vec<u8>>,
        writes: usize,
    }

    impl ScriptedReader {
        fn new(queue: Vec<Option<Vec<u8>>>) -> Self {
            Self {
                queue,
                fallback: None,
                writes: 0,
            }
        }

        fn repeating(report: Vec<u8>) -> Self {
            Self {
                queue: Vec::new(),
                fallback: Some(report),
                writes: 0,
            }
        }

        fn silent() -> Self {
            Self::new(Vec::new())
        }
    }

    impl PassiveRead for ScriptedReader {
        fn read(&mut self, _input_len: usize, _timeout_ms: u32) -> Option<Vec<u8>> {
            if self.queue.is_empty() {
                return self.fallback.clone();
            }
            self.queue.remove(0)
        }
    }

    #[test]
    fn capture_collects_full_length_reports() {
        let mut reader = ScriptedReader::new(vec![Some(report(42)), Some(report(43))]);
        let cancel = AtomicBool::new(false);
        let mut seen = Vec::new();
        let summary = capture_with(
            &mut reader,
            INPUT_LEN,
            Duration::from_millis(150),
            &cancel,
            &mut |bytes| seen.push(bytes),
        )
        .expect("capture should succeed");
        assert_eq!(summary.report_count, 2);
        assert_eq!(seen.len(), 2);
        assert!(seen.iter().all(|bytes| bytes.len() == INPUT_LEN));
        assert_eq!(seen[0][3], 42);
        assert_eq!(reader.writes, 0);
    }

    #[test]
    fn capture_stops_when_cancelled() {
        let mut reader = ScriptedReader::repeating(report(50));
        let cancel = AtomicBool::new(true);
        let mut count = 0;
        let result = capture_with(
            &mut reader,
            INPUT_LEN,
            Duration::from_millis(500),
            &cancel,
            &mut |_| count += 1,
        );
        assert_eq!(result, Err(PassiveCaptureError::Cancelled));
        assert_eq!(count, 0);
        assert_eq!(reader.writes, 0);
    }

    #[test]
    fn capture_reports_cancelled_after_receiving_reports() {
        let mut reader = ScriptedReader::repeating(report(51));
        let cancel = AtomicBool::new(false);
        let result = capture_with(
            &mut reader,
            INPUT_LEN,
            Duration::from_millis(30),
            &cancel,
            &mut |_| cancel.store(true, Ordering::SeqCst),
        );
        assert_eq!(result, Err(PassiveCaptureError::Cancelled));
        assert_eq!(reader.writes, 0);
    }

    #[test]
    fn capture_without_reports_times_out() {
        let mut reader = ScriptedReader::silent();
        let cancel = AtomicBool::new(false);
        let mut count = 0;
        let result = capture_with(
            &mut reader,
            INPUT_LEN,
            Duration::from_millis(20),
            &cancel,
            &mut |_| count += 1,
        );
        assert_eq!(result, Err(PassiveCaptureError::NoReports));
        assert_eq!(count, 0);
        assert_eq!(reader.writes, 0);
    }

    #[test]
    fn capture_discards_short_reports() {
        let mut reader = ScriptedReader::new(vec![Some(vec![0x72, 0x00, 0x00]), Some(Vec::new())]);
        let cancel = AtomicBool::new(false);
        let mut seen = Vec::new();
        let result = capture_with(
            &mut reader,
            INPUT_LEN,
            Duration::from_millis(20),
            &cancel,
            &mut |bytes| seen.push(bytes),
        );
        assert_eq!(result, Err(PassiveCaptureError::NoReports));
        assert!(seen.is_empty());
        assert_eq!(reader.writes, 0);
    }

    #[test]
    fn capture_rejects_empty_input_length() {
        let mut reader = ScriptedReader::repeating(Vec::new());
        let cancel = AtomicBool::new(false);
        let result = capture_with(
            &mut reader,
            0,
            Duration::from_millis(20),
            &cancel,
            &mut |_| panic!("empty reports must never be delivered"),
        );
        assert_eq!(result, Err(PassiveCaptureError::ReadFailed));
        assert_eq!(reader.writes, 0);
    }

    #[test]
    fn capture_callback_may_collect_into_buffer() {
        let mut reader = ScriptedReader::repeating(report(77));
        let cancel = AtomicBool::new(false);
        let mut buffer: Vec<Vec<u8>> = Vec::new();
        let result = capture_with(
            &mut reader,
            INPUT_LEN,
            Duration::from_millis(40),
            &cancel,
            &mut |bytes| buffer.push(bytes),
        );
        let summary = result.expect("at least one report");
        assert!(summary.report_count >= 1);
        assert_eq!(buffer.len(), summary.report_count);
        assert_eq!(reader.writes, 0);
    }

    #[test]
    fn slices_are_bounded() {
        assert_eq!(slice_millis(Duration::from_millis(500)), READ_SLICE_MS);
        assert_eq!(slice_millis(Duration::from_millis(20)), 20);
        assert_eq!(slice_millis(Duration::from_nanos(1)), 1);
    }
}
