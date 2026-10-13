//! Physical constants (v1: fixed sea-level standard atmosphere).
pub const GRAVITY_MPS2: f64 = 9.80665;
pub const AIR_DENSITY_KGPM3: f64 = 1.225;

/// One analog (PAL) video field: the video link model runs once per field, and the goggles' picture ages a field
/// at a time. The single source for every crate that needs it (m3e debt: it was written down in three places).
pub const VIDEO_FIELD_RATE_HZ: u32 = 50;
/// The same field as a period, s.
pub const VIDEO_FIELD_PERIOD_S: f64 = 0.02;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_field_period_is_the_field_rate_inverted() {
        assert_eq!(VIDEO_FIELD_PERIOD_S * f64::from(VIDEO_FIELD_RATE_HZ), 1.0);
    }
}
