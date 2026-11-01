import math

EARTH_RADIUS_M = 6_371_000.0


class Projection:
    """Local tangent plane around (lat0, lon0): north and east in metres."""

    def __init__(self, lat0: float, lon0: float):
        self.lat0, self.lon0 = lat0, lon0
        self._cos = math.cos(math.radians(lat0))

    def to_ned(self, lat: float, lon: float) -> tuple[float, float]:
        north = math.radians(lat - self.lat0) * EARTH_RADIUS_M
        east = math.radians(lon - self.lon0) * EARTH_RADIUS_M * self._cos
        return north, east


def quantize(x: float) -> float:
    """Rounds to 1 mm (half to even); the integer step also turns -0.0 into 0.0."""
    return round(x * 1000) / 1000
