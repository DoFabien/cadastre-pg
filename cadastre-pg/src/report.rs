//! Rapport d'import avec graceful degradation
//!
//! Ce module fournit des structures pour collecter et afficher
//! les résultats d'import avec erreurs et warnings détaillés.

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use serde::Serialize;

/// Statut global de l'import
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ImportStatus {
    /// Import réussi sans erreur
    Success,
    /// Import réussi avec des erreurs non-fatales
    PartialSuccess,
    /// Import annulé (rollback)
    RolledBack,
    /// Import échoué
    Failed,
}

/// Niveau de sévérité des erreurs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ErrorLevel {
    /// Erreur fatale: import abandonné
    Fatal,
    /// Erreur: entité skippée
    Error,
    /// Warning: entité importée avec dégradation
    Warning,
}

/// Erreur d'import avec contexte
#[derive(Debug, Clone, Serialize)]
pub struct ImportError {
    /// Niveau de sévérité
    pub level: ErrorLevel,
    /// Nom de l'archive source (optionnel)
    pub archive: Option<String>,
    /// Identifiant de l'entité (optionnel)
    pub entity_id: Option<String>,
    /// Type de l'entité (optionnel)
    pub entity_type: Option<String>,
    /// Message d'erreur
    pub message: String,
    /// Détails supplémentaires (optionnel)
    pub details: Option<String>,
}

/// Warning d'import
#[derive(Debug, Clone, Serialize)]
pub struct ImportWarning {
    /// Nom de l'archive source
    pub archive: String,
    /// Identifiant de l'entité
    pub entity_id: String,
    /// Message de warning
    pub message: String,
}

/// Statistiques par type d'entité
#[derive(Debug, Clone, Default, Serialize)]
pub struct TypeStats {
    /// Nombre d'entités insérées
    pub inserted: usize,
    /// Nombre d'entités mises à jour
    pub updated: usize,
    /// Nombre d'entités inchangées
    pub unchanged: usize,
    /// Nombre d'erreurs
    pub errors: usize,
}

impl TypeStats {
    pub fn total(&self) -> usize {
        self.inserted + self.updated + self.unchanged
    }
}

/// Rapport complet d'import
#[derive(Debug, Clone, Serialize)]
pub struct ImportReport {
    /// Date du millésime
    pub millesime: String,
    /// Durée de l'import
    pub duration_secs: f64,
    /// Statut global
    pub status: ImportStatus,

    // Compteurs globaux
    /// Nombre d'archives traitées
    pub archives_processed: usize,
    /// Nombre d'archives en erreur
    pub archives_failed: usize,
    /// Nombre d'entités insérées
    pub entities_imported: usize,
    /// Nombre d'entités mises à jour
    pub entities_updated: usize,
    /// Nombre d'entités inchangées
    pub entities_unchanged: usize,
    /// Nombre d'entités skippées
    pub entities_skipped: usize,

    /// Statistiques par type d'entité
    pub by_type: HashMap<String, TypeStats>,

    /// Liste des erreurs
    pub errors: Vec<ImportError>,
    /// Liste des warnings
    pub warnings: Vec<ImportWarning>,
}

impl Default for ImportReport {
    fn default() -> Self {
        Self {
            millesime: String::new(),
            duration_secs: 0.0,
            status: ImportStatus::Success,
            archives_processed: 0,
            archives_failed: 0,
            entities_imported: 0,
            entities_updated: 0,
            entities_unchanged: 0,
            entities_skipped: 0,
            by_type: HashMap::new(),
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }
}

impl ImportReport {
    /// Crée un nouveau rapport pour un millésime
    pub fn new(millesime: &str) -> Self {
        Self {
            millesime: millesime.to_string(),
            ..Default::default()
        }
    }

    /// Enregistre une entité insérée
    pub fn record_insert(&mut self, entity_type: &str) {
        self.entities_imported += 1;
        self.by_type
            .entry(entity_type.to_string())
            .or_default()
            .inserted += 1;
    }

