//! Which signals a recording carries, at what rate: patterns resolved once against the bus's names at start.
use ofs_core::SignalKind;

use crate::glob;

/// The default table (spec §4): every signal class has a rate; the first matching row wins.
pub const DEFAULT_RATES: &[(&str, f64)] = &[
    ("body.*", 240.0),
    ("imu.*", 1000.0),
    ("motor.*", 240.0),
    ("esc.*", 240.0),
    ("prop.*", 240.0),
    ("battery.*", 10.0),
    ("radio.*", 50.0),
    ("video.*", 50.0),
    ("rc.*", 50.0),
    ("fault.*", 50.0),
    ("*", 50.0),
];

/// One requested pattern. `rate_hz: None` takes each matched signal's rate from [`DEFAULT_RATES`].
#[derive(Debug, Clone, PartialEq)]
pub struct SignalRate {
    pub pattern: String,
    pub rate_hz: Option<f64>,
}

/// A recorded signal: its bus name and kind, and the base-tick divisor its rate became.
#[derive(Debug, Clone, PartialEq)]
pub struct Channel {
    pub name: String,
    pub kind: SignalKind,
    /// Recorded on ticks whose end time is a multiple of `divisor` base ticks.
    pub divisor: u64,
    /// The rate actually recorded: `base_hz / divisor`.
    pub rate_hz: f64,
}

/// The default table's rate for `name`.
pub fn default_rate(name: &str) -> f64 {
    DEFAULT_RATES.iter().find(|(p, _)| glob::matches(p, name)).map_or(50.0, |&(_, r)| r)
}

/// Up to five signal names that share a literal piece of `pattern` (its dot-separated parts with the wildcards
/// removed): the likely intended signals when a pattern matches nothing. Names sharing more pieces come first, then
/// names that start with the pattern's first piece, then name order.
pub fn near_misses<'a>(pattern: &str, names: impl Iterator<Item = &'a str>) -> Vec<&'a str> {
    let parts: Vec<String> =
        pattern.split('.').map(|p| p.replace(['*', '?'], "")).filter(|p| !p.is_empty()).collect();
    let mut scored: Vec<(usize, bool, &str)> = names
        .map(|n| (parts.iter().filter(|p| n.contains(p.as_str())).count(), parts.first().is_some_and(|p| n.starts_with(p.as_str())), n))
        .filter(|&(shared, _, _)| shared > 0)
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)).then(a.2.cmp(b.2)));
    scored.into_iter().take(5).map(|(_, _, n)| n).collect()
}

