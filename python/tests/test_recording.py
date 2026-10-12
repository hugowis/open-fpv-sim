"""Recording the bus and reading it back (spec §4), open loop: no Betaflight needed."""
import hashlib
import math
import pathlib
import time

import pytest

import ofs
from conftest import QUAD

pytest.importorskip("mcap")
pd = pytest.importorskip("pandas")


def _climb(sim, seed=1):
    sim.load(QUAD, seed=seed, open_loop_fc=True)
    sim.set_sticks(throttle=0.6)
    sim.run(1.0)


def test_a_lockstep_load_records_and_sim_log_reads_it(sim):
    _climb(sim)
    assert sim.recording.active
    log = sim.log
    assert not sim.recording.active, "reading the log finished the file"
    assert pathlib.Path(log.path).parent == pathlib.Path(sim.logs_dir)
    assert pathlib.Path(log.path).name == "opendrone-5f-freestyle-1.mcap"
    assert log.messages > 10_000 and log.last_s == pytest.approx(1.0)
    df = log.to_dataframe(["imu.gyro_frd_radps", "motor.*.omega_radps", "battery.voltage_v"])
    assert df.index.name == "time_s"
    assert list(df.columns) == ["imu.gyro_frd_radps.x", "imu.gyro_frd_radps.y", "imu.gyro_frd_radps.z",
                                "motor.0.omega_radps", "motor.1.omega_radps", "motor.2.omega_radps",
                                "motor.3.omega_radps", "battery.voltage_v"]
    assert df.index[-1] == pytest.approx(1.0)
    gyro = df["imu.gyro_frd_radps.x"].dropna()
    assert len(gyro) == 1000, "the gyro at the default 1 kHz"
    battery = df["battery.voltage_v"].dropna()
    assert len(battery) == 10, "the battery at the default 10 Hz; NaN between its records"
    omega = df["motor.0.omega_radps"].dropna()
    assert omega.iloc[-1] > omega.iloc[0] + 500, "the motor spun up"
    assert sim.log.path == log.path, "a second read returns the same finished recording"


def test_a_real_time_or_unrecorded_session_has_no_log(sim):
    sim.load(QUAD, open_loop_fc=True, record=False)
    assert not sim.recording.active
    with pytest.raises(ofs.InvalidState):
        sim.log
    sim.load(QUAD, open_loop_fc=True, mode="realtime")
    assert not sim.recording.active, "real-time sessions record only when asked"


def test_a_real_time_session_records_when_asked(sim):
    sim.load(QUAD, open_loop_fc=True, mode="realtime")
    sim.recording.start(["battery.voltage_v"])
    sim.start()
    time.sleep(0.5)
    sim.pause()
    log = sim.recording.stop()
    assert log.messages >= 3, "10 Hz over about half a second of wall-paced simulated time"
    assert log.to_dataframe().shape[0] == log.messages


def test_a_recording_can_choose_its_signals_and_rates(sim):
    sim.load(QUAD, open_loop_fc=True, record=False)
    names = sim.recording.start(["battery.voltage_v", ("motor.*.omega_radps", 100)])
    assert names == ["battery.voltage_v", "motor.0.omega_radps", "motor.1.omega_radps", "motor.2.omega_radps",
                     "motor.3.omega_radps"]
    sim.run(0.5)
    log = sim.recording.stop()
    assert log.messages == 5 + 4 * 50
    assert log.signals() == names
    with pytest.raises(ofs.InvalidState):
        sim.recording.stop()
    with pytest.raises(ofs.InvalidArgument, match=r"similar: fault.imu.gyro_bias_radps, imu.gyro_frd_radps"):
        sim.recording.start(["gyro.*"])


def test_reading_a_pattern_the_file_lacks_names_the_near_misses(sim):
    _climb(sim)
    with pytest.raises(KeyError, match=r"pattern 'battery.voltage' matches no recorded signal \(similar: "
                                       r"battery.voltage_v"):
        sim.log.to_dataframe(["battery.voltage"])


def test_csv_and_parquet_exports(sim, tmp_path):
    pytest.importorskip("pyarrow")
    _climb(sim)
    log = sim.log
    out = log.to_csv(tmp_path / "battery.csv", ["battery.voltage_v", "battery.current_a"])
    lines = pathlib.Path(out).read_text().splitlines()
    assert lines[0] == "time_s,battery.voltage_v,battery.current_a", "columns in pattern order"
    assert len(lines) == 1 + 10
    t, voltage, current = lines[1].split(",")
    assert float(t) == pytest.approx(0.1) and float(voltage) > 20.0
    parquet = log.to_parquet(tmp_path / "imu.parquet", "imu.*")
    back = pd.read_parquet(parquet)
    assert back.shape == log.to_dataframe("imu.*").shape


def test_a_new_load_finishes_the_previous_recording(sim):
    _climb(sim)
    first = sim.recording.path
    _climb(sim)
    assert sim.recording.path != first
    assert sim.recording.last.path == first
    assert ofs.reader.Log(first).to_dataframe("battery.voltage_v").shape[0] == 10, "the server finished it"


def _sha(path):
    return hashlib.sha256(pathlib.Path(path).read_bytes()).hexdigest()


def test_two_identical_lockstep_sessions_write_identical_files(sim):
    """Spec §5.5 and §8.4: same quad, seed and inputs in lockstep give identical MCAP files."""
    _climb(sim, seed=3)
    a = sim.log.path
    _climb(sim, seed=3)
    b = sim.log.path
    _climb(sim, seed=4)
    c = sim.log.path
    assert a != b
    assert _sha(a) == _sha(b)
    assert _sha(a) != _sha(c)
    assert not math.isnan(ofs.reader.to_dataframe(a, "body.pos_ned_m").iloc[-1, 2])
