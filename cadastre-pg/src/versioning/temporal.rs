//! Gestion du versioning temporel des entités cadastrales
//!
//! Ce module gère le marquage des entités pour le versioning temporel:
//! - Marquage des entités existantes comme potentiellement terminées
//! - Réactivation des entités trouvées dans le nouveau millésime
//! - Conservation de l'historique des versions

use anyhow::{Context, Result};
use deadpool_postgres::Transaction;
use tracing::info;

/// Tables cadastrales avec versioning temporel
pub const CADASTRE_TABLES: &[&str] = &["parcelles", "sections", "communes", "batiments"];

/// Schéma par défaut pour les tables cadastrales
pub const DEFAULT_SCHEMA: &str = "cadastre";

/// Résultat du marquage pour une table
#[derive(Debug, Clone)]
pub struct TableMarkingResult {
    /// Nom de la table
    pub table: String,
    /// Nombre de lignes marquées
    pub rows_marked: usize,
}

/// Rapport de marquage pour toutes les tables
#[derive(Debug, Default)]
pub struct MarkingReport {
    /// Résultats par table
    pub tables: Vec<TableMarkingResult>,
}

impl MarkingReport {
    /// Retourne le nombre total d'entités marquées
    pub fn total_marked(&self) -> usize {
        self.tables.iter().map(|t| t.rows_marked).sum()
    }

    /// Vérifie si des entités ont été marquées
    pub fn has_marked_entities(&self) -> bool {
        self.total_marked() > 0
    }
}

/// Marque toutes les entités actives comme potentiellement terminées
///
/// Cette fonction est appelée au début d'un import de millésime.
/// Elle met `valid_to` à la date du millésime pour toutes les entités
/// qui ont actuellement `valid_to IS NULL`.
///
/// Après l'import, les entités qui n'ont pas été réactivées ou mises à jour
/// conserveront cette date `valid_to`, indiquant qu'elles ont disparu.
///
/// # Arguments
/// * `tx` - Transaction PostgreSQL en cours
/// * `schema` - Schéma de la base de données
/// * `millesime_date` - Date du millésime (format YYYY-MM-DD)
///
/// # Returns
/// Un rapport indiquant combien d'entités ont été marquées par table
pub async fn mark_all_as_ended(
    tx: &Transaction<'_>,
    schema: &str,
    millesime_date: &str,
) -> Result<MarkingReport> {
    let mut report = MarkingReport::default();

    for table in CADASTRE_TABLES {
        let result = mark_table_as_ended(tx, schema, table, millesime_date).await?;
        report.tables.push(result);
    }

    info!(
        total_marked = report.total_marked(),
        millesime = millesime_date,
        "Completed marking all entities as potentially ended"
    );

    Ok(report)
}

/// Marque les entités d'une table spécifique comme potentiellement terminées
pub async fn mark_table_as_ended(
    tx: &Transaction<'_>,
    schema: &str,
    table: &str,
    millesime_date: &str,
) -> Result<TableMarkingResult> {
    let query = format!(
        "UPDATE {}.{} SET valid_to = $1 WHERE valid_to IS NULL",
        schema, table
    );

    let rows_affected = tx
        .execute(&query, &[&millesime_date])
        .await
        .with_context(|| format!("Failed to mark {} as ended", table))?;

    info!(
        table = table,
        schema = schema,
        rows = rows_affected,
        millesime = millesime_date,
        "Marked entities as potentially ended"
    );

    Ok(TableMarkingResult {
        table: table.to_string(),
        rows_marked: rows_affected as usize,
    })
}

/// Réactive une entité existante (remet valid_to à NULL)
///
/// Appelée lorsqu'une entité existe toujours dans le nouveau millésime
/// sans modification de géométrie.
pub async fn reactivate_entity(
    tx: &Transaction<'_>,
    schema: &str,
    table: &str,
    id: &str,
) -> Result<bool> {
    let query = format!(
        "UPDATE {}.{} SET valid_to = NULL WHERE id = $1 AND valid_to IS NOT NULL",
        schema, table
    );

    let rows = tx.execute(&query, &[&id]).await?;

    Ok(rows > 0)
}

/// Compte les entités qui restent marquées comme terminées après import
///
/// Ces entités sont celles qui existaient avant mais n'apparaissent plus
/// dans le nouveau millésime (parcelles disparues, fusions, etc.)
pub async fn count_ended_entities(
    tx: &Transaction<'_>,
    schema: &str,
    millesime_date: &str,
) -> Result<EndedEntitiesReport> {
    let mut report = EndedEntitiesReport::default();

    for table in CADASTRE_TABLES {
        let query = format!(
            "SELECT COUNT(*) FROM {}.{} WHERE valid_to = $1",
            schema, table
        );

        let row = tx.query_one(&query, &[&millesime_date]).await?;
        let count: i64 = row.get(0);

        if count > 0 {
            report.tables.push(TableEndedCount {
                table: table.to_string(),
                count: count as usize,
            });
        }
    }

    Ok(report)
}

/// Rapport des entités terminées par table
#[derive(Debug, Default)]
pub struct EndedEntitiesReport {
    pub tables: Vec<TableEndedCount>,
}

impl EndedEntitiesReport {
    pub fn total_ended(&self) -> usize {
        self.tables.iter().map(|t| t.count).sum()
    }
}

/// Compte des entités terminées pour une table
#[derive(Debug)]
pub struct TableEndedCount {
    pub table: String,
    pub count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_marking_report_total() {
        let report = MarkingReport {
            tables: vec![
                TableMarkingResult {
                    table: "parcelles".to_string(),
                    rows_marked: 100,
                },
                TableMarkingResult {
                    table: "sections".to_string(),
                    rows_marked: 50,
                },
            ],
        };

        assert_eq!(report.total_marked(), 150);
        assert!(report.has_marked_entities());
    }

    #[test]
    fn test_empty_marking_report() {
        let report = MarkingReport::default();
        assert_eq!(report.total_marked(), 0);
        assert!(!report.has_marked_entities());
    }

    #[test]
    fn test_ended_entities_report() {
        let report = EndedEntitiesReport {
            tables: vec![TableEndedCount {
                table: "parcelles".to_string(),
                count: 10,
            }],
        };
        assert_eq!(report.total_ended(), 10);
    }
}
