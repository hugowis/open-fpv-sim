Point = tuple[float, float, float]  # NED metres


class MeshBuilder:
    def __init__(self):
        self.triangles: list[tuple[Point, Point, Point, str]] = []

    def add_tri(self, a: Point, b: Point, c: Point, material: str) -> None:
        self.triangles.append((a, b, c, material))

    def add_quad(self, a: Point, b: Point, c: Point, d: Point, material: str) -> None:
        self.add_tri(a, b, c, material)
        self.add_tri(a, c, d, material)

    def extend(self, other: "MeshBuilder") -> None:
        self.triangles.extend(other.triangles)
