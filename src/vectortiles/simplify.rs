//! Douglas-Peucker simplification (no topology preservation), like GEOS `simplify(tolerance, preserve_topology=False)`.

fn point_segment_distance(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    if len2 == 0.0 {
        return (p.0 - a.0).hypot(p.1 - a.1);
    }
    let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0);
    (p.0 - (a.0 + t * dx)).hypot(p.1 - (a.1 + t * dy))
}

/// The end points are always kept.
pub fn douglas_peucker(points: &[(f64, f64)], epsilon: f64) -> Vec<(f64, f64)> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    let mut stack = vec![(0usize, points.len() - 1)];
    while let Some((first, last)) = stack.pop() {
        if last <= first + 1 {
            continue;
        }
        let (mut max_d, mut index) = (-1.0, first);
        for i in first + 1..last {
            let d = point_segment_distance(points[i], points[first], points[last]);
            if d > max_d {
                max_d = d;
                index = i;
            }
        }
        if max_d > epsilon {
            keep[index] = true;
            stack.push((first, index));
            stack.push((index, last));
        }
    }
    points
        .iter()
        .zip(&keep)
        .filter(|(_, k)| **k)
        .map(|(p, _)| *p)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_the_points_within_epsilon_and_keeps_the_corners_and_the_ends() {
        let noisy = [(0.0, 0.0), (1.0, 0.05), (2.0, 0.0), (3.0, 0.05), (4.0, 0.0)];
        assert_eq!(douglas_peucker(&noisy, 0.1), vec![(0.0, 0.0), (4.0, 0.0)]);
        assert_eq!(douglas_peucker(&noisy, 0.01).len(), 5);
        let corner = [(0.0, 0.0), (5.0, 0.0), (5.0, 5.0)];
        assert_eq!(douglas_peucker(&corner, 0.1), corner.to_vec());
        // a long gentle bend: the middle point is 0.5 from the chord
        let bend = [(0.0, 0.0), (5.0, 0.5), (10.0, 0.0)];
        assert_eq!(douglas_peucker(&bend, 0.4).len(), 3);
        assert_eq!(douglas_peucker(&bend, 0.6).len(), 2);
    }

    #[test]
    fn short_lines_are_returned_as_they_are() {
        assert_eq!(douglas_peucker(&[(0.0, 0.0), (1.0, 1.0)], 5.0).len(), 2);
    }
}
