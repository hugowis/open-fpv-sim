//! Triangle meshes in the world, with a deterministic BVH: the physics' sphere-contact query and the radio's
//! segment-crossing query. Positions are NED metres (down is +z). Contact is two-sided (no inside/outside test,
//! a mesh need not be watertight). Queries use only IEEE basic operations and `sqrt`, no fused multiply-add, and
//! visit the left child before the right one, so a result is identical on every OS.
use glam::DVec3;

use crate::rng::Fnv1a;
use crate::shape::Aabb;

/// A triangle with the material of the surface it belongs to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Triangle {
    pub a: DVec3,
    pub b: DVec3,
    pub c: DVec3,
    pub material: u32,
}

/// A triangle a sphere touches: `normal` is the unit vector from `point` (the closest point of the triangle) to
/// the sphere centre, the triangle's face normal when the distance is under 1e-12.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeshHit {
    pub distance: f64,
    pub point: DVec3,
    pub normal: DVec3,
    pub triangle: u32,
}

/// A leaf holds at most this many triangles.
const LEAF_MAX: usize = 4;
/// A contact nearer than this has no direction of its own: the face normal stands in.
const FACE_NORMAL_BELOW_M: f64 = 1e-12;
/// Slack on the barycentrics (and on the segment's ends), so a crossing on an edge is not lost to rounding.
const BARY_EPS: f64 = 1e-9;
/// Crossings this close together, as a fraction of the segment, are one surface (a shared edge or vertex).
const MERGE_EPS: f64 = 1e-9;
/// Slack on a node box when a segment is tested against it (metres), above the barycentric slack on big triangles.
const BOX_SLACK_M: f64 = 1e-3;

#[derive(Debug, Clone)]
struct Node {
    bounds: Aabb,
    /// Leaf: the first slot in `order`. Inner: the left child (the right one is `other`).
    first: u32,
    /// Leaf: the triangle count. Inner: the right child.
    other: u32,
    leaf: bool,
}

/// A triangle soup and the BVH over it.
#[derive(Debug, Clone)]
pub struct TriMesh {
    triangles: Vec<Triangle>,
    /// The triangle indices in leaf order.
    order: Vec<u32>,
    nodes: Vec<Node>,
}

fn tri_bounds(t: &Triangle) -> Aabb {
    Aabb { min: t.a.min(t.b).min(t.c), max: t.a.max(t.b).max(t.c) }
}

fn union(a: Aabb, b: Aabb) -> Aabb {
    Aabb { min: a.min.min(b.min), max: a.max.max(b.max) }
}

fn centroid(t: &Triangle) -> DVec3 {
    (t.a + t.b + t.c) / 3.0
}

