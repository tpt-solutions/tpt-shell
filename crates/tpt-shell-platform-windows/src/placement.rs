// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Puts a saved window rectangle back on a monitor that exists now (todo M3, "multi-monitor").
//!
//! The rule in [`place`] is pure, so it is tested without a desktop. [`monitors`] and
//! [`device_of_window`] read the current layout through Win32; they are the only parts that
//! need one.

use std::ptr::{null, null_mut};

use windows_sys::core::BOOL;
use windows_sys::Win32::Foundation::{HWND, LPARAM, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, MonitorFromWindow, HDC, HMONITOR, MONITORINFO,
    MONITORINFOEXW, MONITOR_DEFAULTTONEAREST,
};

/// `MONITORINFOF_PRIMARY`: set in `dwFlags` for the primary monitor.
const MONITORINFOF_PRIMARY: u32 = 1;

/// A monitor as the placement rule sees it.
///
/// Support: Full.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Monitor {
    /// The device name, such as `\\.\DISPLAY1`. A saved placement names its monitor by this.
    pub device: String,
    /// The work area (left, top, right, bottom) in device pixels: the monitor less the taskbar.
    pub work: [i32; 4],
    /// Whether this is the primary monitor. A tie between two monitors goes to the primary one.
    pub primary: bool,
}

/// The area two rectangles (left, top, right, bottom) share. Zero when they do not overlap.
fn overlap(a: [i32; 4], b: [i32; 4]) -> i64 {
    let w = i64::from(a[2].min(b[2])) - i64::from(a[0].max(b[0]));
    let h = i64::from(a[3].min(b[3])) - i64::from(a[1].max(b[1]));
    if w > 0 && h > 0 {
        w * h
    } else {
        0
    }
}

/// The squared distance from the centre of `saved` to the nearest point of `work`. Zero when the
/// centre is inside the work area.
fn distance_sq(saved: [i32; 4], work: [i32; 4]) -> i64 {
    let cx = (i64::from(saved[0]) + i64::from(saved[2])) / 2;
    let cy = (i64::from(saved[1]) + i64::from(saved[3])) / 2;
    let dx = (i64::from(work[0]) - cx)
        .max(0)
        .max(cx - i64::from(work[2]));
    let dy = (i64::from(work[1]) - cy)
        .max(0)
        .max(cy - i64::from(work[3]));
    dx * dx + dy * dy
}

/// Moves a rectangle onto `work`. Its size is kept unless the work area is smaller, in which
/// case it shrinks to fit. The position is clamped, so the window keeps its place relative to the
/// nearest edge.
fn move_into(saved: [i32; 4], work: [i32; 4]) -> [i32; 4] {
    let area_w = (work[2] - work[0]).max(0);
    let area_h = (work[3] - work[1]).max(0);
    let w = (saved[2] - saved[0]).max(0).min(area_w);
    let h = (saved[3] - saved[1]).max(0).min(area_h);
    let left = saved[0].max(work[0]).min(work[2] - w);
    let top = saved[1].max(work[1]).min(work[3] - h);
    [left, top, left + w, top + h]
}

/// Whether a rectangle is no larger than a work area in either direction.
fn fits(saved: [i32; 4], work: [i32; 4]) -> bool {
    saved[2] - saved[0] <= work[2] - work[0] && saved[3] - saved[1] <= work[3] - work[1]
}

/// Chooses where a saved rectangle (left, top, right, bottom) goes.
///
/// A rectangle that overlaps a monitor is kept where it is, even partly off that monitor, as long
/// as it fits the monitor's work area. A larger one shrinks to fit the monitor it overlaps most.
/// A rectangle on no monitor goes to the monitor named by `remembered`, when that monitor is still
/// there. If it is not, it goes to the nearest monitor, and a tie goes to the primary one. With no
/// monitors at all, the rectangle is returned unchanged.
///
/// Support: Full. Unit tested.
pub fn place(saved: [i32; 4], remembered: Option<&str>, monitors: &[Monitor]) -> [i32; 4] {
    if monitors.is_empty() {
        return saved;
    }
    let visible = monitors
        .iter()
        .filter(|m| overlap(saved, m.work) > 0)
        .max_by_key(|m| (overlap(saved, m.work), m.primary));
    if let Some(m) = visible {
        return if fits(saved, m.work) {
            saved
        } else {
            move_into(saved, m.work)
        };
    }
    let target = remembered
        .and_then(|name| monitors.iter().find(|m| m.device == name))
        .or_else(|| {
            monitors
                .iter()
                .min_by_key(|m| (distance_sq(saved, m.work), !m.primary))
        });
    match target {
        Some(m) => move_into(saved, m.work),
        None => saved,
    }
}

/// Reads one monitor's device name, work area and primary flag. `None` if Windows refuses.
fn info_of(monitor: HMONITOR) -> Option<Monitor> {
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    // SAFETY: `MONITORINFOEXW` starts with a `MONITORINFO` and `cbSize` is the extended size, which
    // is what the call expects for the extended form.
    let ok = unsafe {
        GetMonitorInfoW(
            monitor,
            &mut info as *mut MONITORINFOEXW as *mut MONITORINFO,
        )
    };
    if ok == 0 {
        return None;
    }
    let len = info
        .szDevice
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(info.szDevice.len());
    let w = info.monitorInfo.rcWork;
    Some(Monitor {
        device: String::from_utf16_lossy(&info.szDevice[..len]),
        work: [w.left, w.top, w.right, w.bottom],
        primary: info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
    })
}

