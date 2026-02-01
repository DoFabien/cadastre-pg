//! # edigeo
//!
//! Parser pour le format EDIGEO (AFNOR NF Z 52000) utilisé par le cadastre français.
//!
//! ## Features
//!
//! - Parsing SIMD optimisé avec `memchr` et `simdutf8`
//! - Support de tous les fichiers EDIGEO (THF, GEO, QAL, VEC)
//! - Réparation automatique des géométries invalides
//! - Types `geo` pour l'interopérabilité avec l'écosystème Rust géospatial
//!
//! ## Usage
//!
//! ```rust,ignore
//! use edigeo::parse;
//! use std::path::Path;
//!
//! let result = parse(Path::new("archive.tar.bz2"))?;
//! println!("Année: {}", result.year);
//! println!("EPSG: {}", result.projection.epsg);
//!
//! for (table_name, features) in &result.features {
//!     println!("{}: {} features", table_name, features.len());
//! }
//! ```

pub mod archive;
pub mod error;
pub mod parser;
pub mod repair;
pub mod types;

pub use error::EdigeoError;
pub use types::{Feature, ParseResult, Projection};

use std::path::Path;

/// Extrait le code département depuis le nom de fichier EDIGEO
/// Format attendu: EDIGEO-CCXXXXX.tar.bz2 où CC est le département
/// Supporte les départements avec 2 chiffres (01-19), 2A, 2B pour la Corse
pub fn extract_departement(path: &Path) -> Option<String> {
    let filename = path.file_stem()?.to_str()?;
    // Supprimer l'extension .tar si présente
    let base = filename.trim_end_matches(".tar");

    // Format: EDIGEO-CCXXXXX ou EDIGEO-CC
    // Le département est après le premier tiret et fait 2 ou 3 caractères
    if let Some(pos) = base.find("EDIGEO-") {
        let after_prefix = &base[pos + 7..]; // Après "EDIGEO-"
                                             // Prendre les 2 ou 3 premiers caractères (pour 2A, 2B)
        if after_prefix.len() >= 2 {
            let dep = &after_prefix[..2];
            // Vérifier si c'est 2A ou 2B (Corse)
            if dep == "2A" || dep == "2B" {
                return Some(dep.to_string());
            }
            // Sinon prendre juste les 2 chiffres
            return Some(dep.to_string());
        }
    }

    // Fallback: essayer de trouver un pattern avec tiret
    if let Some(dash_pos) = base.find('-') {
        let after_dash = &base[dash_pos + 1..];
        if after_dash.len() >= 2 {
            let candidate = &after_dash[..2];
            // Vérifier que ça ressemble à un département
            if candidate.chars().all(|c| c.is_ascii_digit())
                || candidate == "2A"
                || candidate == "2B"
            {
                return Some(candidate.to_string());
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_departement() {
        assert_eq!(
            extract_departement(Path::new("EDIGEO-380910000C01.tar.bz2")),
            Some("38".to_string())
        );
        assert_eq!(
            extract_departement(Path::new("EDIGEO-2A0010001A01.tar.bz2")),
            Some("2A".to_string())
        );
        assert_eq!(
            extract_departement(Path::new("EDIGEO-01.tar.bz2")),
            Some("01".to_string())
        );
        assert_eq!(
            extract_departement(Path::new("EDIGEO-2B.tar.bz2")),
            Some("2B".to_string())
        );
        assert_eq!(extract_departement(Path::new("fichier-invalide.txt")), None);
    }
}

/// Parse une archive EDIGEO (.tar.bz2) et retourne les features géographiques.
///
/// # Arguments
///
/// * `archive_path` - Chemin vers l'archive .tar.bz2
///
/// # Returns
///
/// Un `ParseResult` contenant les features groupées par type, la projection source,
/// l'année du millésime, le code département, et les erreurs non fatales rencontrées.
///
/// # Errors
///
/// Retourne `EdigeoError` si l'archive est illisible ou si aucun fichier THF n'est trouvé.
pub fn parse(archive_path: &Path) -> Result<ParseResult, EdigeoError> {
    // 1. Extraire le département depuis le nom de fichier
    let departement = extract_departement(archive_path).unwrap_or_else(|| "00".to_string());

    // 2. Extraire l'archive
    let archive_data = archive::extract(archive_path)?;

    // 3. Parser les métadonnées
    let thf = parser::thf::parse(&archive_data.thf)?;
    let projection = parser::geo::parse(&archive_data.geo)?;
    let quality = parser::qal::parse(&archive_data.qal)?;

    // 4. Parser les VEC et construire les géométries
    // Note: Le parallélisme est géré au niveau des archives (--jobs), pas ici
    let mut all_features = std::collections::HashMap::new();
    let mut errors = Vec::new();

    for vec_data in &archive_data.vec {
        // Décoder avec le bon encodage
        let decoded = decode_with_encoding(vec_data, thf.encoding);

        match parser::vec::parse(&decoded) {
            Ok(parsed_vec) => {
                // Construire les géométries depuis les entités parsées
                match repair::build_geometries(&parsed_vec, &quality) {
                    Ok(features) => {
                        for feature in features {
                            let feature_type = feature.feature_type.clone();
                            all_features
                                .entry(feature_type)
                                .or_insert_with(Vec::new)
                                .push(feature);
                        }
                    }
                    Err(e) => errors.push(e),
                }
            }
            Err(e) => errors.push(e),
        }
    }

    Ok(ParseResult {
        features: all_features,
        projection,
        year: thf.year,
        departement,
        errors,
    })
}

/// Décode les bytes avec l'encodage détecté
fn decode_with_encoding(data: &[u8], encoding: &'static encoding_rs::Encoding) -> String {
    let (decoded, _, _) = encoding.decode(data);
    decoded.into_owned()
}
