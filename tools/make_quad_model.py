#!/usr/bin/env python3
"""Generates the Godot client's drone model, godot/drone/quad.gltf (provenance: godot/drone/QUAD_MODEL.md).

A glTF 2.0 file: one embedded buffer, one unit-cube mesh per colour (flat baseColorFactor, no textures),
authored in Godot's frame (Y up, forward -Z) at the 5F frame's true size. Regenerate with
`python tools/make_quad_model.py` from the repository root; the output is deterministic, so commit the
script and the asset together.

Usage: python tools/make_quad_model.py [godot/drone/quad.gltf]
"""
import base64
import json
import math
import struct
import sys

OUT = sys.argv[1] if len(sys.argv) > 1 else "godot/drone/quad.gltf"

CARBON = [0.10, 0.10, 0.11, 1.0]
ARM_COLOR = [0.20, 0.20, 0.22, 1.0]
BATTERY = [0.90, 0.55, 0.10, 1.0]
STRAP = [0.15, 0.15, 0.16, 1.0]
BELL = [0.16, 0.16, 0.18, 1.0]
PROP = [0.08, 0.08, 0.08, 1.0]
ANTENNA = [[0.85, 0.20, 0.20, 1.0], [0.20, 0.40, 0.90, 1.0]]
HOUSING = [0.08, 0.08, 0.09, 1.0]
LENS = [0.02, 0.02, 0.03, 1.0]

EYE = [0.0, 0.02, -0.04]
# Betaflight quad-X in Godot's frame (right +X, back +Z): M1 rear right, M2 front right, M3 rear left, M4 front left.
MOTORS = [[0.08, 0.0, 0.08], [0.08, 0.0, -0.08], [-0.08, 0.0, 0.08], [-0.08, 0.0, -0.08]]


def unit_cube():
    faces = [
        ((0, 0, 1), [(-0.5, -0.5, 0.5), (0.5, -0.5, 0.5), (0.5, 0.5, 0.5), (-0.5, 0.5, 0.5)]),
        ((0, 0, -1), [(0.5, -0.5, -0.5), (-0.5, -0.5, -0.5), (-0.5, 0.5, -0.5), (0.5, 0.5, -0.5)]),
        ((0, 1, 0), [(-0.5, 0.5, 0.5), (0.5, 0.5, 0.5), (0.5, 0.5, -0.5), (-0.5, 0.5, -0.5)]),
        ((0, -1, 0), [(-0.5, -0.5, -0.5), (0.5, -0.5, -0.5), (0.5, -0.5, 0.5), (-0.5, -0.5, 0.5)]),
        ((1, 0, 0), [(0.5, -0.5, 0.5), (0.5, -0.5, -0.5), (0.5, 0.5, -0.5), (0.5, 0.5, 0.5)]),
        ((-1, 0, 0), [(-0.5, -0.5, -0.5), (-0.5, -0.5, 0.5), (-0.5, 0.5, 0.5), (-0.5, 0.5, -0.5)]),
    ]
    positions, normals, indices = [], [], []
    for normal, quad in faces:
        base = len(positions) // 3
        for corner in quad:
            positions += list(corner)
        for _ in quad:
            normals += list(normal)
        indices += [base, base + 1, base + 2, base, base + 2, base + 3]
    return positions, normals, indices


def quat(axis, degrees):
    half = math.radians(degrees) / 2.0
    s, c = math.sin(half), math.cos(half)
    return [s, 0.0, 0.0, c] if axis == "x" else [0.0, s, 0.0, c]


def part(name, mesh, translation, size, rotation=None):
    node = {"name": name, "mesh": mesh, "translation": list(translation), "scale": list(size)}
    if rotation:
        node["rotation"] = rotation
    return node


