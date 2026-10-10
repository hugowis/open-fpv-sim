//! Table lookups.

/// Checks what [`linear`] relies on: at least one point, `x` strictly increasing, every value finite. Models check
/// their tables once, when they are built (config validation checks them too, but a model may be built from code).
pub fn check_table(table: &[(f64, f64)], what: &str) -> Result<(), String> {
    if table.is_empty() {
        return Err(format!("{what} is empty"));
    }
    if !table.windows(2).all(|w| w[1].0 > w[0].0) {
        return Err(format!("{what}: x must be strictly increasing"));
    }
    if !table.iter().all(|(x, y)| x.is_finite() && y.is_finite()) {
        return Err(format!("{what}: values must be finite"));
    }
    Ok(())
}

/// Piecewise-linear interpolation over `(x, y)` points sorted by `x`; clamps outside the range.
pub fn linear(table: &[(f64, f64)], x: f64) -> f64 {
    assert!(!table.is_empty(), "interpolation table is empty");
    if x <= table[0].0 {
        return table[0].1;
    }
    for w in table.windows(2) {
        let (x0, y0) = w[0];
        let (x1, y1) = w[1];
        if x <= x1 {
            if x1 == x0 {
                return y1;
            }
            return y0 + (y1 - y0) * (x - x0) / (x1 - x0);
        }
    }
    table[table.len() - 1].1
}
