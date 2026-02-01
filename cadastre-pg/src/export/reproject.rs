//! Reprojection de géométries avec PROJ
//!
//! Ce module est disponible uniquement avec le feature `reproject`.

#[cfg(feature = "reproject")]
use anyhow::{Context, Result};
#[cfg(feature = "reproject")]
use geo::{Coord, Geometry, LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon};
#[cfg(feature = "reproject")]
use proj::Proj;

/// Reprojection de géométries entre deux systèmes de coordonnées
#[cfg(feature = "reproject")]
pub struct Reprojector {
    proj: Proj,
    source_epsg: u32,
    target_epsg: u32,
}

#[cfg(feature = "reproject")]
impl Reprojector {
    /// Crée un nouveau reprojector entre deux EPSG
    pub fn new(source_epsg: u32, target_epsg: u32) -> Result<Self> {
        if source_epsg == target_epsg {
            // Pas besoin de reprojection
            return Ok(Self {
                proj: Proj::new_known_crs(
                    &format!("EPSG:{}", source_epsg),
                    &format!("EPSG:{}", target_epsg),
                    None,
                )
                .context("Failed to create identity projection")?,
                source_epsg,
                target_epsg,
            });
        }

        let source = format!("EPSG:{}", source_epsg);
        let target = format!("EPSG:{}", target_epsg);

        let proj = Proj::new_known_crs(&source, &target, None).context(format!(
            "Failed to create projection from {} to {}",
            source, target
        ))?;

        Ok(Self {
            proj,
            source_epsg,
            target_epsg,
        })
    }

    /// Retourne le SRID source
    pub fn source_epsg(&self) -> u32 {
        self.source_epsg
    }

    /// Retourne le SRID cible
    pub fn target_epsg(&self) -> u32 {
        self.target_epsg
    }

    /// Transforme une géométrie
    pub fn transform_geometry(&self, geom: &Geometry) -> Result<Geometry> {
        if self.source_epsg == self.target_epsg {
            return Ok(geom.clone());
        }

        match geom {
            Geometry::Point(p) => {
                let (x, y) = self.transform_coord(p.0)?;
                Ok(Geometry::Point(Point::new(x, y)))
            }
            Geometry::LineString(ls) => {
                let transformed = self.transform_linestring(ls)?;
                Ok(Geometry::LineString(transformed))
            }
            Geometry::Polygon(p) => {
                let transformed = self.transform_polygon(p)?;
                Ok(Geometry::Polygon(transformed))
            }
            Geometry::MultiPoint(mp) => {
                let points: Result<Vec<Point>> =
                    mp.0.iter()
                        .map(|p| {
                            let (x, y) = self.transform_coord(p.0)?;
                            Ok(Point::new(x, y))
                        })
                        .collect();
                Ok(Geometry::MultiPoint(MultiPoint::new(points?)))
            }
            Geometry::MultiLineString(mls) => {
                let lines: Result<Vec<LineString>> = mls
                    .0
                    .iter()
                    .map(|ls| self.transform_linestring(ls))
                    .collect();
                Ok(Geometry::MultiLineString(MultiLineString::new(lines?)))
            }
            Geometry::MultiPolygon(mp) => {
                let polys: Result<Vec<Polygon>> =
                    mp.0.iter().map(|p| self.transform_polygon(p)).collect();
                Ok(Geometry::MultiPolygon(MultiPolygon::new(polys?)))
            }
            // Types non supportés: retourner tel quel
            _ => Ok(geom.clone()),
        }
    }

    /// Transforme une coordonnée unique
    fn transform_coord(&self, coord: Coord) -> Result<(f64, f64)> {
        self.proj
            .convert((coord.x, coord.y))
            .context("Coordinate transformation failed")
    }

    /// Transforme une LineString (optimisé avec batch conversion)
    fn transform_linestring(&self, ls: &LineString) -> Result<LineString> {
        // Copier les coordonnées pour transformation in-place
        let mut coords: Vec<(f64, f64)> = ls.0.iter().map(|c| (c.x, c.y)).collect();

        // Transformation batch - beaucoup plus rapide que point par point
        self.proj
            .convert_array(&mut coords)
            .context("Batch coordinate transformation failed")?;

        // Convertir en Coord
        let result: Vec<Coord> = coords.into_iter().map(|(x, y)| Coord { x, y }).collect();
        Ok(LineString::new(result))
    }

