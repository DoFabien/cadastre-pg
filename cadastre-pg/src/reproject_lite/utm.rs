//! Projection UTM (Universal Transverse Mercator)
//!
//! Zones supportées:
//! - Zone 20N (EPSG:32620) - Martinique, Guadeloupe
//! - Zone 22N (EPSG:32622) - Guyane
//! - Zone 38S (EPSG:32738) - Mayotte
//! - Zone 40S (EPSG:32740) - Réunion

use super::ellipsoid::WGS84;
use super::Geographic;
use anyhow::Result;

/// Convertit UTM vers coordonnées géographiques WGS84
pub fn utm_to_geographic(x: f64, y: f64, zone: u32, south: bool) -> Result<Geographic> {
    let a = WGS84::A;
    let e2 = WGS84::E2;
    let e = WGS84::E;
    let ep2 = WGS84::EP2;

    // Paramètres UTM
    let k0 = 0.9996; // Facteur d'échelle
    let x0 = 500000.0; // False easting
    let y0 = if south { 10000000.0 } else { 0.0 }; // False northing

    // Longitude centrale de la zone
    let lon0 = ((zone as f64 - 1.0) * 6.0 - 180.0 + 3.0).to_radians();

    // Coordonnées réduites
    let x = x - x0;
    let y = y - y0;

    // Calcul du footprint latitude
    let m = y / k0;
    let mu = m / (a * (1.0 - e2 / 4.0 - 3.0 * e2.powi(2) / 64.0 - 5.0 * e2.powi(3) / 256.0));

    // Coefficients pour la série
    let e1 = (1.0 - (1.0 - e2).sqrt()) / (1.0 + (1.0 - e2).sqrt());

    let phi1 = mu
        + (3.0 * e1 / 2.0 - 27.0 * e1.powi(3) / 32.0) * (2.0 * mu).sin()
        + (21.0 * e1.powi(2) / 16.0 - 55.0 * e1.powi(4) / 32.0) * (4.0 * mu).sin()
        + (151.0 * e1.powi(3) / 96.0) * (6.0 * mu).sin()
        + (1097.0 * e1.powi(4) / 512.0) * (8.0 * mu).sin();

    // Calculs intermédiaires
    let sin_phi1 = phi1.sin();
    let cos_phi1 = phi1.cos();
    let tan_phi1 = phi1.tan();

    let n1 = a / (1.0 - e2 * sin_phi1.powi(2)).sqrt();
    let t1 = tan_phi1.powi(2);
    let c1 = ep2 * cos_phi1.powi(2);
    let r1 = a * (1.0 - e2) / (1.0 - e2 * sin_phi1.powi(2)).powf(1.5);
    let d = x / (n1 * k0);

    // Latitude
    let lat = phi1
        - (n1 * tan_phi1 / r1)
            * (d.powi(2) / 2.0
                - (5.0 + 3.0 * t1 + 10.0 * c1 - 4.0 * c1.powi(2) - 9.0 * ep2) * d.powi(4) / 24.0
                + (61.0 + 90.0 * t1 + 298.0 * c1 + 45.0 * t1.powi(2) - 252.0 * ep2 - 3.0 * c1.powi(2))
                    * d.powi(6)
                    / 720.0);

    // Longitude
    let lon = lon0
        + (d - (1.0 + 2.0 * t1 + c1) * d.powi(3) / 6.0
            + (5.0 - 2.0 * c1 + 28.0 * t1 - 3.0 * c1.powi(2) + 8.0 * ep2 + 24.0 * t1.powi(2))
                * d.powi(5)
                / 120.0)
            / cos_phi1;

    Ok(Geographic::new(lon, lat))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_martinique() {
        // Fort-de-France approximativement
        // UTM Zone 20N: 708000, 1615000
        let geo = utm_to_geographic(708000.0, 1615000.0, 20, false).unwrap();
        let (lon, lat) = geo.to_degrees();

        // Fort-de-France: -61.07°E, 14.60°N
        assert!((lon - (-61.07)).abs() < 0.2, "lon={}", lon);
        assert!((lat - 14.60).abs() < 0.2, "lat={}", lat);
    }

    #[test]
    fn test_reunion() {
        // Saint-Denis approximativement
        // UTM Zone 40S: 338000, 7691000
        let geo = utm_to_geographic(338000.0, 7691000.0, 40, true).unwrap();
        let (lon, lat) = geo.to_degrees();

        // Saint-Denis: 55.45°E, -20.88°S
        assert!((lon - 55.45).abs() < 0.2, "lon={}", lon);
        assert!((lat - (-20.88)).abs() < 0.2, "lat={}", lat);
    }

    #[test]
    fn test_guyane() {
        // Cayenne approximativement
        // UTM Zone 22N: 352000, 546000
        let geo = utm_to_geographic(352000.0, 546000.0, 22, false).unwrap();
        let (lon, lat) = geo.to_degrees();

        // Cayenne: -52.33°E, 4.93°N
        assert!((lon - (-52.33)).abs() < 0.2, "lon={}", lon);
        assert!((lat - 4.93).abs() < 0.2, "lat={}", lat);
    }
}
