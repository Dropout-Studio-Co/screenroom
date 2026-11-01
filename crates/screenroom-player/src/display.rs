pub const REFERENCE_WHITE_NITS: u32 = 203;

pub const PQ_MAXIMUM_NITS: f32 = 10_000.0;

pub const PQ_M1: f32 = 0.159_301_76;
pub const PQ_M2: f32 = 78.843_75;
pub const PQ_C1: f32 = 0.835_937_5;
pub const PQ_C2: f32 = 18.851_562;
pub const PQ_C3: f32 = 18.687_5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayAssumption {
    Measured,

    Assumed,
}

impl DisplayAssumption {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Measured => "measured",
            Self::Assumed => "assumed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DisplayTarget {
    pub reference_white_nits: u32,

    pub headroom: f32,

    pub assumption: DisplayAssumption,
}

impl DisplayTarget {
    pub fn sdr() -> Self {
        Self {
            reference_white_nits: REFERENCE_WHITE_NITS,
            headroom: 1.0,
            assumption: DisplayAssumption::Assumed,
        }
    }

    pub fn with_headroom(headroom: f32) -> Self {
        let measured = headroom.is_finite() && headroom > 1.0;
        Self {
            reference_white_nits: REFERENCE_WHITE_NITS,
            headroom: if measured { headroom } else { 1.0 },
            assumption: if measured {
                DisplayAssumption::Measured
            } else {
                DisplayAssumption::Assumed
            },
        }
    }

    pub fn with_peak_nits(peak_nits: u32) -> Self {
        let headroom = peak_nits.max(1) as f32 / REFERENCE_WHITE_NITS as f32;
        Self {
            reference_white_nits: REFERENCE_WHITE_NITS,
            headroom,
            assumption: DisplayAssumption::Assumed,
        }
    }

    pub fn is_extended_range(&self) -> bool {
        self.headroom > 1.0
    }

    pub fn peak_nits(&self) -> u32 {
        (self.reference_white_nits as f32 * self.headroom).round() as u32
    }

    pub fn extended_linear_peak(&self) -> f32 {
        self.headroom.max(1.0)
    }

    pub fn label(&self) -> String {
        format!(
            "{} nits peak ({} x {} nits reference white, {})",
            self.peak_nits(),
            self.headroom,
            self.reference_white_nits,
            self.assumption.as_str()
        )
    }
}

impl Default for DisplayTarget {
    fn default() -> Self {
        Self::sdr()
    }
}

pub fn pq_to_relative_luminance(code: f32) -> f32 {
    if !code.is_finite() || code <= 0.0 {
        return 0.0;
    }
    let powered = code.clamp(0.0, 1.0).powf(1.0 / PQ_M2);
    let numerator = (powered - PQ_C1).max(0.0);
    let denominator = PQ_C2 - PQ_C3 * powered;
    if denominator <= 0.0 {
        return 1.0;
    }
    (numerator / denominator).powf(1.0 / PQ_M1)
}

pub fn relative_luminance_to_pq(luminance: f32) -> f32 {
    if !luminance.is_finite() || luminance <= 0.0 {
        return 0.0;
    }
    let value = luminance.clamp(0.0, 1.0).powf(PQ_M1);
    ((PQ_C1 + PQ_C2 * value) / (1.0 + PQ_C3 * value)).powf(PQ_M2)
}

pub fn pq_code_to_nits(code: f32) -> f32 {
    pq_to_relative_luminance(code) * PQ_MAXIMUM_NITS
}

pub fn pq_code_to_extended_linear(code: f32) -> f32 {
    pq_code_to_nits(code) / REFERENCE_WHITE_NITS as f32
}

pub fn extended_linear_to_nits(value: f32) -> f32 {
    value * REFERENCE_WHITE_NITS as f32
}

pub fn extended_range_target() -> DisplayTarget {
    match measured_headroom() {
        Some(headroom) => DisplayTarget::with_headroom(headroom),
        None => DisplayTarget::sdr(),
    }
}

#[cfg(target_os = "macos")]
fn measured_headroom() -> Option<f32> {
    let marker = objc2::MainThreadMarker::new()?;
    let screen = objc2_app_kit::NSScreen::mainScreen(marker)?;
    Some(screen.maximumPotentialExtendedDynamicRangeColorComponentValue() as f32)
}

