// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 TPT Solutions

//! Screenshot comparison (spec §10.6).
//!
//! Each pixel pair gets a distance from 0 (the same) to 1 (black against white). The distance
//! weights luminance more than colour, using the YIQ weights of Kotsarenko and Ramos, so a
//! small change in brightness counts for more than the same change in hue. Transparent pixels
//! are composited over white first. A pixel is "changed" when its distance is above the
//! tolerance for its region. A comparison passes when the changed share is at most
//! `max_changed_fraction`.
//!
//! This is not CIEDE2000, and it has not been tuned against human judgement. The tolerance
//! and the threshold are the knobs to set per screen.
//!
//! Support: Full for the comparison itself. Screenshots are read from PNG files by
//! [`crate::png`], and [`crate::screens`] merges the files from several CI jobs.

/// An RGBA image: 8 bits per channel, rows from the top, no padding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl Image {
    /// Wraps pixels that are `width * height * 4` bytes long.
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Result<Self, String> {
        let expected = u64::from(width) * u64::from(height) * 4;
        if rgba.len() as u64 != expected {
            return Err(format!(
                "{width}x{height} RGBA needs {expected} bytes, got {}",
                rgba.len()
            ));
        }
        Ok(Image {
            width,
            height,
            rgba,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// The pixels, 8 bits per channel, rows from the top.
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = (y as usize * self.width as usize + x as usize) * 4;
        [
            self.rgba[i],
            self.rgba[i + 1],
            self.rgba[i + 2],
            self.rgba[i + 3],
        ]
    }
}

/// What to do with the pixels of a region.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    /// Leave the pixels out of the comparison, such as a clock or an animated ad.
    Ignore,
    /// Compare with this tolerance instead of the default.
    Tolerance(f64),
}

/// A rectangle of pixels with its own action. Where regions overlap, the last one listed wins.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Region {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub action: Action,
}

impl Region {
    fn contains(&self, x: u32, y: u32) -> bool {
        x >= self.x && y >= self.y && x - self.x < self.width && y - self.y < self.height
    }
}

/// How two screenshots are compared.
#[derive(Clone, Debug, PartialEq)]
pub struct Comparison {
    /// Largest distance a pixel may have and still count as unchanged, from 0 to 1.
    pub tolerance: f64,
    /// Largest share of compared pixels that may change, from 0 to 1.
    pub max_changed_fraction: f64,
    pub regions: Vec<Region>,
}

impl Default for Comparison {
    fn default() -> Self {
        Comparison {
            tolerance: 0.0,
            max_changed_fraction: 0.0,
            regions: Vec::new(),
        }
    }
}

/// The result of one comparison.
#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    /// Pixels that took part, which excludes ignored regions.
    pub compared: u64,
    /// Compared pixels whose distance is above their tolerance.
    pub changed: u64,
    /// Largest distance seen among the compared pixels.
    pub max_distance: f64,
    /// Whether the changed share is within the threshold.
    pub passed: bool,
}

impl Report {
    /// The changed share of the compared pixels. Zero when nothing was compared.
    pub fn changed_fraction(&self) -> f64 {
        if self.compared == 0 {
            0.0
        } else {
            self.changed as f64 / self.compared as f64
        }
    }
}

/// Compares `actual` with `baseline`. The two must have the same size.
pub fn compare(baseline: &Image, actual: &Image, cfg: &Comparison) -> Result<Report, String> {
    if baseline.width != actual.width || baseline.height != actual.height {
        return Err(format!(
            "the images differ in size: {}x{} against {}x{}",
            baseline.width, baseline.height, actual.width, actual.height
        ));
    }
    let mut compared = 0u64;
    let mut changed = 0u64;
    let mut max_distance = 0.0f64;
    for y in 0..baseline.height {
        for x in 0..baseline.width {
            let action = cfg
                .regions
                .iter()
                .rev()
                .find(|r| r.contains(x, y))
                .map_or(Action::Tolerance(cfg.tolerance), |r| r.action);
            let tolerance = match action {
                Action::Ignore => continue,
                Action::Tolerance(t) => t,
            };
            let d = distance(baseline.pixel(x, y), actual.pixel(x, y));
            compared += 1;
            max_distance = max_distance.max(d);
            if d > tolerance {
                changed += 1;
            }
        }
    }
    let report = Report {
        compared,
        changed,
        max_distance,
        passed: false,
    };
    let passed = report.changed_fraction() <= cfg.max_changed_fraction;
    Ok(Report { passed, ..report })
}

/// The weighted sum for black against white, which is a full luminance step. Distances are
/// scaled so that this pair is 1. Colour differences can reach 1 sooner and are clamped there.
const BLACK_TO_WHITE: f64 = 0.5053;

/// Distance from 0 to 1 between two pixels.
fn distance(a: [u8; 4], b: [u8; 4]) -> f64 {
    let (ya, ia, qa) = yiq(a);
    let (yb, ib, qb) = yiq(b);
    let (dy, di, dq) = (ya - yb, ia - ib, qa - qb);
    let weighted = 0.5053 * dy * dy + 0.299 * di * di + 0.1957 * dq * dq;
    (weighted / BLACK_TO_WHITE).sqrt().min(1.0)
}

