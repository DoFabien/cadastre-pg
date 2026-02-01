//! Extraction des archives EDIGEO (.tar.bz2)

use bzip2::read::BzDecoder;
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use tar::Archive;

use crate::EdigeoError;

/// Contenu extrait d'une archive EDIGEO
#[derive(Debug)]
pub struct EdigeoArchive {
    /// Contenu du fichier THF (métadonnées)
    pub thf: Vec<u8>,

    /// Contenu du fichier GEO (projection)
    pub geo: Vec<u8>,

    /// Contenu du fichier QAL (qualité)
    pub qal: Vec<u8>,

    /// Contenu des fichiers VEC (données vectorielles)
    /// Il peut y en avoir plusieurs (un par feuille)
    pub vec: Vec<Vec<u8>>,
}

/// Extrait une archive EDIGEO en mémoire
///
/// # Arguments
///
/// * `path` - Chemin vers l'archive .tar.bz2
///
/// # Returns
///
/// Les contenus des fichiers THF, GEO, QAL et VEC
pub fn extract(path: &Path) -> Result<EdigeoArchive, EdigeoError> {
    let file = std::fs::File::open(path)?;
    let decoder = BzDecoder::new(file);
    let mut archive = Archive::new(decoder);

    let mut files: HashMap<String, Vec<u8>> = HashMap::new();

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.to_string_lossy().to_uppercase();

        // Extraire l'extension
        let extension = path.rsplit('.').next().unwrap_or("").to_uppercase();

        // Lire le contenu en mémoire
        let mut content = Vec::new();
        entry.read_to_end(&mut content)?;

        // Stocker selon le type
        match extension.as_str() {
            "THF" | "GEO" | "QAL" => {
                files.insert(extension, content);
            }
            "VEC" => {
                // Plusieurs fichiers VEC possibles, on les préfixe
                let key = format!(
                    "VEC_{}",
                    files.keys().filter(|k| k.starts_with("VEC")).count()
                );
                files.insert(key, content);
            }
            _ => {
                // Ignorer les autres fichiers (DIC, SCD, GEN, etc.)
            }
        }
    }

    // Vérifier la présence des fichiers obligatoires
    let thf = files
        .remove("THF")
        .ok_or_else(|| EdigeoError::MissingFile("THF".into()))?;

    let geo = files
        .remove("GEO")
        .ok_or_else(|| EdigeoError::MissingFile("GEO".into()))?;

    let qal = files.remove("QAL").unwrap_or_default(); // QAL peut être absent

    // Collecter tous les VEC
    let vec: Vec<Vec<u8>> = files
        .into_iter()
        .filter(|(k, _)| k.starts_with("VEC"))
        .map(|(_, v)| v)
        .collect();

    if vec.is_empty() {
        return Err(EdigeoError::MissingFile("VEC".into()));
    }

    Ok(EdigeoArchive { thf, geo, qal, vec })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_missing_file() {
        let result = extract(Path::new("nonexistent.tar.bz2"));
        assert!(result.is_err());
    }
}