#[cfg(not(target_os = "macos"))]
fn measured_headroom() -> Option<f32> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_white_is_the_broadcast_reference_level() {
        assert_eq!(REFERENCE_WHITE_NITS, 203);
        assert_eq!(PQ_MAXIMUM_NITS, 10_000.0);
    }

    #[test]
    fn an_assumed_display_carries_no_headroom() {
        let target = DisplayTarget::sdr();
        assert!(!target.is_extended_range());
        assert_eq!(target.headroom, 1.0);
        assert_eq!(target.peak_nits(), REFERENCE_WHITE_NITS);
        assert_eq!(target.assumption, DisplayAssumption::Assumed);
        assert!(target.label().contains("assumed"));
    }

    #[test]
    fn a_measured_headroom_sets_the_peak_and_the_extended_ceiling() {
        let target = DisplayTarget::with_headroom(2.0);
        assert!(target.is_extended_range());
        assert_eq!(target.peak_nits(), 406);
        assert_eq!(target.extended_linear_peak(), 2.0);
        assert_eq!(target.assumption, DisplayAssumption::Measured);
        assert!(target.label().contains("406 nits"));
    }

    #[test]
    fn a_headroom_that_is_not_above_white_is_treated_as_a_plain_sdr_display() {
        for headroom in [0.0, 1.0, -3.0, f32::NAN, f32::INFINITY] {
            let target = DisplayTarget::with_headroom(headroom);
            assert_eq!(target.headroom, 1.0, "headroom {headroom}");
            assert_eq!(target.assumption, DisplayAssumption::Assumed);
        }
    }

    #[test]
    fn an_explicit_peak_override_sets_the_headroom_it_stands_for() {
        let target = DisplayTarget::with_peak_nits(406);
        assert_eq!(target.peak_nits(), 406);
        assert_eq!(target.assumption, DisplayAssumption::Assumed);
        assert!((target.headroom - 2.0).abs() < 0.01);
        assert_eq!(DisplayTarget::with_peak_nits(0).peak_nits(), 1);
    }

    #[test]
    fn the_pq_curve_hits_its_documented_anchors() {
        assert_eq!(pq_to_relative_luminance(0.0), 0.0);
        assert!((pq_to_relative_luminance(1.0) - 1.0).abs() < 1e-6);
        assert!((pq_code_to_nits(1.0) - PQ_MAXIMUM_NITS).abs() < 0.1);

        let hundred = pq_to_relative_luminance(0.508_078);
        assert!(
            (hundred - 0.01).abs() < 2e-4,
            "code 0.508078 is 100 nits, got {}",
            hundred * PQ_MAXIMUM_NITS
        );

        let thousand = pq_to_relative_luminance(0.751_827);
        assert!(
            (thousand - 0.1).abs() < 2e-3,
            "code 0.751827 is 1000 nits, got {}",
            thousand * PQ_MAXIMUM_NITS
        );
    }

    #[test]
    fn the_pq_curve_is_monotonic_and_clamped() {
        let mut previous = -1.0;
        for step in 0..=1000 {
            let code = step as f32 / 1000.0;
            let luminance = pq_to_relative_luminance(code);
            assert!(luminance >= previous, "the curve must not fall at {code}");
            previous = luminance;
        }
        assert_eq!(pq_to_relative_luminance(-1.0), 0.0);
        assert_eq!(pq_to_relative_luminance(f32::NAN), 0.0);
        assert!((pq_to_relative_luminance(4.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn the_inverse_curve_round_trips_a_pq_code() {
        for code in [0.05f32, 0.2, 0.508_078, 0.751_827, 0.9] {
            let luminance = pq_to_relative_luminance(code);
            let back = relative_luminance_to_pq(luminance);
            assert!(
                (back - code).abs() < 1e-4,
                "code {code} round-tripped to {back}"
            );
        }
        assert_eq!(relative_luminance_to_pq(0.0), 0.0);
        assert_eq!(relative_luminance_to_pq(-1.0), 0.0);
    }

    #[test]
    fn a_reference_white_code_is_exactly_one_in_extended_linear_space() {
        let code = relative_luminance_to_pq(REFERENCE_WHITE_NITS as f32 / PQ_MAXIMUM_NITS);
        let extended = pq_code_to_extended_linear(code);
        assert!(
            (extended - 1.0).abs() < 1e-3,
            "reference white must be 1.0 in extended linear space, got {extended}"
        );
        assert!((extended_linear_to_nits(1.0) - REFERENCE_WHITE_NITS as f32).abs() < f32::EPSILON);
    }

    #[test]
    fn the_display_peak_lands_at_the_middle_of_the_extended_linear_range() {
        let code = relative_luminance_to_pq(406.0 / PQ_MAXIMUM_NITS);
        let extended = pq_code_to_extended_linear(code);
        assert!(
            (extended - 2.0).abs() < 1e-2,
            "406 nits must be 2.0 in extended linear space, got {extended}"
        );
    }

    #[test]
    fn an_extended_frame_code_reports_the_luminance_it_stands_for() {
        assert!((pq_code_to_extended_linear(0.5) * REFERENCE_WHITE_NITS as f32 - 92.5).abs() < 0.5);
        assert!(
            (pq_code_to_extended_linear(1.0) * REFERENCE_WHITE_NITS as f32 - 10_000.0).abs() < 1.0
        );
    }
}