/// The monitors that exist now. Empty if Windows reports none (for example, with no desktop).
///
/// Support: Partial. The layout is read when this is called, so a change made while the app runs
/// is seen at the next placement, not as it happens.
pub fn monitors() -> Vec<Monitor> {
    unsafe extern "system" fn collect(
        monitor: HMONITOR,
        _dc: HDC,
        _clip: *mut RECT,
        data: LPARAM,
    ) -> BOOL {
        // SAFETY: `data` is the address of the vector `monitors` passed in, and it is alive for
        // the whole call, since EnumDisplayMonitors runs the callback synchronously.
        let found = unsafe { &mut *(data as *mut Vec<HMONITOR>) };
        found.push(monitor);
        1
    }
    let mut handles: Vec<HMONITOR> = Vec::new();
    // SAFETY: no clip rectangle and no device context: all monitors. The callback above only
    // runs during this call, while `handles` is alive.
    unsafe {
        EnumDisplayMonitors(
            null_mut(),
            null(),
            Some(collect),
            &mut handles as *mut Vec<HMONITOR> as LPARAM,
        )
    };
    handles.into_iter().filter_map(info_of).collect()
}

/// The device name of the monitor a window is on now, or the nearest one if it is off screen.
/// `window` is the window's `HWND` value, as [`crate::backend::WindowId`] holds it.
///
/// Support: Partial. Returns `None` when Windows refuses the monitor lookup.
pub fn device_of_window(window: isize) -> Option<String> {
    // SAFETY: MonitorFromWindow takes a window handle by value; an invalid one gives null.
    let monitor = unsafe { MonitorFromWindow(window as HWND, MONITOR_DEFAULTTONEAREST) };
    info_of(monitor).map(|m| m.device)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(device: &str, work: [i32; 4], primary: bool) -> Monitor {
        Monitor {
            device: device.to_owned(),
            work,
            primary,
        }
    }

    const PRIMARY: [i32; 4] = [0, 0, 1920, 1040];
    const RIGHT: [i32; 4] = [1920, 0, 3840, 1040];

    #[test]
    fn a_window_on_a_monitor_keeps_its_saved_place() {
        let monitors = [monitor("A", PRIMARY, true)];
        assert_eq!(
            place([100, 100, 900, 700], None, &monitors),
            [100, 100, 900, 700]
        );
    }

    #[test]
    fn a_window_partly_on_a_monitor_is_not_moved() {
        let monitors = [monitor("A", PRIMARY, true)];
        assert_eq!(
            place([1800, 100, 2600, 700], None, &monitors),
            [1800, 100, 2600, 700]
        );
    }

    #[test]
    fn a_window_off_every_monitor_moves_onto_the_nearest_one_and_keeps_its_size() {
        let monitors = [monitor("A", PRIMARY, true)];
        // 800 x 600 saved past the right edge: clamped to the right edge, same top.
        assert_eq!(
            place([3000, 100, 3800, 700], None, &monitors),
            [1120, 100, 1920, 700]
        );
    }

    #[test]
    fn the_nearest_monitor_is_chosen_when_nothing_is_remembered() {
        let monitors = [monitor("A", PRIMARY, true), monitor("B", RIGHT, false)];
        // Off screen, far below both monitors but closer to B's horizontal range.
        assert_eq!(
            place([4000, 2000, 4800, 2600], None, &monitors),
            [3040, 440, 3840, 1040]
        );
    }

    #[test]
    fn a_window_larger_than_the_work_area_shrinks_to_fit_it() {
        let monitors = [monitor("A", PRIMARY, true)];
        assert_eq!(
            place([-5000, -5000, 5000, 5000], None, &monitors),
            [0, 0, 1920, 1040]
        );
    }

    #[test]
    fn a_remembered_monitor_is_preferred_over_the_nearest_one_when_it_is_present() {
        let monitors = [monitor("A", PRIMARY, true), monitor("B", RIGHT, false)];
        // Centre is nearer B, but the placement was saved on A, so it goes to A.
        assert_eq!(
            place([4000, 100, 4800, 700], Some("A"), &monitors),
            [1120, 100, 1920, 700]
        );
        assert_eq!(
            place([4000, 100, 4800, 700], None, &monitors),
            [3040, 100, 3840, 700]
        );
    }

    #[test]
    fn a_remembered_monitor_that_is_gone_falls_back_to_the_nearest_one() {
        let monitors = [monitor("A", PRIMARY, true), monitor("B", RIGHT, false)];
        assert_eq!(
            place([4000, 100, 4800, 700], Some("C"), &monitors),
            [3040, 100, 3840, 700]
        );
    }

    #[test]
    fn a_tie_goes_to_the_primary_monitor() {
        // The rectangle's centre is 360 px below the primary monitor and 360 px above the other
        // one, so both are equally near. The primary wins even though it is listed second.
        let below = [0, 1760, 1920, 2800];
        let monitors = [monitor("Q", below, false), monitor("P", PRIMARY, true)];
        assert_eq!(
            place([0, 1200, 400, 1600], None, &monitors),
            [0, 640, 400, 1040]
        );
    }

    #[test]
    fn no_monitors_leaves_the_rectangle_alone() {
        assert_eq!(
            place([3000, 100, 3800, 700], Some("A"), &[]),
            [3000, 100, 3800, 700]
        );
    }

    #[test]
    fn the_monitor_query_runs_without_a_desktop() {
        // Headless machines report no monitors; that is a valid answer, not a failure. When there
        // are monitors, exactly the ones this process can see are listed, with one primary.
        let found = monitors();
        if found.is_empty() {
            eprintln!("skipped: no monitors are reported on this machine");
            return;
        }
        assert!(
            found.iter().any(|m| m.primary),
            "a desktop has a primary monitor"
        );
        for m in &found {
            assert!(m.work[2] > m.work[0] && m.work[3] > m.work[1], "{m:?}");
        }
    }
}
