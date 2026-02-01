//! Reprojection légère en Rust pur (sans dépendances externes)
//!
//! Supporte les projections du cadastre français :
//! - Lambert 93 (EPSG:2154) - Métropole
//! - UTM 20N (EPSG:32620) - Martinique, Guadeloupe
//! - UTM 22N (EPSG:32622) - Guyane
//! - UTM 40S (EPSG:32740) - Réunion
//! - UTM 38S (EPSG:32738) - Mayotte
//!
//! Cibles supportées :
//! - WGS84 (EPSG:4326)
//! - Web Mercator (EPSG:3857)

mod ellipsoid;
mod lambert;
mod mercator;
mod smart;
mod utm;

pub use smart::SmartReprojector;

use anyhow::{bail, Result};
use geo::{Coord, Geometry, LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon};

pub use ellipsoid::WGS84;

/// Point en coordonnées géographiques (radians)
#[derive(Debug, Clone, Copy)]
pub struct Geographic {
    /// Longitude en radians
    pub lon: f64,
    /// Latitude en radians
    pub lat: f64,
}

impl Geographic {
    pub fn new(lon: f64, lat: f64) -> Self {
        Self { lon, lat }
    }

    /// Convertit en degrés
    pub fn to_degrees(self) -> (f64, f64) {
        (self.lon.to_degrees(), self.lat.to_degrees())
    }

    /// Crée depuis des degrés
    pub fn from_degrees(lon_deg: f64, lat_deg: f64) -> Self {
        Self {
            lon: lon_deg.to_radians(),
            lat: lat_deg.to_radians(),
        }
    }
}

/// Reprojection légère pour le cadastre français
pub struct ReprojectorLite {
    source_epsg: u32,
    target_epsg: u32,
}

impl ReprojectorLite {
    /// Crée un nouveau reprojector
    pub fn new(source_epsg: u32, target_epsg: u32) -> Result<Self> {
        // Vérifier que les EPSG sont supportés
        if !Self::is_supported_source(source_epsg) {
            bail!(
                "EPSG:{} non supporté. Sources supportées: 2154, 32620, 32622, 32738, 32740",
                source_epsg
            );
        }
        if !Self::is_supported_target(target_epsg) {
            bail!(
                "EPSG:{} non supporté. Cibles supportées: 4326, 3857",
                target_epsg
            );
        }

        Ok(Self {
            source_epsg,
            target_epsg,
        })
    }

    /// Vérifie si l'EPSG source est supporté
    pub fn is_supported_source(epsg: u32) -> bool {
        matches!(epsg, 2154 | 32620 | 32622 | 32738 | 32740)
    }

    /// Vérifie si l'EPSG cible est supporté
    pub fn is_supported_target(epsg: u32) -> bool {
        matches!(epsg, 4326 | 3857)
    }

    /// Vérifie si la reprojection est supportée
    pub fn is_supported(source: u32, target: u32) -> bool {
        Self::is_supported_source(source) && Self::is_supported_target(target)
    }

    /// Transforme un point (x, y) de la source vers la cible
    pub fn transform_point(&self, x: f64, y: f64) -> Result<(f64, f64)> {
        // Étape 1: Source → Géographique (WGS84)
        let geo = self.source_to_geographic(x, y)?;

        // Étape 2: Géographique → Cible
        self.geographic_to_target(geo)
    }

    /// Convertit les coordonnées source en géographique (WGS84)
    fn source_to_geographic(&self, x: f64, y: f64) -> Result<Geographic> {
        match self.source_epsg {
            2154 => lambert::lambert93_to_geographic(x, y),
            32620 => utm::utm_to_geographic(x, y, 20, false),
            32622 => utm::utm_to_geographic(x, y, 22, false),
            32738 => utm::utm_to_geographic(x, y, 38, true),
            32740 => utm::utm_to_geographic(x, y, 40, true),
            _ => bail!("EPSG:{} non supporté", self.source_epsg),
        }
    }

    /// Convertit les coordonnées géographiques vers la cible
    fn geographic_to_target(&self, geo: Geographic) -> Result<(f64, f64)> {
        match self.target_epsg {
            4326 => {
                let (lon, lat) = geo.to_degrees();
                Ok((lon, lat))
            }
            3857 => mercator::geographic_to_web_mercator(geo),
            _ => bail!("EPSG:{} non supporté", self.target_epsg),
        }
    }