/// The point of the triangle `a b c` closest to `p` (Ericson, Real-Time Collision Detection, 5.1.5).
pub fn closest_point_on_triangle(p: DVec3, a: DVec3, b: DVec3, c: DVec3) -> DVec3 {
    let ab = b - a;
    let ac = c - a;
    let ap = p - a;
    let d1 = ab.dot(ap);
    let d2 = ac.dot(ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let d3 = ab.dot(bp);
    let d4 = ac.dot(bp);
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let d5 = ab.dot(cp);
    let d6 = ac.dot(cp);
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && d4 - d3 >= 0.0 && d5 - d6 >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let denom = 1.0 / (va + vb + vc);
    a + ab * (vb * denom) + ac * (vc * denom)
}

/// The fraction `t` along the segment `a + dir * t` at which it crosses the triangle, strictly inside the segment
/// (Moller-Trumbore); a segment parallel to the triangle's plane crosses nothing.
fn segment_triangle(a: DVec3, dir: DVec3, t: &Triangle) -> Option<f64> {
    let e1 = t.b - t.a;
    let e2 = t.c - t.a;
    let pvec = dir.cross(e2);
    let det = e1.dot(pvec);
    if det.abs() <= 1e-12 * dir.length() * e1.cross(e2).length() {
        return None;
    }
    let inv = 1.0 / det;
    let tvec = a - t.a;
    let u = tvec.dot(pvec) * inv;
    if u < -BARY_EPS || u > 1.0 + BARY_EPS {
        return None;
    }
    let qvec = tvec.cross(e1);
    let v = dir.dot(qvec) * inv;
    if v < -BARY_EPS || u + v > 1.0 + BARY_EPS {
        return None;
    }
    let s = e2.dot(qvec) * inv;
    (s > BARY_EPS && s < 1.0 - BARY_EPS).then_some(s)
}

/// Whether the segment `a + dir * t`, t in 0..1, touches the box grown by `BOX_SLACK_M` (slab test).
fn segment_hits_box(a: DVec3, dir: DVec3, bounds: Aabb) -> bool {
    let (mut t0, mut t1) = (0.0_f64, 1.0_f64);
    for axis in 0..3 {
        let lo = bounds.min[axis] - BOX_SLACK_M;
        let hi = bounds.max[axis] + BOX_SLACK_M;
        if dir[axis] == 0.0 {
            if a[axis] < lo || a[axis] > hi {
                return false;
            }
        } else {
            let (s0, s1) = ((lo - a[axis]) / dir[axis], (hi - a[axis]) / dir[axis]);
            t0 = t0.max(s0.min(s1));
            t1 = t1.min(s0.max(s1));
            if t0 > t1 {
                return false;
            }
        }
    }
    true
}

impl TriMesh {
    /// A mesh of `triangles`; empty, non-finite and zero-area ones are rejected.
    pub fn new(triangles: Vec<Triangle>) -> Result<TriMesh, String> {
        if triangles.is_empty() {
            return Err("a mesh needs at least one triangle".into());
        }
        for (i, t) in triangles.iter().enumerate() {
            if ![t.a, t.b, t.c].iter().all(|p| p.is_finite()) {
                return Err(format!("triangle {i} has a non-finite vertex"));
            }
            if (t.b - t.a).cross(t.c - t.a).length_squared() == 0.0 {
                return Err(format!("triangle {i} has zero area"));
            }
        }
        let centroids: Vec<DVec3> = triangles.iter().map(centroid).collect();
        let mut order: Vec<u32> = (0..triangles.len() as u32).collect();
        let mut nodes = Vec::new();
        let n = order.len();
        build(&triangles, &centroids, &mut order, 0, n, &mut nodes);
        Ok(TriMesh { triangles, order, nodes })
    }

    pub fn len(&self) -> usize {
        self.triangles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.triangles.is_empty()
    }

    /// The box around every vertex.
    pub fn bounds(&self) -> Aabb {
        self.nodes[0].bounds
    }

    pub fn triangle(&self, i: u32) -> &Triangle {
        &self.triangles[i as usize]
    }

    /// FNV-1a of every triangle's vertex bits and material, then every BVH node box's bits.
    pub fn digest(&self) -> u64 {
        let mut h = Fnv1a::new();
        let put = |h: &mut Fnv1a, p: DVec3| {
            for x in [p.x, p.y, p.z] {
                h.write(&x.to_bits().to_le_bytes());
            }
        };
        for t in &self.triangles {
            for p in [t.a, t.b, t.c] {
                put(&mut h, p);
            }
            h.write(&t.material.to_le_bytes());
        }
        for n in &self.nodes {
            put(&mut h, n.bounds.min);
            put(&mut h, n.bounds.max);
        }
        h.finish()
    }

    /// Appends every triangle within `radius` of `p` (`distance < radius`), in BVH traversal order.
    pub fn within(&self, p: DVec3, radius: f64, out: &mut Vec<MeshHit>) {
        self.within_node(0, p, radius, out);
    }

    fn within_node(&self, node: usize, p: DVec3, radius: f64, out: &mut Vec<MeshHit>) {
        let n = &self.nodes[node];
        let gap = (n.bounds.min - p).max(p - n.bounds.max).max(DVec3::ZERO);
        if gap.length_squared() > radius * radius {
            return;
        }
        if !n.leaf {
            self.within_node(n.first as usize, p, radius, out);
            self.within_node(n.other as usize, p, radius, out);
            return;
        }
        for &i in &self.order[n.first as usize..(n.first + n.other) as usize] {
            let t = &self.triangles[i as usize];
            let point = closest_point_on_triangle(p, t.a, t.b, t.c);
            let to_centre = p - point;
            let distance = to_centre.length();
            if distance < radius {
                let normal = if distance < FACE_NORMAL_BELOW_M {
                    (t.b - t.a).cross(t.c - t.a).normalize()
                } else {
                    to_centre / distance
                };
                out.push(MeshHit { distance, point, normal, triangle: i });
            }
        }
    }

    /// Appends the material of every surface the open segment `a`..`b` crosses, in segment order; crossings
    /// within 1e-9 of the segment length of each other (a shared edge) count once.
    pub fn crossings(&self, a: DVec3, b: DVec3, out: &mut Vec<u32>) {
        let mut found = Vec::new();
        self.crossings_node(0, a, b - a, &mut found);
        found.sort_by(|x: &(f64, u32), y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
        let mut last = f64::NEG_INFINITY;
        for (s, i) in found {
            if s - last > MERGE_EPS {
                out.push(self.triangles[i as usize].material);
                last = s;
            }
        }
    }

    fn crossings_node(&self, node: usize, a: DVec3, dir: DVec3, found: &mut Vec<(f64, u32)>) {
        let n = &self.nodes[node];
        if !segment_hits_box(a, dir, n.bounds) {
            return;
        }
        if !n.leaf {
            self.crossings_node(n.first as usize, a, dir, found);
            self.crossings_node(n.other as usize, a, dir, found);
            return;
        }
        for &i in &self.order[n.first as usize..(n.first + n.other) as usize] {
            if let Some(s) = segment_triangle(a, dir, &self.triangles[i as usize]) {
                found.push((s, i));
            }
        }
    }
}

/// Builds the node over `order[lo..hi]` (pre-order, left then right) and returns its index.
fn build(tris: &[Triangle], centroids: &[DVec3], order: &mut [u32], lo: usize, hi: usize, nodes: &mut Vec<Node>) -> u32 {
    let bounds = order[lo..hi].iter().map(|&i| tri_bounds(&tris[i as usize])).reduce(union).expect("non-empty range");
    let me = nodes.len() as u32;
    if hi - lo <= LEAF_MAX {
        nodes.push(Node { bounds, first: lo as u32, other: (hi - lo) as u32, leaf: true });
        return me;
    }
    nodes.push(Node { bounds, first: 0, other: 0, leaf: false });
    let cb = order[lo..hi]
        .iter()
        .map(|&i| Aabb { min: centroids[i as usize], max: centroids[i as usize] })
        .reduce(union)
        .expect("non-empty range");
    let ext = cb.max - cb.min;
    let axis = if ext.x >= ext.y && ext.x >= ext.z { 0 } else if ext.y >= ext.z { 1 } else { 2 };
    order[lo..hi].sort_by(|&i, &j| centroids[i as usize][axis].total_cmp(&centroids[j as usize][axis]).then(i.cmp(&j)));
    let mid = lo + (hi - lo) / 2;
    let left = build(tris, centroids, order, lo, mid, nodes);
    let right = build(tris, centroids, order, mid, hi, nodes);
    nodes[me as usize].first = left;
    nodes[me as usize].other = right;
    me
}
