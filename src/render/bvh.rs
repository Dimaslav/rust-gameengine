use glam::Vec3;

const MAX_LEAF_SIZE: usize = 8;
const STACK_SIZE: usize = 64;

#[derive(Clone, Copy, Debug)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    pub fn empty() -> Self {
        Self {
            min: Vec3::splat(f32::INFINITY),
            max: Vec3::splat(f32::NEG_INFINITY),
        }
    }

    pub fn from_point(p: Vec3) -> Self {
        Self { min: p, max: p }
    }

    pub fn union(&self, other: &Aabb) -> Self {
        Self {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }

    pub fn extend(&mut self, p: Vec3) {
        self.min = self.min.min(p);
        self.max = self.max.max(p);
    }

    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    pub fn extent(&self) -> Vec3 {
        self.max - self.min
    }

    pub fn ray_intersect(&self, origin: Vec3, dir: Vec3, max_t: f32) -> bool {
        let o = origin.to_array();
        let d = dir.to_array();
        let mn = self.min.to_array();
        let mx = self.max.to_array();
        let mut tmin = 0.0f32;
        let mut tmax = max_t;

        for i in 0..3 {
            let di = d[i];
            let oi = o[i];

            if di.abs() < 1e-12 {
                if oi < mn[i] || oi > mx[i] {
                    return false;
                }
            } else {
                let inv = 1.0 / di;
                let mut t1 = (mn[i] - oi) * inv;
                let mut t2 = (mx[i] - oi) * inv;
                if t1 > t2 {
                    std::mem::swap(&mut t1, &mut t2);
                }
                if t1 > tmin {
                    tmin = t1;
                }
                if t2 < tmax {
                    tmax = t2;
                }
                if tmin > tmax {
                    return false;
                }
            }
        }
        true
    }
}

#[derive(Clone, Copy, Debug)]
struct Node {
    aabb: Aabb,
    first: u32,
    count: u32,
}

pub struct Bvh {
    nodes: Vec<Node>,
    tri_indices: Vec<u32>,
    tri_aabbs: Vec<Aabb>,
}

