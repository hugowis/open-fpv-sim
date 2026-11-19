//! The loss an object puts on a radio path, in dB: one number, or a curve across frequency. Both are measured
//! on the way in and out of a building, so they rise with frequency roughly as a power law.
use serde::Deserialize;

/// A loss of one number at every frequency, or a curve. Untagged: a TOML float or a table.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum RfLoss {
    Db(f64),
    Curve(RfLossCurve),
}

/// Losses in dB at frequency points in MHz, strictly increasing. Between points the loss follows a power law
/// (`ln dB` linear in `ln f`) where both ends are above 0 dB, else dB linearly in `ln f`; outside them it clamps.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RfLossCurve {
    pub at_mhz: Vec<f64>,
    pub db: Vec<f64>,
}

impl RfLoss {
    /// The loss in dB at `freq_mhz`.
    pub fn db_at(&self, freq_mhz: f64) -> f64 {
        let c = match self {
            RfLoss::Db(db) => return *db,
            RfLoss::Curve(c) => c,
        };
        let Some((&f0, &d0)) = c.at_mhz.first().zip(c.db.first()) else {
            return 0.0;
        };
        if freq_mhz <= f0 {
            return d0;
        }
        let (&f1, &d1) = c.at_mhz.last().zip(c.db.last()).expect("at_mhz and db have the same length");
        if freq_mhz >= f1 {
            return d1;
        }
        let i = c.at_mhz.partition_point(|&f| f <= freq_mhz);
        let ((fa, da), (fb, db)) = ((c.at_mhz[i - 1], c.db[i - 1]), (c.at_mhz[i], c.db[i]));
        let t = (freq_mhz / fa).ln() / (fb / fa).ln();
        if da > 0.0 && db > 0.0 {
            (da.ln() + t * (db / da).ln()).exp()
        } else {
            da + t * (db - da)
        }
    }

    /// The largest loss in dB: the scalar itself, or the largest listed dB.
    pub fn max_db(&self) -> f64 {
        match self {
            RfLoss::Db(db) => *db,
            RfLoss::Curve(c) => c.db.iter().copied().fold(0.0, f64::max),
        }
    }

    /// Whether the loss blocks the path anywhere (a positive loss at some frequency).
    pub fn is_obstacle(&self) -> bool {
        self.max_db() > 0.0
    }
}

impl From<f64> for RfLoss {
    fn from(db: f64) -> Self {
        RfLoss::Db(db)
    }
}

impl Default for RfLoss {
    fn default() -> Self {
        RfLoss::Db(0.0)
    }
}

impl RfLossCurve {
    /// At least two points, equal lengths, finite positive strictly increasing frequencies, finite non-negative dB.
    pub fn validate(&self) -> Result<(), String> {
        if self.at_mhz.len() < 2 {
            return Err(format!("an rf loss curve needs at least two points, got {}", self.at_mhz.len()));
        }
        if self.at_mhz.len() != self.db.len() {
            return Err(format!("rf loss curve has {} frequencies but {} losses", self.at_mhz.len(), self.db.len()));
        }
        for pair in self.at_mhz.windows(2) {
            if !(pair[0].is_finite() && pair[0] > 0.0 && pair[0] < pair[1]) {
                return Err(format!("rf loss curve frequencies must be finite, positive and increasing: {pair:?} MHz"));
            }
        }
        if !self.at_mhz.last().copied().unwrap_or(0.0).is_finite() {
            return Err("rf loss curve frequency is not finite".to_owned());
        }
        if let Some(&db) = self.db.iter().find(|db| !(db.is_finite() && **db >= 0.0)) {
            return Err(format!("rf loss curve losses must be finite and not negative, got {db} dB"));
        }
        Ok(())
    }
}
