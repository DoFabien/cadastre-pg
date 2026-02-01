//! Reconstruction des rings à partir des arcs

use geo::{Coord, LineString};

use crate::EdigeoError;

/// Reconstruit des rings fermés à partir d'arcs non ordonnés
pub fn reconstruct_rings(arcs: &[Vec<Coord>]) -> Result<Vec<LineString>, EdigeoError> {
    if arcs.is_empty() {
        return Ok(Vec::new());
    }

    let mut remaining: Vec<Vec<Coord>> = arcs.to_vec();
    let mut rings = Vec::new();

    // D'abord, extraire les arcs qui bouclent sur eux-mêmes
    remaining.retain(|arc| {
        if arc.len() > 3 && coords_equal(arc[0], arc[arc.len() - 1]) {
            rings.push(LineString::new(arc.clone()));
            false
        } else {
            true
        }
    });

    if remaining.is_empty() {
        return Ok(rings);
    }

    // Reconstruire les rings à partir des arcs restants
    while !remaining.is_empty() {
        let mut ring = remaining.pop().unwrap();

        let mut made_progress = true;
        while made_progress && !remaining.is_empty() {
            made_progress = false;
            let ring_first = ring[0];
            let ring_last = ring[ring.len() - 1];

            for i in (0..remaining.len()).rev() {
                let arc = &remaining[i];
                let arc_first = arc[0];
                let arc_last = arc[arc.len() - 1];

                if coords_equal(ring_last, arc_first) {
                    // Cas 1: ring_last == arc_first → ajouter tel quel
                    let arc = remaining.swap_remove(i); // O(1) au lieu de O(n)
                    ring.pop(); // Éviter doublon
                    ring.extend(arc);
                    made_progress = true;
                    break;
                } else if coords_equal(ring_last, arc_last) {
                    // Cas 2: ring_last == arc_last → ajouter reversé
                    let arc = remaining.swap_remove(i);
                    ring.pop();
                    ring.extend(arc.into_iter().rev());
                    made_progress = true;
                    break;
                } else if coords_equal(ring_first, arc_last) {
                    // Cas 3: ring_first == arc_last → insérer au début
                    let mut new_ring = remaining.swap_remove(i);
                    new_ring.pop();
                    new_ring.extend(ring);
                    ring = new_ring;
                    made_progress = true;
                    break;
                } else if coords_equal(ring_first, arc_first) {
                    // Cas 4: ring_first == arc_first → insérer reversé au début
                    let arc = remaining.swap_remove(i);
                    let mut reversed: Vec<Coord> = arc.into_iter().rev().collect();
                    reversed.pop();
                    reversed.extend(ring);
                    ring = reversed;
                    made_progress = true;
                    break;
                }
            }
        }

        // Vérifier si le ring est fermé
        let is_closed = ring.len() > 1 && coords_equal(ring[0], ring[ring.len() - 1]);

        if is_closed && ring.len() > 3 {
            rings.push(LineString::new(ring));
        } else if ring.len() > 3 {
            // Ring non fermé: fermeture automatique avec log
            let gap = ((ring[0].x - ring[ring.len() - 1].x).powi(2)
                + (ring[0].y - ring[ring.len() - 1].y).powi(2))
            .sqrt();
            tracing::warn!(
                points = ring.len(),
                gap_meters = gap,
                "Auto-closing unclosed ring"
            );
            let first = ring[0];
            ring.push(first);
            rings.push(LineString::new(ring));
        }
    }

    if rings.is_empty() {
        Err(EdigeoError::RepairFailed {
            entity_id: "unknown".to_string(),
            reason: "Could not reconstruct any closed rings".to_string(),
        })
    } else {
        Ok(rings)
    }
}

/// Compare deux coordonnées avec tolérance
fn coords_equal(a: Coord, b: Coord) -> bool {
    const TOLERANCE: f64 = 1e-6;
    (a.x - b.x).abs() < TOLERANCE && (a.y - b.y).abs() < TOLERANCE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reconstruct_simple_ring() {
        let arcs = vec![
            vec![Coord { x: 0.0, y: 0.0 }, Coord { x: 1.0, y: 0.0 }],
            vec![Coord { x: 1.0, y: 0.0 }, Coord { x: 1.0, y: 1.0 }],
            vec![Coord { x: 1.0, y: 1.0 }, Coord { x: 0.0, y: 1.0 }],
            vec![Coord { x: 0.0, y: 1.0 }, Coord { x: 0.0, y: 0.0 }],
        ];

        let result = reconstruct_rings(&arcs);
        assert!(result.is_ok());
        let rings = result.unwrap();
        assert_eq!(rings.len(), 1);
    }

    #[test]
    fn test_self_closing_arc() {
        let arcs = vec![vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 0.0, y: 0.0 },
        ]];

        let result = reconstruct_rings(&arcs);
        assert!(result.is_ok());
        let rings = result.unwrap();
        assert_eq!(rings.len(), 1);
    }
}
