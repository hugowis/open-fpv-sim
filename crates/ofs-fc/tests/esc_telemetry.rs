use ofs_core::{names, Bus, Model, StepCtx, Wire};
use ofs_fc::sitl::esc_telemetry::{crc8, kiss_frame, EscTelemetry, FRAME_SIZE};

#[test]
fn a_frame_has_the_kiss_layout_and_checksum() {
    // 25 degC, 24.60 V (0x099C), 1.00 A (0x0064), 16 mAh (0x0010), 29100 eRPM -> 291 (0x0123).
    let f = kiss_frame(25, 24.6, 1.0, 16.0, 29_100.0);
    assert_eq!(&f[..9], &[25, 0x09, 0x9C, 0x00, 0x64, 0x00, 0x10, 0x01, 0x23]);
    assert_eq!(f[9], 0x0D);
    assert_eq!(f[9], crc8(&f[..9]));
}

#[test]
fn values_are_clamped_to_the_field_widths() {
    let f = kiss_frame(25, -3.0, 1.0e9, 1.0e9, -50.0);
    assert_eq!(&f[1..3], &[0, 0], "negative voltage");
    assert_eq!(&f[3..5], &[0xFF, 0xFF], "current saturates");
    assert_eq!(&f[5..7], &[0xFF, 0xFF], "charge saturates");
    assert_eq!(&f[7..9], &[0, 0], "negative rpm");
}

#[test]
fn the_model_sends_one_quarter_of_the_pack_current_in_each_of_four_frames() {
    let mut bus = Bus::new();
    let v = bus.signal::<f64>(names::BATTERY_VOLTAGE);
    let i = bus.signal::<f64>(names::BATTERY_CURRENT);
    let c = bus.signal::<f64>(names::BATTERY_CONSUMED);
    bus.set(v, 24.6);
    bus.set(i, 8.0);
    bus.set(c, 120.0);
    let uart = Wire::new(256);
    let mut model = EscTelemetry::new(uart.clone(), 4, 80, &mut bus);
    assert_eq!(model.rate_divisor(), 80);
    model.step(&StepCtx { tick: 0, time_s: 0.0, dt_s: 1.0 / 8000.0 }, &mut bus).unwrap();
    let bytes = uart.take(256);
    assert_eq!(bytes.len(), 4 * FRAME_SIZE);
    for frame in bytes.chunks(FRAME_SIZE) {
        assert_eq!(&frame[1..3], &[0x09, 0x9C], "pack voltage in every frame");
        assert_eq!(&frame[3..5], &[0x00, 0xC8], "2.00 A = 8.00 A / 4");
        assert_eq!(&frame[5..7], &[0x00, 0x1E], "30 mAh = 120 mAh / 4");
        assert_eq!(frame[9], crc8(&frame[..9]));
    }
}
