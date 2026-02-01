//! Transaction atomique pour import de millésime
//!
//! Garantit le rollback automatique en cas d'erreur fatale.

use anyhow::{Context, Result};
use deadpool_postgres::{Object, Pool, Transaction};
use tracing::{error, info};

/// Statut d'un import de millésime
#[derive(Debug, Clone, PartialEq)]
pub enum ImportStatus {
    /// Import réussi et commité
    Success,
    /// Import annulé (rollback)
    RolledBack,
}

/// Rapport d'import de millésime
#[derive(Debug)]
pub struct ImportReport {
    /// Date du millésime (format YYYY-MM-DD)
    pub millesime: String,
    /// Nombre d'entités importées
    pub entities_imported: usize,
    /// Liste des erreurs rencontrées
    pub errors: Vec<String>,
    /// Statut final de l'import
    pub status: ImportStatus,
}

/// Gestionnaire de transaction atomique pour un import de millésime
///
/// Encapsule une transaction PostgreSQL et garantit le rollback
/// en cas d'erreur fatale pendant l'import.
pub struct MillesimeImport<'a> {
    transaction: Transaction<'a>,
    millesime_date: String,
    entities_imported: usize,
    errors: Vec<String>,
}

impl<'a> MillesimeImport<'a> {
    /// Démarre une nouvelle transaction d'import
    ///
    /// # Arguments
    /// * `client` - Connexion PostgreSQL (doit rester vivante pendant la transaction)
    /// * `millesime_date` - Date du millésime (format YYYY-MM-DD)
    ///
    /// # Errors
    /// Retourne une erreur si la transaction ne peut pas être démarrée
    pub async fn begin(client: &'a mut Object, millesime_date: &str) -> Result<Self> {
        let transaction = client
            .transaction()
            .await
            .context("Failed to begin transaction")?;

        info!(
            millesime = %millesime_date,
            "Starting millésime import transaction"
        );

        Ok(Self {
            transaction,
            millesime_date: millesime_date.to_string(),
            entities_imported: 0,
            errors: Vec::new(),
        })
    }

    /// Accède à la transaction sous-jacente pour exécuter des requêtes
    pub fn transaction(&self) -> &Transaction<'a> {
        &self.transaction
    }

    /// Accède à la transaction de manière mutable
    pub fn transaction_mut(&mut self) -> &mut Transaction<'a> {
        &mut self.transaction
    }

    /// Retourne la date du millésime
    pub fn millesime_date(&self) -> &str {
        &self.millesime_date
    }

    /// Enregistre un compteur d'entités importées
    pub fn record_import(&mut self, count: usize) {
        self.entities_imported += count;
    }

    /// Retourne le nombre total d'entités importées
    pub fn entities_imported(&self) -> usize {
        self.entities_imported
    }

    /// Enregistre une erreur non-fatale
    pub fn record_error(&mut self, error: String) {
        self.errors.push(error);
    }

    /// Retourne le nombre d'erreurs non-fatales
    pub fn error_count(&self) -> usize {
        self.errors.len()
    }

    /// Valide et commit la transaction
    ///
    /// # Errors
    /// Retourne une erreur si le commit échoue
    pub async fn commit(self) -> Result<ImportReport> {
        self.transaction
            .commit()
            .await
            .context("Failed to commit transaction")?;

        info!(
            millesime = %self.millesime_date,
            entities = %self.entities_imported,
            errors = %self.errors.len(),
            "Millésime import committed successfully"
        );

        Ok(ImportReport {
            millesime: self.millesime_date,
            entities_imported: self.entities_imported,
            errors: self.errors,
            status: ImportStatus::Success,
        })
    }

    /// Annule la transaction (rollback)
    ///
    /// Appelé explicitement en cas d'erreur fatale.
    /// La transaction est également rollback automatiquement si droppée.
    pub async fn rollback(self, reason: &str) -> ImportReport {
        error!(
            millesime = %self.millesime_date,
            reason = %reason,
            entities_attempted = %self.entities_imported,
            "Rolling back millésime import"
        );

        // Le rollback est explicite pour clarté (sinon implicite au drop)
        if let Err(e) = self.transaction.rollback().await {
            error!(error = %e, "Explicit rollback failed (will rollback on drop anyway)");
        }

        ImportReport {
            millesime: self.millesime_date,
            entities_imported: 0, // Rollback = rien importé
            errors: vec![reason.to_string()],
            status: ImportStatus::RolledBack,
        }
    }
}

/// Exécute un import de millésime avec gestion automatique de transaction
///
/// Cette fonction helper acquiert une connexion du pool, démarre une transaction,
/// exécute la closure d'import, et commit ou rollback selon le résultat.
///
/// # Arguments
/// * `pool` - Pool de connexions PostgreSQL
/// * `millesime_date` - Date du millésime
/// * `import_fn` - Closure exécutant l'import
///
/// # Example
/// ```ignore
/// let report = with_millesime_transaction(pool, "2024-01-01", |import| async {
///     import.record_import(100);
///     Ok(())
/// }).await?;
/// ```
pub async fn with_millesime_transaction<F, Fut>(
    pool: &Pool,
    millesime_date: &str,
    import_fn: F,
) -> Result<ImportReport>
where
    F: FnOnce(&mut MillesimeImport<'_>) -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    let mut client = pool
        .get()
        .await
        .context("Failed to get connection from pool")?;

    let mut import = MillesimeImport::begin(&mut client, millesime_date).await?;

    match import_fn(&mut import).await {
        Ok(()) => import.commit().await,
        Err(e) => Ok(import.rollback(&e.to_string()).await),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_import_status_equality() {
        assert_eq!(ImportStatus::Success, ImportStatus::Success);
        assert_eq!(ImportStatus::RolledBack, ImportStatus::RolledBack);
        assert_ne!(ImportStatus::Success, ImportStatus::RolledBack);
    }

    #[test]
    fn test_import_report_debug() {
        let report = ImportReport {
            millesime: "2024-01-01".to_string(),
            entities_imported: 100,
            errors: vec!["error1".to_string()],
            status: ImportStatus::Success,
        };
        let debug_str = format!("{:?}", report);
        assert!(debug_str.contains("2024-01-01"));
        assert!(debug_str.contains("100"));
    }

    // Les tests d'intégration avec une vraie DB sont dans 3-8-tests-integration-postgresql
}