/// Luminance, in-phase and quadrature of a pixel composited over white.
fn yiq(p: [u8; 4]) -> (f64, f64, f64) {
    let alpha = f64::from(p[3]) / 255.0;
    let channel = |c: u8| f64::from(c) / 255.0 * alpha + (1.0 - alpha);
    let (r, g, b) = (channel(p[0]), channel(p[1]), channel(p[2]));
    (
        0.299 * r + 0.587 * g + 0.114 * b,
        0.596 * r - 0.274 * g - 0.322 * b,
        0.211 * r - 0.523 * g + 0.312 * b,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(width: u32, height: u32, px: [u8; 4]) -> Image {
        let rgba = px.repeat((width * height) as usize);
        Image::new(width, height, rgba).unwrap()
    }

    /// Sets one pixel of `img`.
    fn paint(img: &mut Image, x: u32, y: u32, px: [u8; 4]) {
        let i = (y as usize * img.width as usize + x as usize) * 4;
        img.rgba[i..i + 4].copy_from_slice(&px);
    }

    const WHITE: [u8; 4] = [255, 255, 255, 255];
    const BLACK: [u8; 4] = [0, 0, 0, 255];

    #[test]
    fn an_image_needs_exactly_its_pixels() {
        assert!(Image::new(2, 2, vec![0; 15]).is_err());
        assert!(Image::new(2, 2, vec![0; 16]).is_ok());
    }

    #[test]
    fn identical_images_pass_with_no_distance() {
        let a = solid(4, 4, [10, 20, 30, 255]);
        let report = compare(&a, &a, &Comparison::default()).unwrap();
        assert_eq!(report.compared, 16);
        assert_eq!(report.changed, 0);
        assert_eq!(report.max_distance, 0.0);
        assert!(report.passed);
    }

    #[test]
    fn black_against_white_is_the_largest_distance() {
        let report = compare(
            &solid(1, 1, WHITE),
            &solid(1, 1, BLACK),
            &Comparison::default(),
        )
        .unwrap();
        assert!(
            (report.max_distance - 1.0).abs() < 1e-9,
            "{}",
            report.max_distance
        );
    }

    #[test]
    fn transparent_pixels_compare_as_white() {
        let red = solid(2, 2, [255, 0, 0, 0]);
        let blue = solid(2, 2, [0, 0, 255, 0]);
        let report = compare(&red, &blue, &Comparison::default()).unwrap();
        assert_eq!(report.max_distance, 0.0);
    }

    #[test]
    fn the_threshold_decides_the_result() {
        let baseline = solid(4, 4, WHITE);
        let mut actual = baseline.clone();
        paint(&mut actual, 0, 0, BLACK);
        let strict = Comparison::default();
        let report = compare(&baseline, &actual, &strict).unwrap();
        assert_eq!(report.changed, 1);
        assert!(!report.passed);

        let lenient = Comparison {
            max_changed_fraction: 1.0 / 16.0,
            ..Comparison::default()
        };
        assert!(compare(&baseline, &actual, &lenient).unwrap().passed);
    }

    #[test]
    fn an_ignored_region_leaves_its_pixels_out() {
        let baseline = solid(4, 4, WHITE);
        let mut actual = baseline.clone();
        paint(&mut actual, 3, 3, BLACK);
        let cfg = Comparison {
            regions: vec![Region {
                x: 2,
                y: 2,
                width: 2,
                height: 2,
                action: Action::Ignore,
            }],
            ..Comparison::default()
        };
        let report = compare(&baseline, &actual, &cfg).unwrap();
        assert_eq!(report.compared, 12);
        assert_eq!(report.changed, 0);
        assert!(report.passed);
    }

    #[test]
    fn a_region_tolerance_applies_only_inside_the_region() {
        let baseline = solid(4, 1, WHITE);
        let slight = [254, 254, 254, 255];
        let mut actual = baseline.clone();
        paint(&mut actual, 0, 0, slight);
        paint(&mut actual, 3, 0, slight);
        let cfg = Comparison {
            regions: vec![Region {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
                action: Action::Tolerance(0.05),
            }],
            ..Comparison::default()
        };
        let report = compare(&baseline, &actual, &cfg).unwrap();
        assert_eq!(report.changed, 1, "only the pixel outside the region");
    }

    #[test]
    fn mismatched_sizes_are_refused() {
        let a = solid(2, 2, WHITE);
        let b = solid(3, 2, WHITE);
        assert!(compare(&a, &b, &Comparison::default()).is_err());
    }

    #[test]
    fn the_report_fraction_is_zero_when_nothing_was_compared() {
        let report = Report {
            compared: 0,
            changed: 0,
            max_distance: 0.0,
            passed: true,
        };
        assert_eq!(report.changed_fraction(), 0.0);
    }
}
