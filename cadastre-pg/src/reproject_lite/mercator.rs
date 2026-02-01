//! Projection Web Mercator (EPSG:3857)
//!
//! Aussi connu sous le nom de Pseudo-Mercator ou Spherical Mercator.
//! Utilisé par Google Maps, OpenStreetMap, etc.

use super::ellipsoid::WGS84;
use super::Geographic;
use anyhow::Result;

/// Convertit coordonnées géographiques vers Web Mercator (EPSG:3857)
pub fn geographic_to_web_mercator(geo: Geographic) -> Result<(f64, f64)> {
    // Web Mercator utilise un modèle sphérique avec le rayon équatorial
    let r = WGS84::A;

    // Limiter la latitude pour éviter l'infini
    let lat = geo.lat.clamp(-85.0_f64.to_radians(), 85.0_f64.to_radians());

    // X = R * longitude
    let x = r * geo.lon;

    // Y = R * ln(tan(π/4 + lat/2))
    let y = r * (std::f64::consts::FRAC_PI_4 + lat / 2.0).tan().ln();

    Ok((x, y))
}

/// Convertit Web Mercator vers coordonnées géographiques
#[allow(dead_code)]
pub fn web_mercator_to_geographic(x: f64, y: f64) -> Result<Geographic> {
    let r = WGS84::A;

    // Longitude = x / R
    let lon = x / r;

    // Latitude = 2 * atan(exp(y/R)) - π/2
    let lat = 2.0 * (y / r).exp().atan() - std::f64::consts::FRAC_PI_2;

    Ok(Geographic::new(lon, lat))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_paris_to_web_mercator() {
        // Paris: 2.35°E, 48.85°N
        let geo = Geographic::from_degrees(2.35, 48.85);
        let (x, y) = geographic_to_web_mercator(geo).unwrap();

        // Valeurs attendues approximatives
        // X ≈ 261600
        // Y ≈ 6250000
        assert!((x - 261600.0).abs() < 1000.0, "x={}", x);
        assert!((y - 6250000.0).abs() < 10000.0, "y={}", y);
    }

    #[test]
    fn test_roundtrip() {
        let geo = Geographic::from_degrees(2.35, 48.85);
        let (x, y) = geographic_to_web_mercator(geo).unwrap();
        let geo2 = web_mercator_to_geographic(x, y).unwrap();
        let (lon, lat) = geo2.to_degrees();

        assert!((lon - 2.35).abs() < 0.001, "lon={}", lon);
        assert!((lat - 48.85).abs() < 0.001, "lat={}", lat);
    }
}