    /// Enregistre une entité mise à jour
    pub fn record_update(&mut self, entity_type: &str) {
        self.entities_updated += 1;
        self.by_type
            .entry(entity_type.to_string())
            .or_default()
            .updated += 1;
    }

    /// Enregistre une entité inchangée
    pub fn record_unchanged(&mut self, entity_type: &str) {
        self.entities_unchanged += 1;
        self.by_type
            .entry(entity_type.to_string())
            .or_default()
            .unchanged += 1;
    }

    /// Enregistre une erreur
    pub fn record_error(&mut self, error: ImportError) {
        self.entities_skipped += 1;
        if let Some(ref entity_type) = error.entity_type {
            self.by_type.entry(entity_type.clone()).or_default().errors += 1;
        }
        self.errors.push(error);
    }

    /// Enregistre un warning
    pub fn record_warning(&mut self, warning: ImportWarning) {
        self.warnings.push(warning);
    }

    /// Enregistre une archive traitée avec succès
    pub fn record_archive_success(&mut self) {
        self.archives_processed += 1;
    }

    /// Enregistre une archive en échec
    pub fn record_archive_failure(&mut self, archive: &str, message: &str) {
        self.archives_processed += 1;
        self.archives_failed += 1;
        self.errors.push(ImportError {
            level: ErrorLevel::Error,
            archive: Some(archive.to_string()),
            entity_id: None,
            entity_type: None,
            message: message.to_string(),
            details: None,
        });
    }

    /// Définit la durée de l'import
    pub fn set_duration(&mut self, duration: Duration) {
        self.duration_secs = duration.as_secs_f64();
    }

    /// Détermine le statut final basé sur les erreurs
    pub fn finalize(&mut self) {
        let has_fatal = self.errors.iter().any(|e| e.level == ErrorLevel::Fatal);
        let has_errors = !self.errors.is_empty();
        let has_success =
            self.entities_imported > 0 || self.entities_updated > 0 || self.entities_unchanged > 0;

        self.status = if has_fatal {
            ImportStatus::Failed
        } else if has_errors && has_success {
            ImportStatus::PartialSuccess
        } else if has_errors {
            ImportStatus::Failed
        } else {
            ImportStatus::Success
        };
    }

    /// Nombre total d'entités traitées
    pub fn total_entities(&self) -> usize {
        self.entities_imported + self.entities_updated + self.entities_unchanged
    }

    /// Affiche le rapport sur la console
    pub fn display(&self) {
        println!("\n{}", "=".repeat(60));
        println!("IMPORT REPORT - Millésime {}", self.millesime);
        println!("{}", "=".repeat(60));

        println!("\nStatus: {:?}", self.status);
        println!("Duration: {:.2}s", self.duration_secs);

        println!("\n--- SUMMARY ---");
        println!(
            "Archives: {} processed, {} failed",
            self.archives_processed, self.archives_failed
        );
        println!(
            "Entities: {} imported, {} updated, {} unchanged, {} skipped",
            self.entities_imported,
            self.entities_updated,
            self.entities_unchanged,
            self.entities_skipped
        );

        if !self.by_type.is_empty() {
            println!("\n--- BY TYPE ---");
            let mut types: Vec<_> = self.by_type.iter().collect();
            types.sort_by_key(|(k, _)| k.as_str());
            for (type_name, stats) in types {
                println!(
                    "  {}: {} inserted, {} updated, {} unchanged, {} errors",
                    type_name, stats.inserted, stats.updated, stats.unchanged, stats.errors
                );
            }
        }

        if !self.warnings.is_empty() {
            println!("\n--- WARNINGS ({}) ---", self.warnings.len());
            for w in self.warnings.iter().take(10) {
                println!("  [{}] {}: {}", w.archive, w.entity_id, w.message);
            }
            if self.warnings.len() > 10 {
                println!("  ... and {} more", self.warnings.len() - 10);
            }
        }

        if !self.errors.is_empty() {
            println!("\n--- ERRORS ({}) ---", self.errors.len());
            for e in self.errors.iter().take(20) {
                let location = match (&e.archive, &e.entity_id) {
                    (Some(a), Some(id)) => format!("[{}:{}]", a, id),
                    (Some(a), None) => format!("[{}]", a),
                    (None, Some(id)) => format!("[{}]", id),
                    _ => String::new(),
                };
                println!("  {:?} {} {}", e.level, location, e.message);
            }
            if self.errors.len() > 20 {
                println!("  ... and {} more", self.errors.len() - 20);
            }
        }

        println!("\n{}", "=".repeat(60));
    }

