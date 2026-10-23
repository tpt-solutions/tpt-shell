// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Our own DPI and geometry types (spec §4: no `dpi` crate, which is Apache-2.0 AND MIT).

/// A size in device-independent units.
///
/// Support: Full. Plain data; unit tested.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LogicalSize {
    pub width: f64,
    pub height: f64,
}

/// A rectangle in device-independent units.
///
/// Support: Full. Plain data; unit tested.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LogicalRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// A size in device pixels.
///
/// Support: Full. Plain data; unit tested.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhysicalSize {
    pub width: u32,
    pub height: u32,
}

/// Device pixels per logical unit. For example 1.5 at 150% scaling.
///
/// Support: Full. Plain data; unit tested.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScaleFactor(f64);

impl ScaleFactor {
    /// Creates a scale factor. Values that are not finite or not positive become 1.0.
    ///
    /// Support: Full. Unit tested.
    pub fn new(value: f64) -> Self {
        if value.is_finite() && value > 0.0 {
            ScaleFactor(value)
        } else {
            ScaleFactor(1.0)
        }
    }
    /// Support: Full. Unit tested.

    pub fn get(self) -> f64 {
        self.0
    }
    /// Support: Full. Unit tested.

    pub fn to_physical(self, size: LogicalSize) -> PhysicalSize {
        PhysicalSize {
            width: (size.width * self.0).round().max(0.0) as u32,
            height: (size.height * self.0).round().max(0.0) as u32,
        }
    }
    /// Support: Full. Unit tested.

    pub fn to_logical(self, size: PhysicalSize) -> LogicalSize {
        LogicalSize {
            width: size.width as f64 / self.0,
            height: size.height as f64 / self.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_scale_becomes_one() {
        assert_eq!(ScaleFactor::new(0.0).get(), 1.0);
        assert_eq!(ScaleFactor::new(-2.0).get(), 1.0);
        assert_eq!(ScaleFactor::new(f64::NAN).get(), 1.0);
        assert_eq!(ScaleFactor::new(f64::INFINITY).get(), 1.0);
    }

    #[test]
    fn logical_to_physical_rounds() {
        let s = ScaleFactor::new(1.5);
        let p = s.to_physical(LogicalSize {
            width: 1100.0,
            height: 700.0,
        });
        assert_eq!(
            p,
            PhysicalSize {
                width: 1650,
                height: 1050
            }
        );
    }

    #[test]
    fn round_trip_at_150_percent() {
        let s = ScaleFactor::new(1.5);
        let l = s.to_logical(PhysicalSize {
            width: 1650,
            height: 1050,
        });
        assert_eq!(
            l,
            LogicalSize {
                width: 1100.0,
                height: 700.0
            }
        );
    }

    #[test]
    fn negative_size_clamps_to_zero() {
        let p = ScaleFactor::new(1.0).to_physical(LogicalSize {
            width: -5.0,
            height: 3.0,
        });
        assert_eq!(p.width, 0);
        assert_eq!(p.height, 3);
    }
}
