// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Local time of day, for the supervisor's scheduled restart (spec §10.1). The schedule is in
//! the user's local time, which std does not provide.

use windows_sys::Win32::Foundation::SYSTEMTIME;
use windows_sys::Win32::System::SystemInformation::GetLocalTime;

/// The current minute of the day in local time, 0 to 1439.
pub fn minute_of_day() -> u16 {
    // SAFETY: SYSTEMTIME is plain old data, so the zeroed value is valid before GetLocalTime
    // fills it in.
    let mut now: SYSTEMTIME = unsafe { std::mem::zeroed() };
    // SAFETY: `now` is a valid out-pointer for the call.
    unsafe { GetLocalTime(&mut now) };
    now.wHour * 60 + now.wMinute
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_minute_is_within_the_day() {
        assert!(minute_of_day() < 24 * 60);
    }
}
