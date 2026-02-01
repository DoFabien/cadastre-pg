//! Projection Lambert 93 (EPSG:2154)
//!
//! Lambert Conformal Conic avec 2 parallèles standards

use super::ellipsoid::GRS80;
use super::Geographic;
use anyhow::Result;

/// Paramètres Lambert 93 (EPSG:2154)
struct Lambert93 {
    /// Longitude origine (méridien de Paris en RGF93 = Greenwich)
    lon0: f64,
    /// Latitude origine
    lat0: f64,
    /// Premier parallèle standard
    lat1: f64,
    /// Deuxième parallèle standard
    lat2: f64,
    /// False easting
    x0: f64,
    /// False northing
    y0: f64,
}

impl Default for Lambert93 {
    fn default() -> Self {
        Self {
            lon0: 3.0_f64.to_radians(),          // 3°E
            lat0: 46.5_f64.to_radians(),         // 46.5°N
            lat1: 44.0_f64.to_radians(),         // 44°N
            lat2: 49.0_f64.to_radians(),         // 49°N
            x0: 700000.0,                        // False easting
            y0: 6600000.0,                       // False northing
        }
    }
}

/// Calcule la latitude isométrique
fn isometric_latitude(lat: f64, e: f64) -> f64 {
    let sin_lat = lat.sin();
    let term = ((1.0 - e * sin_lat) / (1.0 + e * sin_lat)).powf(e / 2.0);
    ((std::f64::consts::FRAC_PI_4 + lat / 2.0).tan() * term).ln()
}

/// Calcule la latitude depuis la latitude isométrique (itératif)
fn latitude_from_isometric(iso_lat: f64, e: f64) -> f64 {
    let mut lat = 2.0 * iso_lat.exp().atan() - std::f64::consts::FRAC_PI_2;

    for _ in 0..10 {
        let sin_lat = lat.sin();
        let term = ((1.0 + e * sin_lat) / (1.0 - e * sin_lat)).powf(e / 2.0);
        let new_lat = 2.0 * (iso_lat.exp() * term).atan() - std::f64::consts::FRAC_PI_2;

        if (new_lat - lat).abs() < 1e-12 {
            return new_lat;
        }
        lat = new_lat;
    }
    lat
}

/// Calcule le grand normal (rayon de courbure dans le plan vertical)
fn grande_normale(lat: f64, a: f64, e2: f64) -> f64 {
    a / (1.0 - e2 * lat.sin().powi(2)).sqrt()
}

/// Convertit Lambert 93 vers coordonnées géographiques WGS84
pub fn lambert93_to_geographic(x: f64, y: f64) -> Result<Geographic> {
    let params = Lambert93::default();
    let e = GRS80::E;
    let e2 = GRS80::E2;
    let a = GRS80::A;

    // Calcul des constantes de la projection
    let n1 = grande_normale(params.lat1, a, e2);
    let n2 = grande_normale(params.lat2, a, e2);

    let iso_lat1 = isometric_latitude(params.lat1, e);
    let iso_lat2 = isometric_latitude(params.lat2, e);
    let iso_lat0 = isometric_latitude(params.lat0, e);

    // Exposant de la projection
    let n = (n1 * params.lat1.cos()).ln() - (n2 * params.lat2.cos()).ln();
    let n = n / (iso_lat2 - iso_lat1);

    // Constante C
    let c = (n1 * params.lat1.cos() / n) * (n * iso_lat1).exp();

    // Rayon à l'origine
    let r0 = c * (-n * iso_lat0).exp();

    // Coordonnées centrées
    let dx = x - params.x0;
    let dy = y - params.y0;

    // Rayon et angle
    let r = (dx.powi(2) + (r0 - dy).powi(2)).sqrt();
    let r = if n < 0.0 { -r } else { r };

    let gamma = (dx / (r0 - dy)).atan();

    // Latitude isométrique
    let iso_lat = -(r / c).ln() / n;

    // Latitude géographique
    let lat = latitude_from_isometric(iso_lat, e);

    // Longitude
    let lon = params.lon0 + gamma / n;

    Ok(Geographic::new(lon, lat))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_paris() {
        // Tour Eiffel approximativement
        let geo = lambert93_to_geographic(648237.0, 6862107.0).unwrap();
        let (lon, lat) = geo.to_degrees();

        // Tour Eiffel: 2.2945°E, 48.8584°N
        assert!((lon - 2.2945).abs() < 0.01, "lon={}", lon);
        assert!((lat - 48.8584).abs() < 0.01, "lat={}", lat);
    }

    #[test]
    fn test_marseille() {
        // Vieux-Port approximativement
        let geo = lambert93_to_geographic(893193.0, 6245829.0).unwrap();
        let (lon, lat) = geo.to_degrees();

        // Marseille: 5.37°E, 43.30°N
        assert!((lon - 5.37).abs() < 0.1, "lon={}", lon);
        assert!((lat - 43.30).abs() < 0.1, "lat={}", lat);
    }
}