    /// Transforme une géométrie
    pub fn transform_geometry(&self, geom: &Geometry) -> Result<Geometry> {
        match geom {
            Geometry::Point(p) => {
                let (x, y) = self.transform_point(p.x(), p.y())?;
                Ok(Geometry::Point(Point::new(x, y)))
            }
            Geometry::LineString(ls) => {
                let coords: Result<Vec<Coord>> = ls
                    .coords()
                    .map(|c| {
                        let (x, y) = self.transform_point(c.x, c.y)?;
                        Ok(Coord { x, y })
                    })
                    .collect();
                Ok(Geometry::LineString(LineString::new(coords?)))
            }
            Geometry::Polygon(poly) => {
                let exterior: Result<Vec<Coord>> = poly
                    .exterior()
                    .coords()
                    .map(|c| {
                        let (x, y) = self.transform_point(c.x, c.y)?;
                        Ok(Coord { x, y })
                    })
                    .collect();
                let interiors: Result<Vec<LineString>> = poly
                    .interiors()
                    .iter()
                    .map(|ring| {
                        let coords: Result<Vec<Coord>> = ring
                            .coords()
                            .map(|c| {
                                let (x, y) = self.transform_point(c.x, c.y)?;
                                Ok(Coord { x, y })
                            })
                            .collect();
                        Ok(LineString::new(coords?))
                    })
                    .collect();
                Ok(Geometry::Polygon(Polygon::new(
                    LineString::new(exterior?),
                    interiors?,
                )))
            }
            Geometry::MultiPoint(mp) => {
                let points: Result<Vec<Point>> = mp
                    .iter()
                    .map(|p| {
                        let (x, y) = self.transform_point(p.x(), p.y())?;
                        Ok(Point::new(x, y))
                    })
                    .collect();
                Ok(Geometry::MultiPoint(MultiPoint::new(points?)))
            }
            Geometry::MultiLineString(mls) => {
                let lines: Result<Vec<LineString>> = mls
                    .iter()
                    .map(|ls| {
                        let coords: Result<Vec<Coord>> = ls
                            .coords()
                            .map(|c| {
                                let (x, y) = self.transform_point(c.x, c.y)?;
                                Ok(Coord { x, y })
                            })
                            .collect();
                        Ok(LineString::new(coords?))
                    })
                    .collect();
                Ok(Geometry::MultiLineString(MultiLineString::new(lines?)))
            }
            Geometry::MultiPolygon(mp) => {
                let polys: Result<Vec<Polygon>> = mp
                    .iter()
                    .map(|poly| {
                        let exterior: Result<Vec<Coord>> = poly
                            .exterior()
                            .coords()
                            .map(|c| {
                                let (x, y) = self.transform_point(c.x, c.y)?;
                                Ok(Coord { x, y })
                            })
                            .collect();
                        let interiors: Result<Vec<LineString>> = poly
                            .interiors()
                            .iter()
                            .map(|ring| {
                                let coords: Result<Vec<Coord>> = ring
                                    .coords()
                                    .map(|c| {
                                        let (x, y) = self.transform_point(c.x, c.y)?;
                                        Ok(Coord { x, y })
                                    })
                                    .collect();
                                Ok(LineString::new(coords?))
                            })
                            .collect();
                        Ok(Polygon::new(LineString::new(exterior?), interiors?))
                    })
                    .collect();
                Ok(Geometry::MultiPolygon(MultiPolygon::new(polys?)))
            }
            _ => bail!("Type de géométrie non supporté"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lambert93_to_wgs84() {
        // Paris (environ)
        let reproj = ReprojectorLite::new(2154, 4326).unwrap();
        let (lon, lat) = reproj.transform_point(652381.0, 6862047.0).unwrap();

        // Paris est à environ 2.35°E, 48.85°N
        assert!((lon - 2.35).abs() < 0.1, "lon={}", lon);
        assert!((lat - 48.85).abs() < 0.1, "lat={}", lat);
    }

    #[test]
    fn test_utm_to_wgs84() {
        // Fort-de-France, Martinique (environ 14.6°N, -61.0°W)
        let reproj = ReprojectorLite::new(32620, 4326).unwrap();
        let (lon, lat) = reproj.transform_point(708000.0, 1615000.0).unwrap();

        assert!((lon - (-61.0)).abs() < 0.5, "lon={}", lon);
        assert!((lat - 14.6).abs() < 0.5, "lat={}", lat);
    }

    #[test]
    fn test_unsupported_epsg() {
        assert!(ReprojectorLite::new(4326, 4326).is_err());
        assert!(ReprojectorLite::new(2154, 2154).is_err());
    }
}
