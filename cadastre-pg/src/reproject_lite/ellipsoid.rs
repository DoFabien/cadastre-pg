//! Définitions des ellipsoïdes

/// Ellipsoïde WGS84
pub struct WGS84;

impl WGS84 {
    /// Demi-grand axe (rayon équatorial) en mètres
    pub const A: f64 = 6378137.0;

    /// Aplatissement
    pub const F: f64 = 1.0 / 298.257223563;

    /// Demi-petit axe (rayon polaire) en mètres
    pub const B: f64 = Self::A * (1.0 - Self::F);

    /// Première excentricité au carré
    pub const E2: f64 = 2.0 * Self::F - Self::F * Self::F;

    /// Première excentricité
    pub const E: f64 = 0.0818191908426215; // sqrt(E2)

    /// Deuxième excentricité au carré
    pub const EP2: f64 = Self::E2 / (1.0 - Self::E2);
}

/// Ellipsoïde GRS80 (utilisé par Lambert 93)
/// Note: Quasi identique à WGS84, différence < 0.1mm
pub struct GRS80;

impl GRS80 {
    pub const A: f64 = 6378137.0;
    pub const F: f64 = 1.0 / 298.257222101;
    pub const E2: f64 = 2.0 * Self::F - Self::F * Self::F;
    pub const E: f64 = 0.0818191910428158; // sqrt(E2)
}
