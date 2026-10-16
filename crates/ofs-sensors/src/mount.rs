//! The flight controller's mount: a mass on a spring and damper between the frame and the IMU. Its transmissibility
//! `T(s) = (1 + 2ζ s/ωn) / (1 + 2ζ s/ωn + s²/ωn²)` is 1 at DC, peaks at `√(1+4ζ²)/(2ζ)` at `ωn` and rolls off at
//! 40 dB/decade above `√2·ωn`: a stiff mount passes the motors' vibration, a soft one cuts it.

/// Filter state below this magnitude is flushed to zero (the vibration terms are of order 1e-3 and up).
const STATE_FLOOR: f64 = 1e-30;

/// A mount's natural frequency and damping ratio (part data, `[frame.fc_mount]`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MountResponse {
    pub natural_hz: f64,
    pub damping_ratio: f64,
}

/// One axis of the transmissibility, discretised with a bilinear transform prewarped at the natural frequency
/// (direct form II transposed).
#[derive(Debug, Clone)]
pub struct SecondOrder {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

impl SecondOrder {
    /// The filter for sample interval `dt_s`. Needs `natural_hz` well below the sample rate (the config checks
    /// `base_hz / 10`); the gain at `natural_hz` is exact, the shape elsewhere is the bilinear transform's.
    pub fn transmissibility(m: MountResponse, dt_s: f64) -> Self {
        let wn = 2.0 * std::f64::consts::PI * m.natural_hz;
        let c = 1.0 / (wn * dt_s / 2.0).tan(); // K / ωn, with the prewarped K = ωn / tan(ωn·dt/2)
        let zc = 2.0 * m.damping_ratio * c;
        let a0 = 1.0 + zc + c * c;
        Self {
            b0: (1.0 + zc) / a0,
            b1: 2.0 / a0,
            b2: (1.0 - zc) / a0,
            a1: (2.0 - 2.0 * c * c) / a0,
            a2: (1.0 - zc + c * c) / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    pub fn step(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        // With no input the state would decay into an endless tiny oscillation among subnormal numbers rather than
        // reach zero; far below anything the IMU can resolve, flush it so `is_idle` can come true again.
        if self.z1.abs() < STATE_FLOOR && self.z2.abs() < STATE_FLOOR {
            self.z1 = 0.0;
            self.z2 = 0.0;
        }
        y
    }

    /// True while the filter holds no state at all (nothing has been fed through it, or it has fully decayed).
    pub fn is_idle(&self) -> bool {
        self.z1 == 0.0 && self.z2 == 0.0
    }
}
