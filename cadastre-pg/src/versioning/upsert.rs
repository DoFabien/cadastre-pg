//! Upsert d'entités avec détection de changement de géométrie
//!
//! Ce module gère l'insertion ou la mise à jour d'entités cadastrales
//! en détectant les changements via le hash de géométrie.

use std::collections::HashMap;

use anyhow::{Context, Result};
use deadpool_postgres::Transaction;
use geo::Geometry;
use tracing::{debug, trace};
use wkb::geom_to_wkb;

use super::temporal::DEFAULT_SCHEMA;

/// Entité à insérer ou mettre à jour
#[derive(Debug, Clone)]
pub struct EntityUpsert {
    /// Identifiant EDIGEO de l'entité
    pub id: String,
    /// Géométrie de l'entité
    pub geometry: Geometry,
    /// Hash de la géométrie pour comparaison
    pub geom_hash: [u8; 32],
    /// Propriétés supplémentaires
    pub properties: HashMap<String, String>,
    /// Type de feature EDIGEO
    pub feature_type: String,
}

/// Résultat d'un upsert
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpsertResult {
    /// Nouvelle entité insérée
    Inserted,
    /// Entité existante mise à jour (géométrie changée)
    Updated,
    /// Entité existante inchangée (réactivée)
    Unchanged,
}

/// Rapport d'upsert pour un batch
#[derive(Debug, Default)]
pub struct UpsertReport {
    /// Nombre d'entités insérées
    pub inserted: usize,
    /// Nombre d'entités mises à jour
    pub updated: usize,
    /// Nombre d'entités inchangées
    pub unchanged: usize,
    /// Nombre d'erreurs
    pub errors: usize,
}

impl UpsertReport {
    pub fn total_processed(&self) -> usize {
        self.inserted + self.updated + self.unchanged
    }

    pub fn record(&mut self, result: UpsertResult) {
        match result {
            UpsertResult::Inserted => self.inserted += 1,
            UpsertResult::Updated => self.updated += 1,
            UpsertResult::Unchanged => self.unchanged += 1,
        }
    }

    pub fn record_error(&mut self) {
        self.errors += 1;
    }
}

/// Insère ou met à jour une entité avec détection de changement
///
/// # Arguments
/// * `tx` - Transaction PostgreSQL
/// * `entity` - Entité à upserter
/// * `millesime_date` - Date du millésime (format YYYY-MM-DD)
/// * `srid` - SRID de la géométrie
///
/// # Returns
/// Le résultat de l'opération (Inserted, Updated, ou Unchanged)
pub async fn upsert_entity(
    tx: &Transaction<'_>,
    entity: &EntityUpsert,
    millesime_date: &str,
    srid: u32,
) -> Result<UpsertResult> {
    let table = feature_type_to_table(&entity.feature_type);
    let schema = DEFAULT_SCHEMA;

    // 1. Chercher si l'entité existe (dernière version)
    let query = format!(
        "SELECT row_id, geometry_hash, valid_to FROM {}.{}
         WHERE id = $1
         ORDER BY valid_from DESC LIMIT 1",
        schema, table
    );

    let existing = tx
        .query_opt(&query, &[&entity.id])
        .await
        .context("Failed to check existing entity")?;

    match existing {
        Some(row) => {
            let existing_hash: Vec<u8> = row.get("geometry_hash");
            let valid_to: Option<String> = row.get("valid_to");
            let row_id: i64 = row.get("row_id");

            // Comparer les hashes
            if hash_matches(&existing_hash, &entity.geom_hash) {
                // Même géométrie, juste réactiver si nécessaire
                if valid_to.is_some() {
                    reactivate_entity(tx, schema, table, row_id).await?;
                    debug!(id = %entity.id, table = table, "Entity reactivated (unchanged)");
                } else {
                    trace!(id = %entity.id, table = table, "Entity already active (unchanged)");
                }
                Ok(UpsertResult::Unchanged)
            } else {
                // Géométrie différente, mettre à jour
                update_entity_geometry(tx, schema, table, row_id, entity, srid).await?;
                debug!(id = %entity.id, table = table, "Entity geometry updated");
                Ok(UpsertResult::Updated)
            }
        }
        None => {
            // Nouvelle entité
            insert_entity(tx, schema, table, entity, millesime_date, srid).await?;
            debug!(id = %entity.id, table = table, "New entity inserted");
            Ok(UpsertResult::Inserted)
        }
    }
}

/// Compare un hash stocké en DB avec un hash calculé
fn hash_matches(stored: &[u8], computed: &[u8; 32]) -> bool {
    stored == computed.as_slice()
}

/// Réactive une entité (remet valid_to à NULL)
async fn reactivate_entity(
    tx: &Transaction<'_>,
    schema: &str,
    table: &str,
    row_id: i64,
) -> Result<()> {
    let query = format!(
        "UPDATE {}.{} SET valid_to = NULL WHERE row_id = $1",
        schema, table
    );

    tx.execute(&query, &[&row_id])
        .await
        .context("Failed to reactivate entity")?;

    Ok(())
}

/// Met à jour la géométrie d'une entité existante
async fn update_entity_geometry(
    tx: &Transaction<'_>,
    schema: &str,
    table: &str,
    row_id: i64,
    entity: &EntityUpsert,
    srid: u32,
) -> Result<()> {
    let ewkb = geometry_to_ewkb(&entity.geometry, srid)?;

    let query = format!(
        "UPDATE {}.{} SET geometry = $1, geometry_hash = $2, valid_to = NULL WHERE row_id = $3",
        schema, table
    );

    tx.execute(&query, &[&ewkb, &entity.geom_hash.as_slice(), &row_id])
        .await
        .context("Failed to update entity geometry")?;

    Ok(())
}

