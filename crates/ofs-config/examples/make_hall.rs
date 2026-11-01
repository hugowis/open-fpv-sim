//! Writes `worlds/assets/hall.glb`, the demo hall of `worlds/hall.toml`: single-sided surfaces, no thickness.
//! In NED the footprint is north 30..50, east -6..6, 6 m tall under a flat roof, with a 4 x 3 m doorway in the
//! south wall (east -2..2, up 0..3) and a 4 x 1 m glass window in the east wall (north 38..42, up 3..4).
//! The file frame is Godot's: x = east, y = up, z = -north. Run: `cargo run -p ofs-config --example make_hall`.
#[path = "../tests/common/glb.rs"]
mod glb;

use glb::{write_glb, GlbNode};

type Tri = [[f32; 3]; 3];

/// A rectangle `[u0, u1] x [v0, v1]` as two triangles, mapped into the file frame by `at(u, v)`.
fn rect(out: &mut Vec<Tri>, at: impl Fn(f32, f32) -> [f32; 3], [u0, u1]: [f32; 2], [v0, v1]: [f32; 2]) {
    let (a, b, c, d) = (at(u0, v0), at(u1, v0), at(u1, v1), at(u0, v1));
    out.extend([[a, b, c], [a, c, d]]);
}

fn main() {
    const UP: f32 = 6.0;
    let mut walls = Vec::new();
    // South wall (north = 30): u = east, v = up; the doorway is left out.
    let south = |u, v| [u, v, -30.0];
    rect(&mut walls, south, [-6.0, -2.0], [0.0, UP]);
    rect(&mut walls, south, [2.0, 6.0], [0.0, UP]);
    rect(&mut walls, south, [-2.0, 2.0], [3.0, UP]);
    // East wall (east = 6): u = north; the window is a hole, glazed below.
    let east = |u: f32, v| [6.0, v, -u];
    rect(&mut walls, east, [30.0, 50.0], [0.0, 3.0]);
    rect(&mut walls, east, [30.0, 50.0], [4.0, UP]);
    rect(&mut walls, east, [30.0, 38.0], [3.0, 4.0]);
    rect(&mut walls, east, [42.0, 50.0], [3.0, 4.0]);
    // North wall (north = 50), west wall (east = -6) and the roof, whole.
    rect(&mut walls, |u, v| [u, v, -50.0], [-6.0, 6.0], [0.0, UP]);
    rect(&mut walls, |u: f32, v| [-6.0, v, -u], [30.0, 50.0], [0.0, UP]);
    rect(&mut walls, |u, v: f32| [u, UP, -v], [-6.0, 6.0], [30.0, 50.0]);
    let mut pane = Vec::new();
    rect(&mut pane, east, [38.0, 42.0], [3.0, 4.0]);

    let bytes = write_glb(&[
        GlbNode { name: "walls", triangles: walls, material: Some("concrete") },
        GlbNode { name: "window", triangles: pane, material: Some("glass") },
    ]);
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../worlds/assets/hall.glb");
    std::fs::create_dir_all(std::path::Path::new(path).parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}
