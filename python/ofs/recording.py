"""Recording the simulator's signal bus to MCAP (`Sim.recording`): one recording at a time, in a file the server
names under its logs directory (`Sim.logs_dir`)."""
from __future__ import annotations

from ofs.v1 import sim_pb2 as pb

from .reader import Log


def _signal(p) -> pb.RecordingSignal:
    if isinstance(p, str):
        return pb.RecordingSignal(pattern=p)
    pattern, rate_hz = p
    return pb.RecordingSignal(pattern=pattern, rate_hz=float(rate_hz))


class Recording:
    """`sim.recording`: start and stop the session's recording. A lockstep `load` starts one with the default
    rates (`load(record=False)` does not); `sim.log` stops the running one and reads it."""

    def __init__(self, sim):
        self._sim = sim
        self.path = None  # the running recording's file, None when none runs
        self.last = None  # the last finished recording, as a reader.Log

    @property
    def active(self) -> bool:
        return self.path is not None

    def start(self, patterns=None) -> list:
        """Starts recording. `patterns`: None for every signal at the default table's rates, or a list of patterns
        (`"imu.*"`, at the default rate for each signal) and `(pattern, rate_hz)` pairs; only matching signals are
        recorded. Returns the recorded signals' names."""
        signals = [_signal(p) for p in ([] if patterns is None else ([patterns] if isinstance(patterns, str) else patterns))]
        reply = self._sim._call(self._sim._stub.StartRecording, pb.StartRecordingRequest(signals=signals))
        self.path = reply.path
        return list(reply.signals)

    def stop(self) -> Log:
        """Finishes the running recording's file and returns it as a `reader.Log`."""
        m = self._sim._call(self._sim._stub.StopRecording, pb.Empty())
        self.path = None
        self.last = Log(m.path, messages=m.messages, first_s=m.first_s, last_s=m.last_s)
        return self.last

    def _session_ended(self) -> None:
        """A new session replaced the old one: its running recording was finished by the server as it ended."""
        if self.path is not None:
            self.last = Log(self.path)
            self.path = None
