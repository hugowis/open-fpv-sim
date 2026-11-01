import json
import struct

from make_golden import GOLDEN, build
from sitegen.glb import write_glb
from sitegen.mesh import MeshBuilder

MATS = {"a": (1.0, 0.0, 0.0), "b": (0.0, 1.0, 0.0)}


def tri_node(material="a", p=((0.0, 0.0, 0.0), (1.0, 0.0, 0.0), (0.0, 1.0, 0.0))):
    m = MeshBuilder()
    m.add_tri(*p, material)
    return m


def parse(blob):
    magic, version, length = struct.unpack_from("<4sII", blob, 0)
    jlen, jtype = struct.unpack_from("<I4s", blob, 12)
    doc = json.loads(blob[20 : 20 + jlen])
    boff = 20 + jlen
    blen, btype = struct.unpack_from("<I4s", blob, boff)
    return magic, version, length, jtype, btype, doc, blob[boff + 8 : boff + 8 + blen]


def test_glb_header_and_chunks():
    blob = write_glb({"n": tri_node()}, MATS)
    magic, version, length, jtype, btype, doc, bin_ = parse(blob)
    assert (magic, version, length) == (b"glTF", 2, len(blob))
    assert (jtype, btype) == (b"JSON", b"BIN\x00")
    assert len(blob) % 4 == 0 and len(bin_) % 4 == 0 and doc["buffers"][0]["byteLength"] == len(bin_)


def test_frame_maps_north_to_minus_z():
    m = MeshBuilder()
    m.add_tri((100.0, 0.0, 0.0), (0.0, 7.0, 0.0), (0.0, 0.0, -3.0), "a")
    doc, bin_ = parse(write_glb({"n": m}, MATS))[5:]
    acc = doc["accessors"][0]
    view = doc["bufferViews"][acc["bufferView"]]
    pts = [struct.unpack_from("<3f", bin_, view["byteOffset"] + 12 * i) for i in range(acc["count"])]
    assert (0.0, 0.0, -100.0) in pts  # north
    assert (7.0, 0.0, 0.0) in pts  # east is +x
    assert (0.0, 3.0, 0.0) in pts  # up (down negative) is +y


def test_winding_survives_the_frame_change():
    # The map (n, e, d) -> (e, -d, -n) has determinant +1, so indices are kept: NED n x e = down = -y.
    m = tri_node(p=((0.0, 0.0, 0.0), (1.0, 0.0, 0.0), (0.0, 1.0, 0.0)))
    doc, bin_ = parse(write_glb({"n": m}, MATS))[5:]
    view = doc["bufferViews"][doc["accessors"][0]["bufferView"]]
    idx = doc["bufferViews"][doc["accessors"][1]["bufferView"]]
    pts = [struct.unpack_from("<3f", bin_, view["byteOffset"] + 12 * i) for i in range(3)]
    a, b, c = (pts[i] for i in struct.unpack_from("<3I", bin_, idx["byteOffset"]))
    u, v = [b[i] - a[i] for i in range(3)], [c[i] - a[i] for i in range(3)]
    normal = (u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0])
    assert normal == (0.0, -1.0, 0.0)


def test_materials_are_double_sided():
    doc = parse(write_glb({"n": tri_node()}, MATS))[5]
    assert [m["name"] for m in doc["materials"]] == ["a", "b"]
    assert all(m["doubleSided"] is True for m in doc["materials"])
    assert doc["materials"][0]["pbrMetallicRoughness"]["baseColorFactor"] == [1.0, 0.0, 0.0, 1.0]


def test_same_input_same_bytes():
    assert write_glb({"n": tri_node()}, MATS) == write_glb({"n": tri_node()}, MATS)


def test_node_order_does_not_depend_on_insertion_order():
    one = write_glb({"x": tri_node("a"), "y": tri_node("b")}, MATS)
    two = write_glb({"y": tri_node("b"), "x": tri_node("a")}, MATS)
    assert one == two
    assert [n["name"] for n in parse(one)[5]["nodes"]] == ["x", "y"]


def test_golden_is_current():
    assert GOLDEN.read_bytes() == build()
