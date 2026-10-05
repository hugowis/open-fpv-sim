//! Table lookups.

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
