//! Types d'erreurs pour le crate edigeo

use thiserror::Error;

/// Erreurs pouvant survenir lors du parsing EDIGEO
#[derive(Debug, Error)]
pub enum EdigeoError {
    /// Erreur d'I/O lors de la lecture de l'archive
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Archive corrompue ou format invalide
    #[error("Invalid archive format: {0}")]
    InvalidArchive(String),

    /// Fichier manquant dans l'archive
    #[error("Missing required file: {0}")]
    MissingFile(String),

    /// Erreur de parsing d'un fichier
    #[error("Parse error in {file}: {reason}")]
    ParseError { file: String, reason: String },

    /// Géométrie invalide
    #[error("Invalid geometry for {entity_id}: {reason}")]
    InvalidGeometry { entity_id: String, reason: String },

    /// Encodage non supporté
    #[error("Unsupported encoding: {0}")]
    UnsupportedEncoding(String),

    /// Projection non reconnue
    #[error("Unknown projection: {0}")]
    UnknownProjection(String),

    /// Erreur lors de la réparation de géométrie
    #[error("Geometry repair failed for {entity_id}: {reason}")]
    RepairFailed { entity_id: String, reason: String },
}

impl EdigeoError {
    /// Crée une erreur de parsing avec contexte
    pub fn parse_error(file: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::ParseError {
            file: file.into(),
            reason: reason.into(),
        }
    }

    /// Crée une erreur de géométrie invalide
    pub fn invalid_geometry(entity_id: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::InvalidGeometry {
            entity_id: entity_id.into(),
            reason: reason.into(),
        }
    }
}