positions, normals, indices = unit_cube()
blob = struct.pack("<%df" % len(positions), *positions)
blob += struct.pack("<%df" % len(normals), *normals)
blob += struct.pack("<%dH" % len(indices), *indices)
views = [
    {"buffer": 0, "byteOffset": 0, "byteLength": len(positions) * 4},
    {"buffer": 0, "byteOffset": len(positions) * 4, "byteLength": len(normals) * 4},
    {"buffer": 0, "byteOffset": len(positions) * 4 + len(normals) * 4, "byteLength": len(indices) * 2},
]
accessors = [
    {"bufferView": 0, "componentType": 5126, "count": len(positions) // 3, "type": "VEC3", "max": [0.5, 0.5, 0.5], "min": [-0.5, -0.5, -0.5]},
    {"bufferView": 1, "componentType": 5126, "count": len(normals) // 3, "type": "VEC3"},
    {"bufferView": 2, "componentType": 5123, "count": len(indices), "type": "SCALAR", "max": [len(positions) - 1], "min": [0]},
]
colors = [CARBON, ARM_COLOR, BATTERY, STRAP, BELL, PROP, ANTENNA[0], ANTENNA[1], HOUSING, LENS]
materials = [
    {"name": "part%d" % i, "pbrMetallicRoughness": {"baseColorFactor": c, "metallicFactor": 0.0, "roughnessFactor": 0.8}}
    for i, c in enumerate(colors)
]
meshes = [
    {"primitives": [{"attributes": {"POSITION": 0, "NORMAL": 1}, "indices": 2, "material": i}]}
    for i in range(len(colors))
]
CARBON_M, ARM_M, BATTERY_M, STRAP_M, BELL_M, PROP_M, ANT1_M, ANT2_M, HOUSING_M, LENS_M = range(len(colors))

tree = [
    ({"name": "Frame"}, [
        (part("Plate", CARBON_M, [0, 0, 0.015], [0.034, 0.005, 0.09]), []),
        (part("PlateTop", CARBON_M, [0, 0.028, 0.012], [0.034, 0.004, 0.085]), []),
        (part("Arm1", ARM_M, [0, 0, 0], [0.24, 0.008, 0.014], quat("y", 45.0)), []),
        (part("Arm2", ARM_M, [0, 0, 0], [0.24, 0.008, 0.014], quat("y", -45.0)), []),
    ]),
    (part("Battery", BATTERY_M, [0, 0.0425, 0.01], [0.035, 0.025, 0.09]), []),
    (part("BatteryStrap", STRAP_M, [0, 0.0425, 0.01], [0.041, 0.028, 0.012]), []),
]
for i, motor in enumerate(MOTORS):
    tree.append((part("Motor%d" % (i + 1), BELL_M, [motor[0], 0.016, motor[2]], [0.052, 0.024, 0.052]), []))
    tree.append(({"name": "Prop%d" % (i + 1), "translation": [motor[0], 0.028, motor[2]]}, [
        (part("Blade", PROP_M, [0, 0, 0], [0.127, 0.002, 0.012]), []),
    ]))
for i, x in enumerate([-0.014, 0.014]):
    tree.append((part("Antenna%d" % (i + 1), ANT1_M if i == 0 else ANT2_M, [x, 0.058, 0.05], [0.007, 0.075, 0.007], quat("x", 40.0)), []))
tree.append((part("CameraMount", HOUSING_M, EYE, [0.02, 0.02, 0.026], quat("x", 30.0)), [
    (part("Lens", LENS_M, [0, 0, -0.015], [0.014, 0.014, 0.004]), []),
]))

nodes, roots = [], []


def walk(entry):
    node, children = entry
    index = len(nodes)
    nodes.append(node)
    node["children"] = [walk(child) for child in children]
    return index


for entry in tree:
    roots.append(walk(entry))

gltf = {
    "asset": {"version": "2.0", "generator": "tools/make_quad_model.py — open-fpv-sim, see godot/drone/QUAD_MODEL.md"},
    "scene": 0,
    "scenes": [{"nodes": roots}],
    "nodes": nodes,
    "meshes": meshes,
    "materials": materials,
    "accessors": accessors,
    "bufferViews": views,
    "buffers": [{"byteLength": len(blob), "uri": "data:application/octet-stream;base64," + base64.b64encode(blob).decode()}],
}
with open(OUT, "w", newline="\n") as f:
    json.dump(gltf, f, indent=1)
    f.write("\n")
print("wrote %s (%d nodes)" % (OUT, len(nodes)))
