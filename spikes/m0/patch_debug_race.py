"""THROWAWAY: widen the review-finding-#1 race window (time visible before the tick) for testing."""
import sys

p = sys.argv[1] + "/src/platform/SIMULATOR/sitl.c"
s = open(p).read()
anchor = "    const uint64_t realtime_now = micros64_real();\n"
assert anchor in s and "OFS_DEBUG_WIDEN_RACE" not in s
# Insert after the external-time block that follows realtime_now: find the end of that #if block.
i = s.index(anchor) + len(anchor)
j = s.index("#endif\n", i) + len("#endif\n")
s = s[:j] + "#ifdef OFS_DEBUG_WIDEN_RACE\n    { struct timespec ts = { 0, 300000 }; nanosleep(&ts, NULL); }  // debug: hold the window between time update and gyro tick open\n#endif\n" + s[j:]
open(p, "w").write(s)
print("race window widened")
