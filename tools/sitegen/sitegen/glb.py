import json
import struct

from .geo import quantize
from .mesh import MeshBuilder

# shortcut: no col_ nodes, so every visible triangle also collides; add them if a site needs collision-only geometry.


def _frame(p) -> tuple[float, float, float]:
    # NED -> Godot (x east, y up, -z north). A proper rotation (det +1), so triangle winding is kept as is.
    n, e, d = (struct.unpack("<f", struct.pack("<f", quantize(v)))[0] for v in p)
    return (e, -d, -n)


def _pad(data: bytes, fill: bytes) -> bytes:
    return data + fill * (-len(data) % 4)


def write_glb(nodes: dict[str, MeshBuilder], materials: dict[str, tuple[float, float, float]]) -> bytes:
    names = sorted(materials)
    mat_index = {m: i for i, m in enumerate(names)}
    blob = bytearray()
    views, accessors, meshes, doc_nodes = [], [], [], []

    for name in sorted(nodes):
        by_mat: dict[str, list] = {}
        for a, b, c, mat in nodes[name].triangles:
            if mat not in mat_index:
                raise ValueError(f"node {name!r} uses unknown material {mat!r}")
            by_mat.setdefault(mat, []).append((a, b, c))
        prims = []
        for mat in sorted(by_mat):
            verts: dict[tuple, int] = {}  # insertion-ordered, so the vertex order is the triangle order
            indices = [verts.setdefault(_frame(p), len(verts)) for tri in by_mat[mat] for p in tri]
            pos = list(verts)
            lo = [min(v[i] for v in pos) for i in range(3)]
            hi = [max(v[i] for v in pos) for i in range(3)]
            for data, target, comp, kind, count, extra in (
                (b"".join(struct.pack("<3f", *v) for v in pos), 34962, 5126, "VEC3", len(pos), {"min": lo, "max": hi}),
                (struct.pack(f"<{len(indices)}I", *indices), 34963, 5125, "SCALAR", len(indices), {}),
            ):
                views.append({"buffer": 0, "byteOffset": len(blob), "byteLength": len(data), "target": target})
                accessors.append({"bufferView": len(views) - 1, "componentType": comp, "count": count, "type": kind, **extra})
                blob += data  # 12-byte vertices and 4-byte indices keep every offset 4-aligned
            prims.append({"attributes": {"POSITION": len(accessors) - 2}, "indices": len(accessors) - 1, "material": mat_index[mat], "mode": 4})
        node = {"name": name}
        if prims:
            meshes.append({"name": name, "primitives": prims})
            node["mesh"] = len(meshes) - 1
        doc_nodes.append(node)

    doc = {
        "asset": {"version": "2.0"},
        "scene": 0,
        "scenes": [{"nodes": list(range(len(doc_nodes)))}],
        "nodes": doc_nodes,
        "meshes": meshes,
        "materials": [
            {
                "name": m,
                "doubleSided": True,
                "pbrMetallicRoughness": {"baseColorFactor": [*materials[m], 1.0], "metallicFactor": 0, "roughnessFactor": 1},
            }
            for m in names
        ],
        "accessors": accessors,
        "bufferViews": views,
        "buffers": [{"byteLength": len(blob)}],
    }
    js = _pad(json.dumps(doc, sort_keys=True, ensure_ascii=False, separators=(",", ":")).encode("utf-8"), b" ")
    bn = _pad(bytes(blob), b"\0")
    total = 12 + 8 + len(js) + 8 + len(bn)
    return struct.pack("<4sII", b"glTF", 2, total) + struct.pack("<I4s", len(js), b"JSON") + js + struct.pack("<I4s", len(bn), b"BIN\0") + bn
