//! A built-in stabilizer for open-loop sessions (`docs/scripted-flight.md`): Betaflight's angle mode in miniature, so a
//! script can fly a path without SITL. It is a test stand-in, not Betaflight: it reads the simulator's truth (no IMU
//! noise, no filters), has no arming, rates, I-term, TPA or airmode, and real tuning still needs SITL.
//!
//! The stick semantics are Betaflight's angle mode: roll `+` leans right, pitch `+` is forward (nose down), yaw `+`
//! turns clockwise seen from above, and throttle is the collective command.
use glam::{DQuat, DVec3};
use ofs_core::{names, Bus, Model, Signal, SimError, StepCtx};

/// Full roll or pitch stick leans the quad this far (Betaflight's default `angle_limit`).
pub const MAX_ANGLE_DEG: f64 = 60.0;
/// Full yaw stick turns the quad this fast.
pub const MAX_YAW_RATE_DPS: f64 = 200.0;
/// The angle loop asks for no more roll or pitch rate than this.
pub const MAX_RATE_DPS: f64 = 400.0;
/// At or below this throttle every motor command is 0: a quad on the ground stays put.
pub const IDLE_THROTTLE: f64 = 0.05;
/// The gains below were estimated from the shipped quad's inertia and thrust slope and tuned against
/// `crates/ofs-sim/tests/stabilizer.rs` on that quad (a 0.3 roll stick reaches 18.5 degrees at 0.5 s and peaks at
/// 20.6; yaw follows its stick within 15 %). They suit that airframe, not every quad.
///
/// Angle error (rad) to rate target (rad/s).
pub const KP_ANGLE: f64 = 15.0;
/// Rate error (rad/s) to motor command, roll and pitch.
pub const KP_RATE_ROLL_PITCH: f64 = 0.035;
/// Rate error (rad/s) to motor command, yaw (the estimate, 0.03, left the yaw rate 19 % short of the stick: a P-only
/// loop against the rotors' drag torque).
pub const KP_RATE_YAW: f64 = 0.06;

/// How each motor's command responds to the three axis outputs, from the frame's geometry.
#[derive(Debug, Clone)]
pub struct Mixer {
    weights: Vec<(f64, f64, f64)>,
}

impl Mixer {
    /// `positions_frd_m` are the motors' mounts (x forward, y right), `spin` is +1 for a rotor turning clockwise seen
    /// from above. A rotor on the left pushes the quad to roll right, one in front pitches it nose up, and a
    /// clockwise rotor's reaction turns the frame the other way.
    pub fn new(positions_frd_m: &[[f64; 3]], spin: &[i8]) -> Mixer {
        let reach = |axis: usize| positions_frd_m.iter().map(|p| p[axis].abs()).fold(0.0, f64::max).max(f64::MIN_POSITIVE);
        let (x_reach, y_reach) = (reach(0), reach(1));
        let weights = positions_frd_m.iter().zip(spin).map(|(p, s)| (-p[1] / y_reach, p[0] / x_reach, -f64::from(*s))).collect();
        Mixer { weights }
    }

    /// The (roll, pitch, yaw) weights of one motor.
    pub fn weights(&self, motor: usize) -> (f64, f64, f64) {
        self.weights[motor]
    }

