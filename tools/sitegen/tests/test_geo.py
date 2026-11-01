import math

from sitegen.geo import Projection, quantize


def test_to_ned_axes():
    p = Projection(48.0, 2.0)
    n, e = p.to_ned(48.001, 2.0)
    assert abs(n - 111.19) < 0.01 and e == 0.0
    n, e = p.to_ned(48.0, 2.001)
    assert n == 0.0 and abs(e - 111.19 * math.cos(math.radians(48.0))) < 0.01
    assert p.to_ned(47.999, 2.0)[0] < 0 and p.to_ned(48.0, 1.999)[1] < 0


def test_quantize_is_millimetre():
    assert quantize(1.23449) == 1.234
    assert quantize(1.2346) == 1.235
    assert quantize(0.0004) == 0.0 and str(quantize(-0.0004)) == "0.0"