    /// Transforme un Polygon
    fn transform_polygon(&self, p: &Polygon) -> Result<Polygon> {
        let exterior = self.transform_linestring(p.exterior())?;
        let interiors: Result<Vec<LineString>> = p
            .interiors()
            .iter()
            .map(|ls| self.transform_linestring(ls))
            .collect();
        Ok(Polygon::new(exterior, interiors?))
    }
}

#[cfg(feature = "reproject")]
#[cfg(test)]
mod tests {
    use super::*;
    use geo::Point;

    #[test]
    fn test_lambert93_to_wgs84() {
        // Point connu: Paris (environ)
        // Lambert-93: X=652381, Y=6862047
        // WGS84: lon=2.35, lat=48.85 (approximatif)
        let reprojector = Reprojector::new(2154, 4326).unwrap();

        let paris_l93 = Geometry::Point(Point::new(652381.0, 6862047.0));
        let paris_wgs84 = reprojector.transform_geometry(&paris_l93).unwrap();

        if let Geometry::Point(p) = paris_wgs84 {
            // Vérifier que les coordonnées sont dans la plage attendue
            assert!(
                p.x() > 2.0 && p.x() < 3.0,
                "Longitude should be around 2.35, got {}",
                p.x()
            );
            assert!(
                p.y() > 48.0 && p.y() < 49.0,
                "Latitude should be around 48.85, got {}",
                p.y()
            );
        } else {
            panic!("Expected Point geometry");
        }
    }

    #[test]
    fn test_identity_transform() {
        let reprojector = Reprojector::new(4326, 4326).unwrap();

        let point = Geometry::Point(Point::new(2.35, 48.85));
        let result = reprojector.transform_geometry(&point).unwrap();

        if let Geometry::Point(p) = result {
            assert!((p.x() - 2.35).abs() < 0.0001);
            assert!((p.y() - 48.85).abs() < 0.0001);
        } else {
            panic!("Expected Point geometry");
        }
    }

    #[test]
    fn test_polygon_transform() {
        let reprojector = Reprojector::new(2154, 4326).unwrap();

        // Petit carré en Lambert-93
        let poly = Geometry::Polygon(Polygon::new(
            LineString::from(vec![
                (652381.0, 6862047.0),
                (652481.0, 6862047.0),
                (652481.0, 6862147.0),
                (652381.0, 6862147.0),
                (652381.0, 6862047.0),
            ]),
            vec![],
        ));

        let result = reprojector.transform_geometry(&poly).unwrap();

        if let Geometry::Polygon(p) = result {
            // Vérifier que le polygone a 5 points (fermé)
            assert_eq!(p.exterior().0.len(), 5);
            // Vérifier que les coordonnées sont en WGS84
            let first = &p.exterior().0[0];
            assert!(first.x > 2.0 && first.x < 3.0);
            assert!(first.y > 48.0 && first.y < 49.0);
        } else {
            panic!("Expected Polygon geometry");
        }
    }

    #[test]
    fn test_invalid_epsg() {
        let result = Reprojector::new(99999, 4326);
        assert!(result.is_err());
    }
}

// Fonction publique sans feature pour permettre l'utilisation conditionnelle
/// Vérifie si la reprojection est disponible
pub fn is_available() -> bool {
    cfg!(feature = "reproject")
}

// Implémentation factice quand le feature reproject est désactivé
#[cfg(not(feature = "reproject"))]
use anyhow::{bail, Result};
#[cfg(not(feature = "reproject"))]
use geo::Geometry;

/// Reprojector factice - pas de reprojection disponible
#[cfg(not(feature = "reproject"))]
pub struct Reprojector;

#[cfg(not(feature = "reproject"))]
impl Reprojector {
    /// Tente de créer un reprojector - échoue toujours sans la feature
    pub fn new(source_epsg: u32, target_epsg: u32) -> Result<Self> {
        if source_epsg == target_epsg {
            Ok(Self)
        } else {
            bail!(
                "Reprojection from EPSG:{} to EPSG:{} requires the 'reproject' feature. \
                 Build with: cargo build --features reproject",
                source_epsg,
                target_epsg
            )
        }
    }

    /// Retourne la géométrie inchangée (pas de reprojection)
    pub fn transform_geometry(&self, geom: &Geometry) -> Result<Geometry> {
        Ok(geom.clone())
    }
}
