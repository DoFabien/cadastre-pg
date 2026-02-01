//! Types de données pour le crate edigeo

use geo::Geometry;
use std::collections::HashMap;

use crate::EdigeoError;

/// Résultat du parsing d'une archive EDIGEO
#[derive(Debug)]
pub struct ParseResult {
    /// Features groupées par type (PARCELLE_id, SECTION_id, etc.)
    pub features: HashMap<String, Vec<Feature>>,

    /// Projection source détectée
    pub projection: Projection,

    /// Année du millésime
    pub year: u16,

    /// Code département (2 ou 3 caractères, ex: "01", "2A", "2B")
    pub departement: String,

    /// Erreurs non fatales rencontrées pendant le parsing
    pub errors: Vec<EdigeoError>,
}

/// Une feature cadastrale avec sa géométrie et ses attributs
#[derive(Debug, Clone)]
pub struct Feature {
    /// Identifiant unique de la feature
    pub id: String,

    /// Géométrie (Point, LineString, ou Polygon)
    pub geometry: Geometry,

    /// Attributs de la feature (clé -> valeur)
    pub properties: HashMap<String, String>,

    /// Type de feature (ex: "PARCELLE_id", "SECTION_id")
    pub feature_type: String,
}

/// Informations de projection
#[derive(Debug, Clone, Copy)]
pub struct Projection {
    /// Code EPSG
    pub epsg: u32,

    /// Nom de la projection EDIGEO
    pub name: &'static str,
}

impl Default for Projection {
    fn default() -> Self {
        Self {
            epsg: 2154, // Lambert-93 par défaut
            name: "LAMB93",
        }
    }
}

/// Données extraites du fichier THF
#[derive(Debug)]
pub struct ThfData {
    /// Encodage des fichiers texte
    pub encoding: &'static encoding_rs::Encoding,

    /// Année du millésime
    pub year: u16,
}

/// Informations de qualité d'un objet (depuis QAL)
#[derive(Debug, Clone, Default)]
pub struct Quality {
    /// Date de création
    pub create_date: Option<String>,

    /// Date de mise à jour
    pub update_date: Option<String>,

    /// Type de mise à jour
    pub update_type: Option<String>,
}