/// Insère une nouvelle entité
async fn insert_entity(
    tx: &Transaction<'_>,
    schema: &str,
    table: &str,
    entity: &EntityUpsert,
    millesime_date: &str,
    srid: u32,
) -> Result<()> {
    let ewkb = geometry_to_ewkb(&entity.geometry, srid)?;

    let query = format!(
        "INSERT INTO {}.{} (id, geometry, valid_from, geometry_hash) VALUES ($1, $2, $3, $4)",
        schema, table
    );

    tx.execute(
        &query,
        &[
            &entity.id,
            &ewkb,
            &millesime_date,
            &entity.geom_hash.as_slice(),
        ],
    )
    .await
    .context("Failed to insert entity")?;

    Ok(())
}

/// Convertit une géométrie geo en EWKB PostGIS
fn geometry_to_ewkb(geom: &Geometry, srid: u32) -> Result<Vec<u8>> {
    let wkb = geom_to_wkb(geom)
        .map_err(|e| anyhow::anyhow!("Failed to convert geometry to WKB: {:?}", e))?;

    // Ajouter le SRID au WKB (format EWKB)
    let mut ewkb = Vec::with_capacity(wkb.len() + 4);

    if wkb.len() >= 5 {
        ewkb.push(wkb[0]); // Byte order

        // Type avec flag SRID (0x20000000)
        let type_bytes = [wkb[1], wkb[2], wkb[3], wkb[4]];
        let geom_type = if wkb[0] == 1 {
            // Little endian
            u32::from_le_bytes(type_bytes) | 0x20000000
        } else {
            // Big endian
            u32::from_be_bytes(type_bytes) | 0x20000000
        };

        if wkb[0] == 1 {
            ewkb.extend_from_slice(&geom_type.to_le_bytes());
            ewkb.extend_from_slice(&srid.to_le_bytes());
        } else {
            ewkb.extend_from_slice(&geom_type.to_be_bytes());
            ewkb.extend_from_slice(&srid.to_be_bytes());
        }

        ewkb.extend_from_slice(&wkb[5..]);
    }

    Ok(ewkb)
}

/// Convertit un type de feature EDIGEO vers le nom de table
pub fn feature_type_to_table(feature_type: &str) -> &'static str {
    let upper = feature_type.to_uppercase();
    match upper.as_str() {
        s if s.contains("PARCELLE") => "parcelles",
        s if s.contains("SECTION") => "sections",
        s if s.contains("COMMUNE") => "communes",
        s if s.contains("BATIMENT") || s.contains("BATI") => "batiments",
        s if s.contains("LIEU") || s.contains("LIEUDIT") => "lieux_dits",
        s if s.contains("SUBDFISC") => "subdivisions_fiscales",
        _ => "parcelles", // Default
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo::Point;

    #[test]
    fn test_upsert_result_equality() {
        assert_eq!(UpsertResult::Inserted, UpsertResult::Inserted);
        assert_ne!(UpsertResult::Inserted, UpsertResult::Updated);
    }

    #[test]
    fn test_upsert_report() {
        let mut report = UpsertReport::default();
        report.record(UpsertResult::Inserted);
        report.record(UpsertResult::Inserted);
        report.record(UpsertResult::Updated);
        report.record(UpsertResult::Unchanged);
        report.record_error();

        assert_eq!(report.inserted, 2);
        assert_eq!(report.updated, 1);
        assert_eq!(report.unchanged, 1);
        assert_eq!(report.errors, 1);
        assert_eq!(report.total_processed(), 4);
    }

    #[test]
    fn test_feature_type_to_table() {
        assert_eq!(feature_type_to_table("PARCELLE"), "parcelles");
        assert_eq!(feature_type_to_table("PARCELLE_1"), "parcelles");
        assert_eq!(feature_type_to_table("SECTION_A"), "sections");
        assert_eq!(feature_type_to_table("COMMUNE"), "communes");
        assert_eq!(feature_type_to_table("BATIMENT"), "batiments");
        assert_eq!(feature_type_to_table("BATI_DUR"), "batiments");
        assert_eq!(feature_type_to_table("LIEUDIT"), "lieux_dits");
        assert_eq!(feature_type_to_table("UNKNOWN"), "parcelles");
    }

    #[test]
    fn test_hash_matches() {
        let hash: [u8; 32] = [1u8; 32];
        let stored = hash.to_vec();
        assert!(hash_matches(&stored, &hash));

        let different: [u8; 32] = [2u8; 32];
        assert!(!hash_matches(&stored, &different));
    }

    #[test]
    fn test_geometry_to_ewkb() {
        let point = Geometry::Point(Point::new(1.0, 2.0));
        let ewkb = geometry_to_ewkb(&point, 4326).unwrap();

        // EWKB should have SRID flag and SRID value
        assert!(ewkb.len() > 5);
        // Little-endian, SRID flag should be set
        if ewkb[0] == 1 {
            let type_word = u32::from_le_bytes([ewkb[1], ewkb[2], ewkb[3], ewkb[4]]);
            assert!(type_word & 0x20000000 != 0, "SRID flag should be set");
        }
    }
}
