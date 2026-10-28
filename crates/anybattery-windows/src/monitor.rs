//! 托盘悬浮窗所需的显示器工作区几何。
//!
//! 这里只调用 Win32 的显示器/DPI 查询，不涉及 HID、蓝牙或窗口控制。

use std::mem::size_of;

use windows::Win32::Foundation::POINT;
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, HMONITOR, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};

/// 托盘悬浮窗定位用的物理像素工作区。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MonitorGeometry {
    pub work_left: f64,
    pub work_top: f64,
    pub work_right: f64,
    pub work_bottom: f64,
    /// 目标显示器的有效 DPI / 96。
    pub scale: f32,
}

impl MonitorGeometry {
    pub fn width(&self) -> f64 {
        (self.work_right - self.work_left).max(1.0)
    }

    pub fn height(&self) -> f64 {
        (self.work_bottom - self.work_top).max(1.0)
    }
}

/// 返回包含给定物理像素点的显示器工作区。
pub fn work_area_at(x: f64, y: f64) -> Option<MonitorGeometry> {
    let point = POINT {
        x: x.round().clamp(i32::MIN as f64, i32::MAX as f64) as i32,
        y: y.round().clamp(i32::MIN as f64, i32::MAX as f64) as i32,
    };
    let monitor = unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST) };
    if monitor.is_invalid() {
        return None;
    }
    let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
    info.cbSize = size_of::<MONITORINFO>() as u32;
    if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        return None;
    }
    let dpi = effective_dpi(monitor).unwrap_or(96);
    Some(MonitorGeometry {
        work_left: f64::from(info.rcWork.left),
        work_top: f64::from(info.rcWork.top),
        work_right: f64::from(info.rcWork.right),
        work_bottom: f64::from(info.rcWork.bottom),
        scale: (dpi as f32 / 96.0).max(0.5),
    })
}

fn effective_dpi(monitor: HMONITOR) -> Option<u32> {
    let mut dpi_x = 96u32;
    let mut dpi_y = 96u32;
    unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) }
        .ok()
        .map(|()| dpi_x.max(48))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_has_positive_extent() {
        let monitor = MonitorGeometry {
            work_left: -1920.0,
            work_top: 0.0,
            work_right: 0.0,
            work_bottom: 1080.0,
            scale: 1.25,
        };
        assert_eq!(monitor.width(), 1920.0);
        assert_eq!(monitor.height(), 1080.0);
    }
}
