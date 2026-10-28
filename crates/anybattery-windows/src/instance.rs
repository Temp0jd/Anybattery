//! Per-session single-instance guard.
//!
//! The first process owns a named mutex and keeps a named auto-reset event
//! alive. A second process signals that event and exits; the primary process
//! turns the signal into the existing `UiCommand::Show` path, so a hidden or
//! minimized window is restored without a raw `FindWindow`/foreground hack.

use std::ffi::c_void;
use std::thread::{self, JoinHandle};

use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, WAIT_OBJECT_0,
};
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexW, INFINITE, SetEvent, WaitForSingleObject,
};
use windows::core::PCWSTR;

const MUTEX_NAME: &str = r"Local\AnyBattery.SingleInstance";
const EVENT_NAME: &str = r"Local\AnyBattery.Show";

#[derive(Debug, thiserror::Error)]
pub enum InstanceError {
    #[error("创建单实例事件失败: {0}")]
    CreateEvent(windows::core::Error),
    #[error("创建单实例互斥量失败: {0}")]
    CreateMutex(windows::core::Error),
    #[error("唤醒已有实例失败: {0}")]
    Signal(windows::core::Error),
    #[error("启动唤醒监听线程失败: {0}")]
    Spawn(std::io::Error),
}

/// The first process in the current Windows session.
pub struct PrimaryInstance {
    mutex: HANDLE,
    event: HANDLE,
}

/// Result of trying to become the primary AnyBattery process.
pub enum AcquireInstance {
    Primary(PrimaryInstance),
    Secondary,
}

impl PrimaryInstance {
    /// Wait for a second process to ask the primary to show its window.
    ///
    /// The callback runs on a dedicated thread. Keep the returned handle only
    /// if you need to join it; the primary instance itself must stay alive for
    /// the lifetime of the process.
    pub fn spawn_show_watcher<F>(&self, on_show: F) -> Result<JoinHandle<()>, InstanceError>
    where
        F: Fn() + Send + 'static,
    {
        // HANDLE is a raw pointer and not Send. The event handle stays valid
        // because `PrimaryInstance` owns it for the process lifetime.
        let raw = self.event.0 as usize;
        thread::Builder::new()
            .name("anybattery-show".to_owned())
            .spawn(move || {
                let event = HANDLE(raw as *mut c_void);
                loop {
                    let status = unsafe { WaitForSingleObject(event, INFINITE) };
                    if status == WAIT_OBJECT_0 {
                        on_show();
                    } else {
                        tracing::warn!(status = status.0, "single-instance show event failed");
                        break;
                    }
                }
            })
            .map_err(InstanceError::Spawn)
    }
}

impl Drop for PrimaryInstance {
    fn drop(&mut self) {
        // The mutex is used as an existence marker; closing the last handle
        // removes it when the process exits. There is no need to ReleaseMutex.
        unsafe {
            let _ = CloseHandle(self.event);
            let _ = CloseHandle(self.mutex);
        }
    }
}

/// Become the primary instance, or signal the running one and return
/// [`AcquireInstance::Secondary`].
pub fn acquire_or_signal() -> Result<AcquireInstance, InstanceError> {
    acquire_named(MUTEX_NAME, EVENT_NAME)
}

fn acquire_named(mutex_name: &str, event_name: &str) -> Result<AcquireInstance, InstanceError> {
    let event_wide = wide(event_name);
    let event = unsafe {
        CreateEventW(
            None,
            false, // auto-reset
            false,
            PCWSTR(event_wide.as_ptr()),
        )
    }
    .map_err(InstanceError::CreateEvent)?;

    let mutex_wide = wide(mutex_name);
    let mutex = match unsafe {
        CreateMutexW(
            None,
            true, // request initial ownership
            PCWSTR(mutex_wide.as_ptr()),
        )
    } {
        Ok(handle) => handle,
        Err(err) => {
            unsafe {
                let _ = CloseHandle(event);
            }
            return Err(InstanceError::CreateMutex(err));
        }
    };

    // GetLastError must be read immediately after CreateMutexW; no other Win32
    // call may run in between.
    let already_exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;

    if already_exists {
        let signal = unsafe { SetEvent(event) };
        unsafe {
            let _ = CloseHandle(event);
            let _ = CloseHandle(mutex);
        }
        signal.map_err(InstanceError::Signal)?;
        Ok(AcquireInstance::Secondary)
    } else {
        Ok(AcquireInstance::Primary(PrimaryInstance { mutex, event }))
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::time::{Duration, Instant};

    fn unique_names(tag: &str) -> (String, String) {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        (
            format!(r"Local\AnyBattery.Test.Mutex.{pid}.{id}.{tag}"),
            format!(r"Local\AnyBattery.Test.Event.{pid}.{id}.{tag}"),
        )
    }

    #[test]
    fn second_acquire_signals_the_primary() {
        let (mutex, event) = unique_names("signal");
        let first = acquire_named(&mutex, &event).expect("first acquire");
        let AcquireInstance::Primary(primary) = first else {
            panic!("first acquire must be primary");
        };

        let seen = Arc::new(AtomicBool::new(false));
        let seen_for_thread = seen.clone();
        let _watcher = primary
            .spawn_show_watcher(move || {
                seen_for_thread.store(true, Ordering::SeqCst);
            })
            .expect("spawn watcher");

        let second = acquire_named(&mutex, &event).expect("second acquire");
        assert!(matches!(second, AcquireInstance::Secondary));

        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if seen.load(Ordering::SeqCst) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("primary did not receive the show signal");
    }

    #[test]
    fn names_are_wide_and_nul_terminated() {
        let encoded = wide(r"Local\AnyBattery.Test");
        assert_eq!(encoded.last(), Some(&0));
        assert_eq!(
            encoded.len(),
            r"Local\AnyBattery.Test".encode_utf16().count() + 1
        );
    }
}