    /// Sauvegarde le rapport en JSON
    pub fn save_to_file(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Affichage compact pour le résumé
    pub fn summary(&self) -> String {
        format!(
            "{}: {} imported, {} updated, {} unchanged, {} errors",
            self.millesime,
            self.entities_imported,
            self.entities_updated,
            self.entities_unchanged,
            self.errors.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_import_report_default() {
        let report = ImportReport::default();
        assert_eq!(report.status, ImportStatus::Success);
        assert_eq!(report.archives_processed, 0);
        assert_eq!(report.entities_imported, 0);
    }

    #[test]
    fn test_record_insert() {
        let mut report = ImportReport::new("2024-01");
        report.record_insert("PARCELLE");
        report.record_insert("PARCELLE");
        report.record_insert("SECTION");

        assert_eq!(report.entities_imported, 3);
        assert_eq!(report.by_type.get("PARCELLE").unwrap().inserted, 2);
        assert_eq!(report.by_type.get("SECTION").unwrap().inserted, 1);
    }

    #[test]
    fn test_record_error() {
        let mut report = ImportReport::new("2024-01");
        report.record_error(ImportError {
            level: ErrorLevel::Error,
            archive: Some("test.tar.bz2".to_string()),
            entity_id: Some("ID123".to_string()),
            entity_type: Some("PARCELLE".to_string()),
            message: "Invalid geometry".to_string(),
            details: None,
        });

        assert_eq!(report.entities_skipped, 1);
        assert_eq!(report.errors.len(), 1);
        assert_eq!(report.by_type.get("PARCELLE").unwrap().errors, 1);
    }

    #[test]
    fn test_finalize_success() {
        let mut report = ImportReport::new("2024-01");
        report.record_insert("PARCELLE");
        report.finalize();

        assert_eq!(report.status, ImportStatus::Success);
    }

    #[test]
    fn test_finalize_partial_success() {
        let mut report = ImportReport::new("2024-01");
        report.record_insert("PARCELLE");
        report.record_error(ImportError {
            level: ErrorLevel::Error,
            archive: None,
            entity_id: None,
            entity_type: None,
            message: "Error".to_string(),
            details: None,
        });
        report.finalize();

        assert_eq!(report.status, ImportStatus::PartialSuccess);
    }

    #[test]
    fn test_finalize_failed() {
        let mut report = ImportReport::new("2024-01");
        report.record_error(ImportError {
            level: ErrorLevel::Fatal,
            archive: None,
            entity_id: None,
            entity_type: None,
            message: "Fatal error".to_string(),
            details: None,
        });
        report.finalize();

        assert_eq!(report.status, ImportStatus::Failed);
    }

    #[test]
    fn test_summary() {
        let mut report = ImportReport::new("2024-01");
        report.entities_imported = 100;
        report.entities_updated = 50;
        report.entities_unchanged = 25;

        let summary = report.summary();
        assert!(summary.contains("2024-01"));
        assert!(summary.contains("100 imported"));
    }

    #[test]
    fn test_type_stats_total() {
        let stats = TypeStats {
            inserted: 10,
            updated: 5,
            unchanged: 3,
            errors: 2,
        };
        assert_eq!(stats.total(), 18);
    }
}
