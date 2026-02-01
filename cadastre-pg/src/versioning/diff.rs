//! Calcul de hash pour comparaison de géométries
//!
//! Le hash est normalisé pour être indépendant de l'ordre de départ des anneaux
//! (un polygone qui commence à un vertex différent aura le même hash).

use blake3::Hasher;
use geo::{Coord, Geometry, LineString};

/// Calcule un hash stable d'une géométrie
///
/// Les anneaux de polygones sont normalisés pour commencer au vertex
/// lexicographiquement le plus petit (min x, puis min y).
pub fn geometry_hash(geom: &Geometry) -> [u8; 32] {
    let mut hasher = Hasher::new();

    match geom {
        Geometry::Point(p) => {
            hasher.update(b"POINT");
            hash_coord(&mut hasher, p.0);
        }
        Geometry::LineString(ls) => {
            hasher.update(b"LINESTRING");
            for coord in ls.0.iter() {
                hash_coord(&mut hasher, *coord);
            }
        }
        Geometry::Polygon(p) => {
            hasher.update(b"POLYGON");
            hasher.update(b"EXT");
            hash_ring_normalized(&mut hasher, p.exterior());
            for interior in p.interiors() {
                hasher.update(b"INT");
                hash_ring_normalized(&mut hasher, interior);
            }
        }
        Geometry::MultiPolygon(mp) => {
            hasher.update(b"MULTIPOLYGON");
            for poly in mp.0.iter() {
                hasher.update(b"POLY");
                hasher.update(b"EXT");
                hash_ring_normalized(&mut hasher, poly.exterior());
                for interior in poly.interiors() {
                    hasher.update(b"INT");
                    hash_ring_normalized(&mut hasher, interior);
                }
            }
        }
        Geometry::MultiPoint(mp) => {
            hasher.update(b"MULTIPOINT");
            for point in mp.0.iter() {
                hash_coord(&mut hasher, point.0);
            }
        }
        Geometry::MultiLineString(mls) => {
            hasher.update(b"MULTILINESTRING");
            for ls in mls.0.iter() {
                hasher.update(b"LS");
                for coord in ls.0.iter() {
                    hash_coord(&mut hasher, *coord);
                }
            }
        }
        _ => {
            hasher.update(format!("{:?}", geom).as_bytes());
        }
    }

    *hasher.finalize().as_bytes()
}

/// Hash un anneau (ring) de polygone en le normalisant
/// pour commencer au vertex lexicographiquement le plus petit.
fn hash_ring_normalized(hasher: &mut Hasher, ring: &LineString) {
    if ring.0.is_empty() {
        return;
    }

    // Trouver l'index du vertex lexicographiquement le plus petit
    // (ignore le dernier point qui est identique au premier pour un ring fermé)
    let len = if ring.0.len() > 1 && ring.0.first() == ring.0.last() {
        ring.0.len() - 1
    } else {
        ring.0.len()
    };

    if len == 0 {
        return;
    }

    let min_idx = (0..len)
        .min_by(|&a, &b| {
            let ca = &ring.0[a];
            let cb = &ring.0[b];
            ca.x.partial_cmp(&cb.x)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| ca.y.partial_cmp(&cb.y).unwrap_or(std::cmp::Ordering::Equal))
        })
        .unwrap_or(0);

    // Hash les coordonnées en commençant par min_idx
    for i in 0..len {
        let idx = (min_idx + i) % len;
        hash_coord(hasher, ring.0[idx]);
    }
}

/// Hash une coordonnée avec arrondi pour stabilité
fn hash_coord(hasher: &mut Hasher, coord: Coord) {
    // Arrondir à 6 décimales (précision ~10cm)
    let x = (coord.x * 1_000_000.0).round() as i64;
    let y = (coord.y * 1_000_000.0).round() as i64;
    hasher.update(&x.to_le_bytes());
    hasher.update(&y.to_le_bytes());
}

/// Compare deux hashes
pub fn hashes_equal(a: &[u8; 32], b: &[u8; 32]) -> bool {
    a == b
}

/// Convertit un hash en hexadécimal
pub fn hash_to_hex(hash: &[u8; 32]) -> String {
    hash.iter().map(|b| format!("{:02x}", b)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo::{LineString, Point, Polygon};

    #[test]
    fn test_same_geometry_same_hash() {
        let p1 = Geometry::Point(Point::new(1.0, 2.0));
        let p2 = Geometry::Point(Point::new(1.0, 2.0));

        assert_eq!(geometry_hash(&p1), geometry_hash(&p2));
    }

    #[test]
    fn test_different_geometry_different_hash() {
        let p1 = Geometry::Point(Point::new(1.0, 2.0));
        let p2 = Geometry::Point(Point::new(1.0, 3.0));

        assert_ne!(geometry_hash(&p1), geometry_hash(&p2));
    }

    #[test]
    fn test_polygon_hash() {
        let poly = Geometry::Polygon(Polygon::new(
            LineString::from(vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 0.0)]),
            vec![],
        ));

        let hash = geometry_hash(&poly);
        assert_eq!(hash.len(), 32);
    }

    #[test]
    fn test_polygon_same_hash_different_start() {
        // Même polygone mais commençant à des vertices différents
        let poly1 = Geometry::Polygon(Polygon::new(
            LineString::from(vec![
                (0.0, 0.0),
                (1.0, 0.0),
                (1.0, 1.0),
                (0.0, 1.0),
                (0.0, 0.0),
            ]),
            vec![],
        ));

        let poly2 = Geometry::Polygon(Polygon::new(
            LineString::from(vec![
                (1.0, 0.0),
                (1.0, 1.0),
                (0.0, 1.0),
                (0.0, 0.0),
                (1.0, 0.0),
            ]),
            vec![],
        ));

        let poly3 = Geometry::Polygon(Polygon::new(
            LineString::from(vec![
                (0.0, 1.0),
                (0.0, 0.0),
                (1.0, 0.0),
                (1.0, 1.0),
                (0.0, 1.0),
            ]),
            vec![],
        ));

        let hash1 = geometry_hash(&poly1);
        let hash2 = geometry_hash(&poly2);
        let hash3 = geometry_hash(&poly3);

        assert_eq!(hash1, hash2, "Same polygon starting at different vertex should have same hash");
        assert_eq!(hash1, hash3, "Same polygon starting at different vertex should have same hash");
    }
}
