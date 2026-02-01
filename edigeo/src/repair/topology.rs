//! Gestion de la topologie (trous, multipolygones)

use geo::{Contains, Coord, LineString, Point, Polygon};

/// Organise les rings en polygones avec trous
pub fn organize_rings(rings: Vec<LineString>) -> Vec<Polygon> {
    if rings.is_empty() {
        return Vec::new();
    }

    if rings.len() == 1 {
        return vec![Polygon::new(rings.into_iter().next().unwrap(), vec![])];
    }

    // Déterminer quels rings sont contenus dans d'autres
    let mut outer_indices: Vec<usize> = Vec::new();
    let mut inner_map: std::collections::HashMap<usize, Vec<usize>> =
        std::collections::HashMap::new();
    let mut assigned: std::collections::HashSet<usize> = std::collections::HashSet::new();

    for i in 0..rings.len() {
        let mut is_inner = false;

        for j in 0..rings.len() {
            if i == j {
                continue;
            }

            // Vérifier si le premier point de rings[i] est dans rings[j]
            if let Some(first_coord) = rings[i].0.first() {
                let point = Point::new(first_coord.x, first_coord.y);
                let poly = Polygon::new(rings[j].clone(), vec![]);

                if poly.contains(&point) {
                    // rings[i] est à l'intérieur de rings[j]
                    if !assigned.contains(&i) {
                        inner_map.entry(j).or_default().push(i);
                        assigned.insert(i);
                        is_inner = true;
                    }
                    break;
                }
            }
        }

        if !is_inner && !assigned.contains(&i) {
            outer_indices.push(i);
        }
    }

    // Construire les polygones
    outer_indices
        .into_iter()
        .map(|outer_idx| {
            let outer = rings[outer_idx].clone();
            let holes: Vec<LineString> = inner_map
                .get(&outer_idx)
                .map(|inners| inners.iter().map(|&i| rings[i].clone()).collect())
                .unwrap_or_default();
            Polygon::new(outer, holes)
        })
        .collect()
}

/// Supprime les arcs en cul-de-sac
pub fn remove_dead_ends(arcs: &mut Vec<Vec<Coord>>) {
    loop {
        let initial_count = arcs.len();

        // Compter les occurrences de chaque point (début/fin)
        let mut point_count: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();

        for arc in arcs.iter() {
            if let Some(first) = arc.first() {
                let key = format!("{:.6},{:.6}", first.x, first.y);
                *point_count.entry(key).or_insert(0) += 1;
            }
            if let Some(last) = arc.last() {
                let key = format!("{:.6},{:.6}", last.x, last.y);
                *point_count.entry(key).or_insert(0) += 1;
            }
        }

        // Supprimer les arcs dont un point n'apparaît qu'une fois
        arcs.retain(|arc| {
            let first_key = arc
                .first()
                .map(|c| format!("{:.6},{:.6}", c.x, c.y))
                .unwrap_or_default();
            let last_key = arc
                .last()
                .map(|c| format!("{:.6},{:.6}", c.x, c.y))
                .unwrap_or_default();

            let first_count = point_count.get(&first_key).copied().unwrap_or(0);
            let last_count = point_count.get(&last_key).copied().unwrap_or(0);

            first_count >= 2 && last_count >= 2
        });

        // Si aucun arc n'a été supprimé, on arrête
        if arcs.len() == initial_count {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_organize_single_ring() {
        let ring = LineString::new(vec![
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 1.0, y: 0.0 },
            Coord { x: 1.0, y: 1.0 },
            Coord { x: 0.0, y: 0.0 },
        ]);

        let polygons = organize_rings(vec![ring]);
        assert_eq!(polygons.len(), 1);
        assert!(polygons[0].interiors().is_empty());
    }
}
