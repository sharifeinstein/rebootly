//! Keep the PC awake during a job; battery check (SPEC §3.5).
#![allow(unsafe_code)]

use windows_sys::Win32::System::Power::{
    ES_CONTINUOUS, ES_SYSTEM_REQUIRED, GetSystemPowerStatus, SYSTEM_POWER_STATUS,
    SetThreadExecutionState,
};

/// Prevents system sleep while alive. Create and drop it on the same thread
/// (the execution state is per thread).
#[derive(Debug)]
pub struct SleepGuard {
    _not_send: std::marker::PhantomData<*const ()>,
}

impl SleepGuard {
    pub fn new() -> Self {
        // SAFETY: plain flag call.
        unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED) };
        SleepGuard {
            _not_send: std::marker::PhantomData,
        }
    }
}

impl Default for SleepGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for SleepGuard {
    fn drop(&mut self) {
        // SAFETY: plain flag call; restores normal sleep behaviour.
        unsafe { SetThreadExecutionState(ES_CONTINUOUS) };
    }
}

/// `true` when running on battery (a laptop unplugged).
pub fn on_battery() -> bool {
    // SAFETY: all-zero is a valid SYSTEM_POWER_STATUS.
    let mut s: SYSTEM_POWER_STATUS = unsafe { std::mem::zeroed() };
    // SAFETY: valid out-pointer.
    if unsafe { GetSystemPowerStatus(&mut s) } == 0 {
        return false;
    }
    // ACLineStatus 0 = offline; BatteryFlag 128 = no system battery.
    s.ACLineStatus == 0 && s.BatteryFlag != 128
}
