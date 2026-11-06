//! Ring simplification with the rule of GEOS `simplify(tolerance, preserve_topology=True)`. Lines are simplified with `geo::Simplify` (Douglas-Peucker).

fn point_segment_distance(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    if len2 == 0.0 {
        return (p.0 - a.0).hypot(p.1 - a.1);
    }
    let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0);
    (p.0 - (a.0 + t * dx)).hypot(p.1 - (a.1 + t * dy))
}

/// A closed ring simplified like GEOS `simplify(tolerance, preserve_topology=True)` (JTS `TaggedLineStringSimplifier`) does a ring: Douglas-Peucker
/// from the first vertex, except that a section may only be flattened when the result cannot end up with fewer than four vertices (the
/// first sections are always split further). The checks against self-intersection of the simplified ring are left out.
pub fn simplify_ring(ring: &[(f64, f64)], tolerance: f64) -> Vec<(f64, f64)> {
    const MIN_SIZE: usize = 4;
    fn section(
        pts: &[(f64, f64)],
        i: usize,
        j: usize,
        depth: usize,
        tolerance: f64,
        segments: &mut usize,
        kept: &mut Vec<usize>,
    ) {
        let depth = depth + 1;
        if j - i == 1 {
            kept.push(j);
            *segments += 1;
            return;
        }
        let mut valid = true;
        // enough vertices must stay in the result
        let result_size = if *segments == 0 { 0 } else { *segments + 1 };
        if result_size < MIN_SIZE && depth + 1 < MIN_SIZE {
            valid = false;
        }
        let (mut far, mut max_d) = (i, -1.0);
        for k in i + 1..j {
            let d = point_segment_distance(pts[k], pts[i], pts[j]);
            if d > max_d {
                max_d = d;
                far = k;
            }
        }
        if max_d > tolerance {
            valid = false;
        }
        if valid {
            kept.push(j);
            *segments += 1;
            return;
        }
        section(pts, i, far, depth, tolerance, segments, kept);
        section(pts, far, j, depth, tolerance, segments, kept);
    }

    if ring.len() < 4 {
        return ring.to_vec();
    }
    let mut kept = vec![0usize];
    let mut segments = 0usize;
    section(
        ring,
        0,
        ring.len() - 1,
        0,
        tolerance,
        &mut segments,
        &mut kept,
    );
    kept.iter().map(|&i| ring[i]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // the lines are simplified with geo::Simplify: this pins down the behaviour the tiles rely on
    fn dp(points: &[(f64, f64)], epsilon: f64) -> Vec<(f64, f64)> {
        use geo::Simplify;
        geo::LineString::from(points.to_vec())
            .simplify(&epsilon)
            .coords()
            .map(|c| (c.x, c.y))
            .collect()
    }

    #[test]
    fn drops_the_points_within_epsilon_and_keeps_the_corners_and_the_ends() {
        let noisy = [(0.0, 0.0), (1.0, 0.05), (2.0, 0.0), (3.0, 0.05), (4.0, 0.0)];
        assert_eq!(dp(&noisy, 0.1), vec![(0.0, 0.0), (4.0, 0.0)]);
        assert_eq!(dp(&noisy, 0.01).len(), 5);
        let corner = [(0.0, 0.0), (5.0, 0.0), (5.0, 5.0)];
        assert_eq!(dp(&corner, 0.1), corner.to_vec());
        // a long gentle bend: the middle point is 0.5 from the chord
        let bend = [(0.0, 0.0), (5.0, 0.5), (10.0, 0.0)];
        assert_eq!(dp(&bend, 0.4).len(), 3);
        assert_eq!(dp(&bend, 0.6).len(), 2);
    }

    #[test]
    fn a_small_ring_keeps_four_vertices_and_a_big_one_loses_the_noise() {
        // a 2 m square with a tolerance of 5 m: the first sections are always split, so all four corners stay (5 coordinates, closed)
        let small = [(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0), (0.0, 0.0)];
        let out = simplify_ring(&small, 5.0);
        assert_eq!(out.len(), 5);
        assert_eq!(out.first(), out.last());
        // a 100 m square with points on its sides: the points on the straight sides go, the corners stay
        let mut big = Vec::new();
        for i in 0..10 {
            big.push((i as f64 * 10.0, 0.0));
        }
        for i in 0..10 {
            big.push((100.0, i as f64 * 10.0));
        }
        for i in 0..10 {
            big.push((100.0 - i as f64 * 10.0, 100.0));
        }
        for i in 0..10 {
            big.push((0.0, 100.0 - i as f64 * 10.0));
        }
        big.push((0.0, 0.0));
        let out = simplify_ring(&big, 1.0);
        assert_eq!(
            out,
            vec![
                (0.0, 0.0),
                (100.0, 0.0),
                (100.0, 100.0),
                (0.0, 100.0),
                (0.0, 0.0)
            ]
        );
    }

    #[test]
    fn short_lines_are_returned_as_they_are() {
        assert_eq!(dp(&[(0.0, 0.0), (1.0, 1.0)], 5.0).len(), 2);
    }
}
