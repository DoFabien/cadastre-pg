//! Fallback convex hull pour les géométries non réparables

use geo::{ConvexHull, Coord, MultiPoint, Point, Polygon};

use crate::EdigeoError;

/// Calcule un convex hull à partir des arcs
pub fn convex_hull_fallback(arcs: &[Vec<Coord>]) -> Result<Polygon, EdigeoError> {
    // Collecter tous les points
    let all_points: Vec<Point> = arcs
        .iter()
        .flat_map(|arc| arc.iter().map(|c| Point::new(c.x, c.y)))
        .collect();

    if all_points.len() < 3 {
        return Err(EdigeoError::RepairFailed {
            entity_id: "unknown".to_string(),
            reason: "Not enough points for convex hull".to_string(),
        });
    }

    let multi_point = MultiPoint::new(all_points);
    let hull = multi_point.convex_hull();

    Ok(hull)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convex_hull() {
        let arcs = vec![
            vec![Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 0.0 }],
            vec![Coord { x: 0.5, y: 1.0 }],
        ];

        let result = convex_hull_fallback(&arcs);
        assert!(result.is_ok());
    }
}