/// Resolves `requests` against the bus's `signals` (name order). No requests: every signal at its default rate.
/// Otherwise each signal takes the first request that matches it, and signals no request matches are not
/// recorded. Every request must match at least one signal; every rate must be finite, > 0 and at most `base_hz`.
/// All problems are collected into one message.
pub fn resolve(requests: &[SignalRate], signals: &[(String, SignalKind)], base_hz: u32) -> Result<Vec<Channel>, String> {
    let mut problems = Vec::new();
    for r in requests {
        if let Some(rate) = r.rate_hz {
            if !rate.is_finite() || rate <= 0.0 || rate > f64::from(base_hz) {
                problems.push(format!("pattern '{}': rate_hz must be in (0, {base_hz}] (got {rate})", r.pattern));
            }
        }
        if !signals.iter().any(|(n, _)| glob::matches(&r.pattern, n)) {
            let near = near_misses(&r.pattern, signals.iter().map(|(n, _)| n.as_str()));
            let hint = if near.is_empty() { String::new() } else { format!(" (similar: {})", near.join(", ")) };
            problems.push(format!("pattern '{}' matches no signal{hint}", r.pattern));
        }
    }
    if !problems.is_empty() {
        return Err(problems.join("; "));
    }
    let channels = signals
        .iter()
        .filter_map(|(name, kind)| {
            let rate = if requests.is_empty() {
                default_rate(name)
            } else {
                let r = requests.iter().find(|r| glob::matches(&r.pattern, name))?;
                r.rate_hz.unwrap_or_else(|| default_rate(name))
            };
            let divisor = ((f64::from(base_hz) / rate).round() as u64).max(1);
            Some(Channel { name: name.clone(), kind: *kind, divisor, rate_hz: f64::from(base_hz) / divisor as f64 })
        })
        .collect();
    Ok(channels)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bus_names() -> Vec<(String, SignalKind)> {
        [
            ("battery.voltage_v", SignalKind::Scalar),
            ("body.att_q", SignalKind::Quat),
            ("body.pos_ned_m", SignalKind::Vec3),
            ("fault.imu.gyro_bias_radps", SignalKind::Vec3),
            ("fc.motor.0.cmd", SignalKind::Scalar),
            ("imu.gyro_frd_radps", SignalKind::Vec3),
            ("motor.0.omega_radps", SignalKind::Scalar),
            ("motor.1.omega_radps", SignalKind::Scalar),
        ]
        .into_iter()
        .map(|(n, k)| (n.to_string(), k))
        .collect()
    }

    fn req(pattern: &str, rate_hz: Option<f64>) -> SignalRate {
        SignalRate { pattern: pattern.into(), rate_hz }
    }

    #[test]
    fn no_requests_record_everything_at_the_default_table_rates() {
        let ch = resolve(&[], &bus_names(), 8000).unwrap();
        let rates: Vec<(&str, u64)> = ch.iter().map(|c| (c.name.as_str(), c.divisor)).collect();
        assert_eq!(
            rates,
            [
                ("battery.voltage_v", 800),
                ("body.att_q", 33),
                ("body.pos_ned_m", 33),
                ("fault.imu.gyro_bias_radps", 160),
                ("fc.motor.0.cmd", 160),
                ("imu.gyro_frd_radps", 8),
                ("motor.0.omega_radps", 33),
                ("motor.1.omega_radps", 33),
            ]
        );
        assert!((ch[1].rate_hz - 8000.0 / 33.0).abs() < 1e-12, "the actual rate is base / divisor");
    }

    #[test]
    fn requests_select_signals_and_the_first_match_sets_the_rate() {
        let ch = resolve(&[req("motor.0.*", Some(1000.0)), req("motor.*", None)], &bus_names(), 8000).unwrap();
        let rates: Vec<(&str, u64)> = ch.iter().map(|c| (c.name.as_str(), c.divisor)).collect();
        assert_eq!(rates, [("motor.0.omega_radps", 8), ("motor.1.omega_radps", 33)]);
    }

    #[test]
    fn a_pattern_matching_nothing_names_its_near_misses_and_all_problems_are_collected() {
        let err = resolve(&[req("gyro.*", None), req("battery.voltage", None), req("imu.*", Some(0.0))], &bus_names(), 8000)
            .unwrap_err();
        assert!(err.contains("pattern 'gyro.*' matches no signal (similar: fault.imu.gyro_bias_radps, imu.gyro_frd_radps)"), "{err}");
        assert!(err.contains("pattern 'battery.voltage' matches no signal (similar: battery.voltage_v)"), "{err}");
        assert!(err.contains("pattern 'imu.*': rate_hz must be in (0, 8000] (got 0)"), "{err}");
        let rpm = resolve(&[req("motor.*.rpm", None)], &bus_names(), 8000).unwrap_err();
        assert!(rpm.contains("(similar: motor.0.omega_radps, motor.1.omega_radps, fc.motor.0.cmd)"), "the first piece's names come first: {rpm}");
        let err = resolve(&[req("zzz", None)], &bus_names(), 8000).unwrap_err();
        assert_eq!(err, "pattern 'zzz' matches no signal");
    }

    #[test]
    fn a_rate_above_the_base_rate_is_refused_and_a_fast_rate_records_every_tick() {
        assert!(resolve(&[req("imu.*", Some(8001.0))], &bus_names(), 8000).is_err());
        let ch = resolve(&[req("imu.*", Some(8000.0))], &bus_names(), 8000).unwrap();
        assert_eq!(ch[0].divisor, 1);
    }
}
