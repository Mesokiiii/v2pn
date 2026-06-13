//! OS power-management notifications.
//!
//! Windows: hooks suspend/resume via `PowerRegisterSuspendResumeNotification`
//! so we can pre-emptively disconnect *before* the system goes to sleep —
//! which prevents the "wake up to broken internet because the proxy points
//! at a dead sing-box" failure mode.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PowerEvent {
    Suspend,
    Resume,
}

#[cfg(windows)]
mod imp {
    use super::PowerEvent;
    use std::ffi::c_void;
    use std::sync::OnceLock;

    use ::windows::Win32::Foundation::HANDLE;
    use ::windows::Win32::System::Power::{
        PowerRegisterSuspendResumeNotification, DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS,
    };
    use ::windows::Win32::UI::WindowsAndMessaging::REGISTER_NOTIFICATION_FLAGS;

    // Copy these constants from <winuser.h>/<winnt.h> — windows-rs doesn't
    // expose them under the same names across modules.
    const DEVICE_NOTIFY_CALLBACK: REGISTER_NOTIFICATION_FLAGS = REGISTER_NOTIFICATION_FLAGS(2);
    const PBT_APMSUSPEND: u32 = 0x0004;
    const PBT_APMRESUMEAUTOMATIC: u32 = 0x0012;

    type Handler = std::sync::Arc<dyn Fn(PowerEvent) + Send + Sync + 'static>;
    static HANDLER: OnceLock<Handler> = OnceLock::new();
    /// We keep the registration alive for the lifetime of the process —
    /// nothing currently calls `PowerUnregisterSuspendResumeNotification`.
    static REGISTERED: OnceLock<usize> = OnceLock::new();

    pub fn register<F>(handler: F)
    where
        F: Fn(PowerEvent) + Send + Sync + 'static,
    {
        if HANDLER.set(std::sync::Arc::new(handler)).is_err() {
            tracing::warn!(target: "power", "handler already registered, ignoring");
            return;
        }

        // Leak the params on the heap so the OS callback machinery can keep
        // a stable pointer for the lifetime of the process.
        let params = Box::leak(Box::new(DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS {
            Callback: Some(callback),
            Context: std::ptr::null_mut(),
        }));

        let mut handle = HANDLE::default();
        let rc = unsafe {
            PowerRegisterSuspendResumeNotification(
                DEVICE_NOTIFY_CALLBACK,
                HANDLE(params as *mut _ as *mut c_void),
                &mut handle as *mut _ as *mut _,
            )
        };
        if rc.0 != 0 {
            tracing::error!(target: "power",
                "PowerRegisterSuspendResumeNotification failed: code={}", rc.0);
            return;
        }
        let _ = REGISTERED.set(handle.0 as usize);
        tracing::info!(target: "power", "suspend/resume notifications registered");
    }

    unsafe extern "system" fn callback(
        _ctx: *const c_void,
        kind: u32,
        _setting: *const c_void,
    ) -> u32 {
        let evt = match kind {
            PBT_APMSUSPEND => Some(PowerEvent::Suspend),
            PBT_APMRESUMEAUTOMATIC => Some(PowerEvent::Resume),
            _ => None,
        };
        if let (Some(evt), Some(h)) = (evt, HANDLER.get()) {
            let h = h.clone();
            // Run off the OS callback thread to avoid blocking it.
            std::thread::spawn(move || h(evt));
        }
        0
    }
}

#[cfg(not(windows))]
mod imp {
    use super::PowerEvent;
    pub fn register<F>(_handler: F)
    where
        F: Fn(PowerEvent) + Send + Sync + 'static,
    {
        // no-op on non-Windows for now
    }
}

pub use imp::register;