    /// Writes each motor's command for the collective `throttle` and the three axis outputs, clamped to [0, 1].
    pub fn mix(&self, throttle: f64, roll: f64, pitch: f64, yaw: f64, out: &mut [f64]) {
        for (cmd, (wr, wp, wy)) in out.iter_mut().zip(&self.weights) {
            *cmd = (throttle + roll * wr + pitch * wp + yaw * wy).clamp(0.0, 1.0);
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Sticks {
    pub roll: f64,
    pub pitch: f64,
    pub yaw: f64,
    pub throttle: f64,
}

/// The truth attitude's roll and pitch (rad).
#[derive(Debug, Clone, Copy, Default)]
pub struct Attitude {
    pub roll_rad: f64,
    pub pitch_rad: f64,
}

/// (roll, pitch, yaw) in radians of an FRD-to-NED attitude, aerospace Z-Y-X.
pub fn euler_zyx(q: DQuat) -> (f64, f64, f64) {
    let roll = (2.0 * (q.w * q.x + q.y * q.z)).atan2(1.0 - 2.0 * (q.x * q.x + q.y * q.y));
    let pitch = (2.0 * (q.w * q.y - q.z * q.x)).clamp(-1.0, 1.0).asin();
    let yaw = (2.0 * (q.w * q.z + q.x * q.y)).atan2(1.0 - 2.0 * (q.y * q.y + q.z * q.z));
    (roll, pitch, yaw)
}

/// The (roll, pitch, yaw) outputs before mixing: the angle loop sets roll and pitch rate targets, the rate loop turns
/// every rate error into an output. Roll and pitch outputs are torque demands about the body's x and y axes (pitch
/// positive is nose up).
pub fn control(sticks: &Sticks, attitude: Attitude, rate_frd_radps: DVec3) -> (f64, f64, f64) {
    let max_angle = MAX_ANGLE_DEG.to_radians();
    let max_rate = MAX_RATE_DPS.to_radians();
    // A forward pitch stick leans the nose down, which is a negative aerospace pitch.
    let roll_rate_target = (KP_ANGLE * (sticks.roll * max_angle - attitude.roll_rad)).clamp(-max_rate, max_rate);
    let pitch_rate_target = (KP_ANGLE * (-sticks.pitch * max_angle - attitude.pitch_rad)).clamp(-max_rate, max_rate);
    let yaw_rate_target = sticks.yaw * MAX_YAW_RATE_DPS.to_radians();
    (
        KP_RATE_ROLL_PITCH * (roll_rate_target - rate_frd_radps.x),
        KP_RATE_ROLL_PITCH * (pitch_rate_target - rate_frd_radps.y),
        KP_RATE_YAW * (yaw_rate_target - rate_frd_radps.z),
    )
}

/// The stabilizer as a scheduler model: reads the sticks and the truth attitude, writes the motor commands.
pub struct Stabilizer {
    div: u32,
    mixer: Mixer,
    roll: Signal<f64>,
    pitch: Signal<f64>,
    yaw: Signal<f64>,
    throttle: Signal<f64>,
    att: Signal<DQuat>,
    rate: Signal<DVec3>,
    cmds: Vec<Signal<f64>>,
    out: Vec<f64>,
}

impl Stabilizer {
    pub fn new(positions_frd_m: &[[f64; 3]], spin: &[i8], rate_divisor: u32, bus: &mut Bus) -> Self {
        let motors = positions_frd_m.len();
        Self {
            div: rate_divisor,
            mixer: Mixer::new(positions_frd_m, spin),
            roll: bus.signal(names::RC_ROLL),
            pitch: bus.signal(names::RC_PITCH),
            yaw: bus.signal(names::RC_YAW),
            throttle: bus.signal(names::RC_THROTTLE),
            att: bus.signal(names::BODY_ATT),
            rate: bus.signal(names::BODY_RATE_FRD),
            cmds: (0..motors).map(|i| bus.signal(&names::motor_cmd(i))).collect(),
            out: vec![0.0; motors],
        }
    }
}

impl Model for Stabilizer {
    fn name(&self) -> &str {
        "fc.stabilizer"
    }

    fn rate_divisor(&self) -> u32 {
        self.div
    }

    fn step(&mut self, _ctx: &StepCtx, bus: &mut Bus) -> Result<(), SimError> {
        let throttle = bus.get(self.throttle).clamp(0.0, 1.0);
        if throttle <= IDLE_THROTTLE {
            self.out.fill(0.0);
        } else {
            let sticks = Sticks {
                roll: bus.get(self.roll).clamp(-1.0, 1.0),
                pitch: bus.get(self.pitch).clamp(-1.0, 1.0),
                yaw: bus.get(self.yaw).clamp(-1.0, 1.0),
                throttle,
            };
            let (roll_rad, pitch_rad, _) = euler_zyx(bus.get(self.att));
            let (roll, pitch, yaw) = control(&sticks, Attitude { roll_rad, pitch_rad }, bus.get(self.rate));
            self.mixer.mix(throttle, roll, pitch, yaw, &mut self.out);
        }
        for (signal, cmd) in self.cmds.iter().zip(&self.out) {
            bus.set(*signal, *cmd);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const POSITIONS: [[f64; 3]; 4] = [[-0.08, 0.08, 0.0], [0.08, 0.08, 0.0], [-0.08, -0.08, 0.0], [0.08, -0.08, 0.0]];
    const SPIN: [i8; 4] = [1, -1, -1, 1];

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-12
    }

    #[test]
    fn the_x_mixer_has_betaflights_signs() {
        let m = Mixer::new(&POSITIONS, &SPIN);
        assert_eq!(m.weights(0), (-1.0, -1.0, -1.0), "rear right, clockwise");
        assert_eq!(m.weights(1), (-1.0, 1.0, 1.0), "front right, counter-clockwise");
        assert_eq!(m.weights(2), (1.0, -1.0, 1.0), "rear left, counter-clockwise");
        assert_eq!(m.weights(3), (1.0, 1.0, -1.0), "front left, clockwise");
    }

    #[test]
    fn mixing_clamps_every_command_into_zero_to_one() {
        let m = Mixer::new(&POSITIONS, &SPIN);
        let mut out = [0.0; 4];
        m.mix(0.95, 0.5, 0.0, 0.0, &mut out);
        assert!(close(out[0], 0.45) && close(out[1], 0.45) && out[2] == 1.0 && out[3] == 1.0, "the left motors saturate at 1, the right ones drop: {out:?}");
        m.mix(0.05, -0.5, 0.0, 0.0, &mut out);
        assert!(close(out[0], 0.55) && close(out[1], 0.55) && out[2] == 0.0 && out[3] == 0.0, "the left motors floor at 0: {out:?}");
        m.mix(0.4, 0.05, -0.02, 0.01, &mut out);
        assert!(out.iter().all(|c| (0.0..=1.0).contains(c)));
        assert!(close(out[0], 0.4 - 0.05 + 0.02 - 0.01) && close(out[3], 0.4 + 0.05 - 0.02 - 0.01), "{out:?}");
    }

    #[test]
    fn a_level_still_quad_with_centred_sticks_asks_for_nothing() {
        assert_eq!(control(&Sticks::default(), Attitude::default(), DVec3::ZERO), (0.0, 0.0, 0.0));
    }

    #[test]
    fn sticks_set_angle_and_yaw_rate_targets() {
        let at_rest = |roll, pitch, yaw| control(&Sticks { roll, pitch, yaw, throttle: 0.4 }, Attitude::default(), DVec3::ZERO);
        let small = 0.1 * MAX_ANGLE_DEG.to_radians();
        let (r, _, _) = at_rest(0.1, 0.0, 0.0);
        assert!(close(r, KP_RATE_ROLL_PITCH * KP_ANGLE * small), "{r}");
        let (r, _, _) = at_rest(1.0, 0.0, 0.0);
        assert!(close(r, KP_RATE_ROLL_PITCH * MAX_RATE_DPS.to_radians()), "a full stick asks for the clamped rate: {r}");
        let (_, p, _) = at_rest(0.0, 0.1, 0.0);
        assert!(close(p, -KP_RATE_ROLL_PITCH * KP_ANGLE * small), "a forward stick asks for nose down, so a negative output: {p}");
        let (_, _, y) = at_rest(0.0, 0.0, 0.5);
        assert!(close(y, KP_RATE_YAW * 0.5 * MAX_YAW_RATE_DPS.to_radians()), "{y}");
    }

    #[test]
    fn the_loop_pushes_back_against_error() {
        let rolled = Attitude { roll_rad: 10f64.to_radians(), pitch_rad: 0.0 };
        let (r, _, _) = control(&Sticks::default(), rolled, DVec3::ZERO);
        assert!(r < 0.0, "rolled right with a centred stick: roll back left, got {r}");
        let (r, _, _) = control(&Sticks::default(), Attitude::default(), DVec3::new(1.0, 0.0, 0.0));
        assert!(close(r, -KP_RATE_ROLL_PITCH), "a positive roll rate is opposed: {r}");
        let (_, p, y) = control(&Sticks::default(), Attitude::default(), DVec3::new(0.0, 2.0, -1.0));
        assert!(close(p, -2.0 * KP_RATE_ROLL_PITCH) && close(y, KP_RATE_YAW), "{p} {y}");
    }

    #[test]
    fn euler_angles_round_trip_a_known_attitude() {
        let (roll, pitch, yaw) = (0.3, -0.2, 1.0);
        let q = DQuat::from_rotation_z(yaw) * DQuat::from_rotation_y(pitch) * DQuat::from_rotation_x(roll);
        let (r, p, y) = euler_zyx(q);
        assert!(close(r, roll) && close(p, pitch) && close(y, yaw), "{r} {p} {y}");
    }
}
