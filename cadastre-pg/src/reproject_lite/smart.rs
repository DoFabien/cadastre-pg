//! Reprojection intelligente : reproject_lite en priorité, fallback sur proj
//!
//! Utilise automatiquement la meilleure option disponible.

use super::ReprojectorLite;
use anyhow::{bail, Result};
use geo::Geometry;

/// Reprojection intelligente
///
/// Essaie d'abord reproject_lite (pure Rust), puis fallback sur proj si disponible.
pub enum SmartReprojector {
    /// Reprojection légère (pure Rust)
    Lite(ReprojectorLite),
    /// Reprojection via PROJ (si feature activée)
    #[cfg(feature = "reproject")]
    Proj(crate::export::reproject::Reprojector),
    /// Pas de reprojection (source == cible)
    Identity,
}

impl SmartReprojector {
    /// Crée un nouveau reprojector
    pub fn new(source_epsg: u32, target_epsg: u32) -> Result<Self> {
        // Pas de reprojection nécessaire
        if source_epsg == target_epsg {
            return Ok(Self::Identity);
        }

        // Essayer reproject_lite d'abord
        if ReprojectorLite::is_supported(source_epsg, target_epsg) {
            let lite = ReprojectorLite::new(source_epsg, target_epsg)?;
            return Ok(Self::Lite(lite));
        }

        // Fallback sur proj si disponible
        #[cfg(feature = "reproject")]
        {
            let proj = crate::export::reproject::Reprojector::new(source_epsg, target_epsg)?;
            return Ok(Self::Proj(proj));
        }

        // Aucune option disponible
        #[cfg(not(feature = "reproject"))]
        bail!(
            "Reprojection EPSG:{} → EPSG:{} non supportée.\n\
             Projections supportées (reproject_lite) :\n\
             - Sources: 2154 (Lambert 93), 32620/32622/32738/32740 (UTM DOM)\n\
             - Cibles: 4326 (WGS84), 3857 (Web Mercator)\n\
             Pour d'autres projections, compilez avec: cargo build --features reproject",
            source_epsg,
            target_epsg
        );
    }

    /// Transforme une géométrie
    pub fn transform_geometry(&self, geom: &Geometry) -> Result<Geometry> {
        match self {
            Self::Identity => Ok(geom.clone()),
            Self::Lite(lite) => lite.transform_geometry(geom),
            #[cfg(feature = "reproject")]
            Self::Proj(proj) => proj.transform_geometry(geom),
        }
    }

    /// Retourne une description du reprojector utilisé
    pub fn description(&self) -> &'static str {
        match self {
            Self::Identity => "identity (pas de reprojection)",
            Self::Lite(_) => "reproject_lite (pure Rust)",
            #[cfg(feature = "reproject")]
            Self::Proj(_) => "proj (PROJ library)",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identity() {
        let r = SmartReprojector::new(4326, 4326).unwrap();
        assert!(matches!(r, SmartReprojector::Identity));
    }

    #[test]
    fn test_lite() {
        let r = SmartReprojector::new(2154, 4326).unwrap();
        assert!(matches!(r, SmartReprojector::Lite(_)));
    }

    #[test]
    fn test_lambert93_to_3857() {
        let r = SmartReprojector::new(2154, 3857).unwrap();
        assert!(matches!(r, SmartReprojector::Lite(_)));
    }
}