impl Bvh {
    pub fn build(triangles: &[[Vec3; 3]]) -> Self {
        let n = triangles.len();
        if n == 0 {
            return Self {
                nodes: Vec::new(),
                tri_indices: Vec::new(),
                tri_aabbs: Vec::new(),
            };
        }

        let mut centroids: Vec<Vec3> = Vec::with_capacity(n);
        let mut tri_aabbs: Vec<Aabb> = Vec::with_capacity(n);
        for tri in triangles {
            let aabb = Aabb::from_point(tri[0])
                .union(&Aabb::from_point(tri[1]))
                .union(&Aabb::from_point(tri[2]));
            centroids.push(aabb.center());
            tri_aabbs.push(aabb);
        }

        let mut tri_indices: Vec<u32> = (0..n as u32).collect();
        let mut nodes: Vec<Node> = Vec::with_capacity(2 * n);
        let root_aabb = compute_range_aabb(&tri_indices, &tri_aabbs);
        nodes.push(Node {
            aabb: root_aabb,
            first: 0,
            count: n as u32,
        });

        build_recursive(
            &mut nodes,
            0,
            &mut tri_indices,
            &centroids,
            &tri_aabbs,
        );

        Self {
            nodes,
            tri_indices,
            tri_aabbs,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn raycast(
        &self,
        origin: Vec3,
        dir: Vec3,
        max_t: f32,
        triangles: &[[Vec3; 3]],
    ) -> Option<(u32, f32)> {
        if self.nodes.is_empty() {
            return None;
        }
        let mut stack: [u32; STACK_SIZE] = [0; STACK_SIZE];
        let mut sp: usize = 0;
        stack[sp] = 0;
        sp += 1;
        let mut best_t = max_t;
        let mut best_tri: Option<u32> = None;
        while sp > 0 {
            sp -= 1;
            let node = self.nodes[stack[sp] as usize];

            if !node.aabb.ray_intersect(origin, dir, best_t) {
                continue;
            }

            if node.count > 0 {
                let first = node.first as usize;
                let count = node.count as usize;
                for i in 0..count {
                    let ti = self.tri_indices[first + i];
                    let tri = &triangles[ti as usize];
                    if let Some(t) =
                        ray_triangle(origin, dir, tri[0], tri[1], tri[2])
                        && t < best_t {
                            best_t = t;
                            best_tri = Some(ti);
                        }
                }
            } else {
                debug_assert!(sp + 2 <= STACK_SIZE, "BVH stack overflow");
                if sp + 2 > STACK_SIZE {
                    continue;
                }
                stack[sp] = node.first;
                stack[sp + 1] = node.first + 1;
                sp += 2;
            }
        }

        best_tri.map(|ti| (ti, best_t))
    }

    pub fn for_each_in_aabb<F>(&self, min: Vec3, max: Vec3, mut f: F)
    where
        F: FnMut(u32) -> bool,
    {
        if self.nodes.is_empty() {
            return;
        }
        let query = Aabb { min, max };
        let mut stack: [u32; STACK_SIZE] = [0; STACK_SIZE];
        let mut sp = 1usize;
        stack[0] = 0;

        while sp > 0 {
            sp -= 1;
            let node = &self.nodes[stack[sp] as usize];
            if !aabb_overlap(&node.aabb, &query) {
                continue;
            }

            if node.count > 0 {
                let first = node.first as usize;
                let count = node.count as usize;
                for i in 0..count {
                    let ti = self.tri_indices[first + i];
                    if aabb_overlap(&self.tri_aabbs[ti as usize], &query)
                        && !f(ti)
                    {
                        return;
                    }
                }
            } else {
                if sp + 2 > STACK_SIZE {
                    continue;
                }
                stack[sp] = node.first;
                stack[sp + 1] = node.first + 1;
                sp += 2;
            }
        }
    }
}

fn aabb_overlap(a: &Aabb, b: &Aabb) -> bool {
    a.min.x <= b.max.x
        && a.max.x >= b.min.x
        && a.min.y <= b.max.y
        && a.max.y >= b.min.y
        && a.min.z <= b.max.z
        && a.max.z >= b.min.z
}

fn compute_range_aabb(indices: &[u32], aabbs: &[Aabb]) -> Aabb {
    let mut r = Aabb::empty();
    for &i in indices {
        let a = &aabbs[i as usize];
        r.min = r.min.min(a.min);
        r.max = r.max.max(a.max);
    }
    r
}

fn build_recursive(
    nodes: &mut Vec<Node>,
    node_idx: usize,
    tri_indices: &mut [u32],
    centroids: &[Vec3],
    tri_aabbs: &[Aabb],
) {
    let (first, count) = {
        let n = &nodes[node_idx];
        (n.first, n.count)
    };

    if count <= MAX_LEAF_SIZE as u32 {
        return;
    }

    let lo = first as usize;
    let hi = (first + count) as usize;
    let mut cmin = Vec3::splat(f32::INFINITY);
    let mut cmax = Vec3::splat(f32::NEG_INFINITY);
    for &i in &tri_indices[lo..hi] {
        let c = centroids[i as usize];
        cmin = cmin.min(c);
        cmax = cmax.max(c);
    }
    let extent = cmax - cmin;
    if extent.max_element() < 1e-9 {
        return;
    }
    let axis = if extent.x >= extent.y && extent.x >= extent.z {
        0
    } else if extent.y >= extent.z {
        1
    } else {
        2
    };

    let mid = lo + (hi - lo) / 2;
    tri_indices[lo..hi].select_nth_unstable_by(mid - lo, |&a, &b| {
        let ca = centroids[a as usize][axis];
        let cb = centroids[b as usize][axis];
        ca.partial_cmp(&cb).unwrap_or(std::cmp::Ordering::Equal)
    });

    let left_count = (mid - lo) as u32;
    let right_count = count - left_count;

    debug_assert!(left_count >= 1 && right_count >= 1);

    let left_aabb = compute_range_aabb(&tri_indices[lo..mid], tri_aabbs);
    let left_idx = nodes.len() as u32;
    nodes.push(Node {
        aabb: left_aabb,
        first: lo as u32,
        count: left_count,
    });

    let right_aabb = compute_range_aabb(&tri_indices[mid..hi], tri_aabbs);
    let right_idx = nodes.len() as u32;
    nodes.push(Node {
        aabb: right_aabb,
        first: mid as u32,
        count: right_count,
    });

    nodes[node_idx].count = 0;
    nodes[node_idx].first = left_idx;

    // Рекурсия. Правый ребёнок всегда left_idx + 1.
    build_recursive(nodes, left_idx as usize, tri_indices, centroids, tri_aabbs);
    build_recursive(nodes, right_idx as usize, tri_indices, centroids, tri_aabbs);
}

fn ray_triangle(
    ro: Vec3,
    rd: Vec3,
    v0: Vec3,
    v1: Vec3,
    v2: Vec3,
) -> Option<f32> {
    let e1 = v1 - v0;
    let e2 = v2 - v0;
    let p = rd.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-8 {
        return None;
    }
    let inv_det = 1.0 / det;
    let tvec = ro - v0;
    let u = tvec.dot(p) * inv_det;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = tvec.cross(e1);
    let v = rd.dot(q) * inv_det;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(q) * inv_det;
    if t > 1e-5 {
        Some(t)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_quad(z: f32) -> Vec<[Vec3; 3]> {
        let v0 = Vec3::new(-1.0, -1.0, z);
        let v1 = Vec3::new( 1.0, -1.0, z);
        let v2 = Vec3::new( 1.0,  1.0, z);
        let v3 = Vec3::new(-1.0,  1.0, z);
        vec![[v0, v1, v2], [v0, v2, v3]]
    }

    #[test]
    fn empty_bvh() {
        let bvh = Bvh::build(&[]);
        assert!(bvh.is_empty());
        let hit = bvh.raycast(Vec3::ZERO, Vec3::Z, 100.0, &[]);
        assert!(hit.is_none());
    }

    #[test]
    fn hits_single_triangle() {
        let tris = make_quad(5.0);
        let bvh = Bvh::build(&tris);
        let hit = bvh.raycast(Vec3::ZERO, Vec3::Z, 100.0, &tris);
        let (_, t) = hit.expect("hit");
        assert!((t - 5.0).abs() < 1e-4, "t = {}", t);
    }

    #[test]
    fn respects_max_t() {
        let tris = make_quad(5.0);
        let bvh = Bvh::build(&tris);
        let hit = bvh.raycast(Vec3::ZERO, Vec3::Z, 3.0, &tris);
        assert!(hit.is_none());
    }

    #[test]
    fn picks_nearest() {
        let mut tris = make_quad(5.0);
        tris.extend(make_quad(2.0));
        let bvh = Bvh::build(&tris);
        let (_, t) = bvh.raycast(Vec3::ZERO, Vec3::Z, 100.0, &tris).unwrap();
        assert!((t - 2.0).abs() < 1e-4, "t = {}", t);
    }

    #[test]
    fn misses_parallel() {
        let tris = make_quad(5.0);
        let bvh = Bvh::build(&tris);
        let hit = bvh.raycast(Vec3::ZERO, Vec3::X, 100.0, &tris);
        assert!(hit.is_none());
    }

    #[test]
    fn large_cloud_no_stack_overflow() {
        let mut tris = Vec::new();
        for i in 0..5000 {
            let x = ((i * 17) % 100) as f32 - 50.0;
            let y = ((i * 31) % 100) as f32 - 50.0;
            let z = ((i * 53) % 100) as f32 - 50.0;
            tris.push([
                Vec3::new(x, y, z),
                Vec3::new(x + 0.5, y, z),
                Vec3::new(x, y + 0.5, z),
            ]);
        }
        let bvh = Bvh::build(&tris);
        let _ = bvh.raycast(Vec3::ZERO, Vec3::X, 1000.0, &tris);
    }
}