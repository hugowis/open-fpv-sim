use ofs_core::{names, Bus, Scheduler};
use ofs_fc::open_loop::OpenLoopFc;

#[test]
fn every_motor_follows_clamped_throttle() {
    let mut bus = Bus::new();
    let fc = OpenLoopFc::new(4, 8, &mut bus);
    let thr = bus.signal::<f64>(names::RC_THROTTLE);
    bus.set(thr, 1.7);
    let mut s = Scheduler::new(8000, bus);
    s.add(Box::new(fc));
    s.step().unwrap();
    for i in 0..4 {
        assert_eq!(s.bus().get(s.bus().lookup::<f64>(&names::motor_cmd(i)).unwrap()), 1.0);
    }
}
