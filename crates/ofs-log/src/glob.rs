//! Signal-name patterns: `*` matches any run of characters (dots included), `?` exactly one, anything else
//! itself. The Python reader (`ofs.reader`) uses the same rules (`fnmatch.fnmatchcase` without brackets).

/// Whether `name` matches `pattern` as a whole.
pub fn matches(pattern: &str, name: &str) -> bool {
    let (p, n): (Vec<char>, Vec<char>) = (pattern.chars().collect(), name.chars().collect());
    let (mut pi, mut ni) = (0, 0);
    // The last `*` seen and the name position it was tried at: on a mismatch, let it swallow one more character.
    let mut star: Option<(usize, usize)> = None;
    while ni < n.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == n[ni]) {
            pi += 1;
            ni += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some((pi, ni));
            pi += 1;
        } else if let Some((sp, sn)) = star {
            pi = sp + 1;
            ni = sn + 1;
            star = Some((sp, sn + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

#[cfg(test)]
mod tests {
    use super::matches;

    #[test]
    fn star_spans_dots_and_question_mark_is_one_character() {
        assert!(matches("imu.*", "imu.gyro_frd_radps"));
        assert!(matches("motor.*.omega_radps", "motor.3.omega_radps"));
        assert!(matches("*", "anything.at.all"));
        assert!(matches("motor.?.current_a", "motor.0.current_a"));
        assert!(!matches("motor.?.current_a", "motor.10.current_a"));
        assert!(matches("motor.*.current_a", "motor.10.current_a"));
        assert!(!matches("imu.*", "fault.imu.gyro_bias_radps"), "anchored at the start");
        assert!(!matches("battery.voltage", "battery.voltage_v"), "anchored at the end");
        assert!(matches("battery.voltage_v", "battery.voltage_v"));
        assert!(matches("*.theta_rad", "motor.2.theta_rad"));
        assert!(matches("a*b*c", "aXXbYYc"));
        assert!(!matches("a*b*c", "aXXbYY"));
        assert!(matches("", "") && !matches("", "x"));
    }
}
