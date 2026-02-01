//! Export vers PostgreSQL/PostGIS

use std::collections::HashMap;

use anyhow::{Context, Result};
use deadpool_postgres::Pool;
use futures::SinkExt;
use geo::Geometry;
use geozero::wkt::WktWriter;
use geozero::GeozeroGeometry;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};
use wkb::geom_to_wkb;

use edigeo::{Feature, Projection};

/// Configuration d'une table d'export
#[derive(Debug, Clone)]
pub struct TableConfig {
    pub name: String,
    pub geometry_type: String,
    pub srid: u32,
    pub columns: Vec<ColumnConfig>,
}

/// Configuration d'une colonne
#[derive(Debug, Clone)]
pub struct ColumnConfig {
    pub name: String,
    pub pg_type: String,
    pub source: String,
}

/// Chunk CSV pré-formaté pour COPY
#[derive(Debug)]
pub struct CopyChunk {
    pub data: bytes::Bytes,
    pub rows: u64,
}

/// Crée le schéma et les tables
pub async fn create_schema(
    pool: &Pool,
    schema: &str,
    tables: &[TableConfig],
    drop_existing: bool,
) -> Result<()> {
    let client = pool.get().await?;

    // Créer le schéma
    if drop_existing {
        client
            .execute(&format!("DROP SCHEMA IF EXISTS {} CASCADE", schema), &[])
            .await
            .context("Failed to drop schema")?;
    }

    client
        .execute(&format!("CREATE SCHEMA IF NOT EXISTS {}", schema), &[])
        .await
        .context("Failed to create schema")?;

    // Activer PostGIS si nécessaire (peut nécessiter des droits superuser).
    // Si l'extension existe déjà mais que l'utilisateur ne peut pas la (re)créer,
    // on dégrade gracieusement.
    match client
        .execute("CREATE EXTENSION IF NOT EXISTS postgis", &[])
        .await
    {
        Ok(_) => {}
        Err(e) => {
            warn!("CREATE EXTENSION postgis failed (will check if already installed): {e}");
            let exists = client
                .query_opt("SELECT 1 FROM pg_extension WHERE extname = 'postgis'", &[])
                .await
                .context("Failed to check pg_extension")?
                .is_some();
            if !exists {
                return Err(anyhow::anyhow!(
                    "PostGIS extension is not installed and could not be created: {e}"
                ));
            }
        }
    }

    // Créer les tables
    for table in tables {
        create_table(&client, schema, table).await?;
    }

    // Créer la table des checksums d'archives
    create_archive_checksums_table(&client, schema).await?;

    Ok(())
}

/// Crée la table de suivi des checksums d'archives pour le skip incrémental.
async fn create_archive_checksums_table(
    client: &deadpool_postgres::Object,
    schema: &str,
) -> Result<()> {
    let sql = format!(
        r#"
        CREATE TABLE IF NOT EXISTS {}._archive_checksums (
            archive_name TEXT PRIMARY KEY,
            checksum TEXT NOT NULL,
            imported_at TIMESTAMPTZ DEFAULT NOW()
        )
        "#,
        schema
    );

    client
        .execute(&sql, &[])
        .await
        .context("Failed to create _archive_checksums table")?;

    Ok(())
}

/// Vérifie si une archive a déjà été importée (checksum identique).
pub async fn is_archive_already_imported(
    pool: &Pool,
    schema: &str,
    archive_name: &str,
    checksum: &str,
) -> Result<bool> {
    let client = pool.get().await?;

    let row = client
        .query_opt(
            &format!(
                "SELECT 1 FROM {}._archive_checksums WHERE archive_name = $1 AND checksum = $2",
                schema
            ),
            &[&archive_name, &checksum],
        )
        .await?;

    Ok(row.is_some())
}

/// Enregistre le checksum d'une archive après import réussi.
pub async fn record_archive_checksum(
    pool: &Pool,
    schema: &str,
    archive_name: &str,
    checksum: &str,
) -> Result<()> {
    let client = pool.get().await?;

    client
        .execute(
            &format!(
                r#"
                INSERT INTO {}._archive_checksums (archive_name, checksum)
                VALUES ($1, $2)
                ON CONFLICT (archive_name) DO UPDATE SET checksum = $2, imported_at = NOW()
                "#,
                schema
            ),
            &[&archive_name, &checksum],
        )
        .await
        .context("Failed to record archive checksum")?;

    Ok(())
}

/// Crée les tables de staging (sans contraintes) utilisées pour COPY.
///
/// Les tables sont créées dans le même schéma, avec un préfixe `_staging_`.
pub async fn create_staging_tables(
    pool: &Pool,
    schema: &str,
    tables: &[TableConfig],
) -> Result<()> {
    let client = pool.get().await?;

    for table in tables {
        let staging = staging_table_name(&table.name);
        let sql = format!(
            r#"
            CREATE UNLOGGED TABLE IF NOT EXISTS {}.{} (LIKE {}.{} INCLUDING DEFAULTS)
            "#,
            schema, staging, schema, table.name
        );
        client
            .execute(&sql, &[])
            .await
            .with_context(|| format!("Failed to create staging table {}.{}", schema, staging))?;

        // S'assurer que la table staging est vide (si réutilisée)
        client
            .execute(&format!("TRUNCATE TABLE {}.{}", schema, staging), &[])
            .await
            .with_context(|| format!("Failed to truncate staging table {}.{}", schema, staging))?;
    }

    Ok(())
}

/// Fusionne la staging vers la table finale en ignorant les doublons (DO NOTHING).
/// Applique ST_MakeValid pour corriger les géométries invalides (auto-intersections, etc.)
pub async fn merge_staging_into_table(
    pool: &Pool,
    schema: &str,
    table: &str,
    dynamic_columns: &[String],
) -> Result<u64> {
    let client = pool.get().await?;
    let staging = staging_table_name(table);

    // Colonnes cibles (on exclut row_id qui est généré)
    let target_cols = vec![
        "id",
        "departement",
        "geometry",
    ];
    let mut all_target_cols: Vec<&str> = target_cols.clone();
    let dynamic_refs: Vec<&str> = dynamic_columns.iter().map(|s| s.as_str()).collect();
    all_target_cols.extend(dynamic_refs.iter());
    all_target_cols.extend(vec!["valid_from", "valid_to", "geometry_hash"]);

    // Colonnes sources (avec ST_MakeValid sur geometry)
    let source_cols: Vec<String> = all_target_cols
        .iter()
        .map(|&col| {
            if col == "geometry" {
                "ST_MakeValid(geometry)".to_string()
            } else {
                col.to_string()
            }
        })
        .collect();

    let target_sql = all_target_cols.join(", ");
    let source_sql = source_cols.join(", ");

    let sql = format!(
        r#"
        INSERT INTO {schema}.{table} ({target_sql})
        SELECT {source_sql} FROM {schema}.{staging}
        ON CONFLICT (departement, id, valid_from) DO NOTHING
        "#,
        schema = schema,
        table = table,
        target_sql = target_sql,
        source_sql = source_sql,
        staging = staging
    );

    let inserted = client
        .execute(&sql, &[])
        .await
        .with_context(|| format!("Failed to merge staging into {}.{}", schema, table))?;

    Ok(inserted)
}

/// Supprime les tables de staging.
pub async fn drop_staging_tables(pool: &Pool, schema: &str, tables: &[TableConfig]) -> Result<()> {
    let client = pool.get().await?;
    for table in tables {
        let staging = staging_table_name(&table.name);
        client
            .execute(
                &format!("DROP TABLE IF EXISTS {}.{} CASCADE", schema, staging),
                &[],
            )
            .await
            .with_context(|| format!("Failed to drop staging table {}.{}", schema, staging))?;
    }
    Ok(())
}

/// Supprime les tables finales existantes dans le schema.
pub async fn drop_tables(pool: &Pool, schema: &str, tables: &[TableConfig]) -> Result<()> {
    let client = pool.get().await?;

    let schema_exists = client
        .query_opt("SELECT 1 FROM pg_namespace WHERE nspname = $1", &[&schema])
        .await?
        .is_some();

    if !schema_exists {
        return Ok(());
    }

    for table in tables {
        client
            .execute(
                &format!("DROP TABLE IF EXISTS {}.{} CASCADE", schema, table.name),
                &[],
            )
            .await
            .with_context(|| format!("Failed to drop table {}.{}", schema, table.name))?;
    }

    Ok(())
}

/// Crée les index (hors contraintes) pour une table, après import.
pub async fn create_indexes(pool: &Pool, schema: &str, table: &str) -> Result<()> {
    let client = pool.get().await?;

    // Index sur (departement, id) pour lookup rapide
    client
        .execute(
            &format!(
                "CREATE INDEX IF NOT EXISTS idx_{}_{}_dep_id ON {}.{} (departement, id)",
                schema, table, schema, table
            ),
            &[],
        )
        .await
        .with_context(|| format!("Failed to create dep/id index on {}.{}", schema, table))?;

    // Index spatial
    client
        .execute(
            &format!(
                "CREATE INDEX IF NOT EXISTS idx_{}_{}_geom ON {}.{} USING GIST (geometry)",
                schema, table, schema, table
            ),
            &[],
        )
        .await
        .with_context(|| format!("Failed to create geometry index on {}.{}", schema, table))?;

    // Index temporel
    client
        .execute(
            &format!(
                "CREATE INDEX IF NOT EXISTS idx_{}_{}_valid ON {}.{} (valid_from, valid_to)",
                schema, table, schema, table
            ),
            &[],
        )
        .await
        .with_context(|| format!("Failed to create temporal index on {}.{}", schema, table))?;

    Ok(())
}

/// Charge les geometry_hash existants d'une table pour filtrage incrémental.
/// Retourne un HashSet des hash (32 bytes) pour lookup O(1).
pub async fn load_existing_hashes(
    pool: &Pool,
    schema: &str,
    table: &str,
) -> Result<std::collections::HashSet<[u8; 32]>> {
    let client = pool.get().await?;

    // Vérifier si la table existe
    let table_exists = client
        .query_opt(
            "SELECT 1 FROM information_schema.tables WHERE table_schema = $1 AND table_name = $2",
            &[&schema, &table],
        )
        .await?
        .is_some();

    if !table_exists {
        return Ok(std::collections::HashSet::new());
    }

    let rows = client
        .query(
            &format!(
                "SELECT geometry_hash FROM {}.{} WHERE geometry_hash IS NOT NULL",
                schema, table
            ),
            &[],
        )
        .await
        .with_context(|| format!("Failed to load hashes from {}.{}", schema, table))?;

    let mut hashes = std::collections::HashSet::with_capacity(rows.len());
    for row in rows {
        let hash_bytes: Vec<u8> = row.get(0);
        if hash_bytes.len() == 32 {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&hash_bytes);
            hashes.insert(arr);
        }
    }

    info!(
        "Loaded {} existing hashes from {}.{}",
        hashes.len(),
        schema,
        table
    );
    Ok(hashes)
}

/// Crée une table avec versioning temporel
async fn create_table(
    client: &deadpool_postgres::Object,
    schema: &str,
    config: &TableConfig,
) -> Result<()> {
    let columns: Vec<String> = config
        .columns
        .iter()
        .map(|c| format!("{} {}", c.name, c.pg_type))
        .collect();
    let dynamic_columns_sql = if columns.is_empty() {
        String::new()
    } else {
        format!("{},", columns.join(",\n            "))
    };

    let sql = format!(
        r#"
        CREATE TABLE IF NOT EXISTS {}.{} (
            row_id BIGSERIAL PRIMARY KEY,
            id TEXT NOT NULL,
            departement VARCHAR(3) NOT NULL,
            geometry geometry({}, {}),
            {}
            valid_from DATE NOT NULL,
            valid_to DATE,
            geometry_hash BYTEA,
            created_at TIMESTAMPTZ DEFAULT NOW(),
            updated_at TIMESTAMPTZ DEFAULT NOW(),
            CONSTRAINT {}_valid_dates CHECK (valid_to IS NULL OR valid_to > valid_from),
            CONSTRAINT {}_dep_id_valid_unique UNIQUE (departement, id, valid_from)
        )
        "#,
        schema,
        config.name,
        config.geometry_type,
        config.srid,
        dynamic_columns_sql,
        config.name,
        config.name
    );

    client
        .execute(&sql, &[])
        .await
        .with_context(|| format!("Failed to create table {}.{}", schema, config.name))?;

    info!("Created table {}.{}", schema, config.name);
    Ok(())
}

fn staging_table_name(table: &str) -> String {
    format!("_staging_{}", table)
}

/// Insère des features dans une table avec versioning
pub async fn insert_features(
    pool: &Pool,
    schema: &str,
    table: &str,
    features: &[Feature],
    projection: &Projection,
    valid_from: &str,
    departement: &str,
    column_mapping: &HashMap<String, String>,
) -> Result<usize> {
    if features.is_empty() {
        return Ok(0);
    }

    let client = pool.get().await?;

    // Préparer les colonnes
    let columns: Vec<&str> = column_mapping.keys().map(|s| s.as_str()).collect();
    let column_list = columns.join(", ");
    let placeholders: Vec<String> = (1..=columns.len() + 5).map(|i| format!("${}", i)).collect();

    let sql = format!(
        "INSERT INTO {}.{} (id, departement, geometry, valid_from, geometry_hash, {}) VALUES ({})",
        schema,
        table,
        column_list,
        placeholders.join(", ")
    );

    let stmt = client.prepare(&sql).await?;

    let mut inserted = 0;

    for feature in features {
        // Convertir la géométrie en WKB
        let wkb = match geometry_to_wkb(&feature.geometry, projection.epsg) {
            Ok(w) => w,
            Err(e) => {
                warn!("Failed to convert geometry for {}: {}", feature.id, e);
                continue;
            }
        };

        // Calculer le hash de la géométrie
        let hash = crate::versioning::diff::geometry_hash(&feature.geometry);

        // Préparer les valeurs des colonnes
        let mut values: Vec<Box<dyn tokio_postgres::types::ToSql + Sync>> = vec![
            Box::new(feature.id.clone()),
            Box::new(departement.to_string()),
            Box::new(wkb),
            Box::new(valid_from.to_string()),
            Box::new(hash.to_vec()),
        ];

        for (_col, source) in column_mapping {
            let value = feature.properties.get(source).cloned().unwrap_or_default();
            values.push(Box::new(value));
        }

        // Conversion pour tokio_postgres
        let refs: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> =
            values.iter().map(|v| v.as_ref()).collect();

        match client.execute(&stmt, &refs).await {
            Ok(_) => inserted += 1,
            Err(e) => {
                debug!("Failed to insert {}: {}", feature.id, e);
            }
        }
    }

    Ok(inserted)
}

/// Convertit une géométrie geo en WKB PostGIS
fn geometry_to_wkb(geom: &Geometry, srid: u32) -> Result<Vec<u8>> {
    let wkb = geom_to_wkb(geom)
        .map_err(|e| anyhow::anyhow!("Failed to convert geometry to WKB: {:?}", e))?;

    // Ajouter le SRID au WKB (format EWKB)
    let mut ewkb = Vec::with_capacity(wkb.len() + 4);

    // Modifier le byte order et ajouter le flag SRID
    if !wkb.is_empty() {
        ewkb.push(wkb[0]); // Byte order

        // Type avec flag SRID (0x20000000)
        if wkb.len() >= 5 {
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
    }

    Ok(ewkb)
}

/// Met à jour les features existantes avec versioning
pub async fn update_features_with_versioning(
    pool: &Pool,
    schema: &str,
    table: &str,
    features: &[Feature],
    projection: &Projection,
    valid_from: &str,
    departement: &str,
    column_mapping: &HashMap<String, String>,
) -> Result<(usize, usize, usize)> {
    let client = pool.get().await?;

    let mut inserted = 0;
    let mut updated = 0;
    let mut unchanged = 0;

    for feature in features {
        let hash = crate::versioning::diff::geometry_hash(&feature.geometry);

        // Vérifier si une version active existe
        let existing = client
            .query_opt(
                &format!(
                    "SELECT row_id, geometry_hash FROM {}.{} WHERE id = $1 AND valid_to IS NULL",
                    schema, table
                ),
                &[&feature.id],
            )
            .await?;

        match existing {
            Some(row) => {
                let existing_hash: Vec<u8> = row.get("geometry_hash");
                if existing_hash == hash.to_vec() {
                    // Géométrie identique, pas de changement
                    unchanged += 1;
                } else {
                    // Fermer la version précédente
                    let existing_id: i64 = row.get("row_id");
                    client
                        .execute(
                            &format!(
                                "UPDATE {}.{} SET valid_to = $1 WHERE row_id = $2",
                                schema, table
                            ),
                            &[&valid_from, &existing_id],
                        )
                        .await?;

                    // Insérer la nouvelle version
                    let wkb = geometry_to_wkb(&feature.geometry, projection.epsg)?;
                    let columns: Vec<&str> = column_mapping.keys().map(|s| s.as_str()).collect();

                    let mut values: Vec<Box<dyn tokio_postgres::types::ToSql + Sync>> = vec![
                        Box::new(feature.id.clone()),
                        Box::new(departement.to_string()),
                        Box::new(wkb),
                        Box::new(valid_from.to_string()),
                        Box::new(hash.to_vec()),
                    ];

                    for (_col, source) in column_mapping {
                        let value = feature.properties.get(source).cloned().unwrap_or_default();
                        values.push(Box::new(value));
                    }

                    let placeholders: Vec<String> =
                        (1..=values.len()).map(|i| format!("${}", i)).collect();

                    let sql = format!(
                        "INSERT INTO {}.{} (id, departement, geometry, valid_from, geometry_hash, {}) VALUES ({})",
                        schema,
                        table,
                        columns.join(", "),
                        placeholders.join(", ")
                    );

                    let refs: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> =
                        values.iter().map(|v| v.as_ref()).collect();

                    client.execute(&sql, &refs).await?;
                    updated += 1;
                }
            }
            None => {
                // Nouvelle feature
                let wkb = geometry_to_wkb(&feature.geometry, projection.epsg)?;
                let columns: Vec<&str> = column_mapping.keys().map(|s| s.as_str()).collect();

                let mut values: Vec<Box<dyn tokio_postgres::types::ToSql + Sync>> = vec![
                    Box::new(feature.id.clone()),
                    Box::new(departement.to_string()),
                    Box::new(wkb),
                    Box::new(valid_from.to_string()),
                    Box::new(hash.to_vec()),
                ];

                for (_col, source) in column_mapping {
                    let value = feature.properties.get(source).cloned().unwrap_or_default();
                    values.push(Box::new(value));
                }

                let placeholders: Vec<String> =
                    (1..=values.len()).map(|i| format!("${}", i)).collect();

                let sql = format!(
                    "INSERT INTO {}.{} (id, departement, geometry, valid_from, geometry_hash, {}) VALUES ({})",
                    schema,
                    table,
                    columns.join(", "),
                    placeholders.join(", ")
                );

                let refs: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> =
                    values.iter().map(|v| v.as_ref()).collect();

                client.execute(&sql, &refs).await?;
                inserted += 1;
            }
        }
    }

    Ok((inserted, updated, unchanged))
}

/// Insère des features en masse avec COPY (beaucoup plus rapide que INSERT)
///
/// Utilise le format CSV avec conversion WKB via geozero pour un import optimisé.
/// Le département est ajouté automatiquement à chaque feature.
pub async fn copy_features(
    pool: &Pool,
    schema: &str,
    table: &str,
    features: &[Feature],
    projection: &Projection,
    valid_from: &str,
    departement: &str,
    column_mapping: &HashMap<String, String>,
) -> Result<usize> {
    if features.is_empty() {
        return Ok(0);
    }

    let mut client = pool.get().await?;
    let tx = client.transaction().await?;

    // Préparer la commande COPY
    let columns: Vec<&str> = column_mapping.keys().map(|s| s.as_str()).collect();
    let copy_sql = format!(
        "COPY {}.{} (id, departement, geometry, valid_from, geometry_hash, {}) FROM STDIN WITH (FORMAT csv, DELIMITER '|', QUOTE E'\\x01', NULL '')",
        schema,
        table,
        columns.join(", ")
    );

    let copy_in = tx.copy_in(&copy_sql).await?;
    let mut pinned = std::pin::pin!(copy_in);

    let mut inserted = 0;

    for feature in features {
        // Convertir la géométrie en WKT (Well-Known Text) avec EWKT (SRID inclus)
        let mut wkt_buf = Vec::new();
        {
            let mut writer = WktWriter::new(&mut wkt_buf);
            if let Err(e) = feature.geometry.process_geom(&mut writer) {
                warn!("Failed to convert geometry for {}: {}", feature.id, e);
                continue;
            }
        }
        let wkt = String::from_utf8_lossy(&wkt_buf);
        // Format EWKT: SRID=epsg;WKT
        let ewkt = format!("SRID={};{}", projection.epsg, wkt);

        // Calculer le hash de la géométrie
        let hash = crate::versioning::diff::geometry_hash(&feature.geometry);
        let hash_hex = hex::encode(&hash);

        // Construire la ligne CSV
        // Format: id|departement|geometry|valid_from|geometry_hash|attr1|attr2|...
        let mut row = format!(
            "{}|{}|{}|{}|\\x{}",
            escape_csv(&feature.id),
            escape_csv(departement),
            escape_csv(&ewkt),
            valid_from,
            hash_hex
        );

        // Ajouter les colonnes dynamiques
        for col in &columns {
            let source = column_mapping.get(*col).unwrap();
            let value = feature.properties.get(source).cloned().unwrap_or_default();
            row.push('|');
            row.push_str(&escape_csv(&value));
        }
        row.push('\n');

        // Écrire dans le flux COPY
        if let Err(e) = pinned.as_mut().send(bytes::Bytes::from(row)).await {
            warn!("Failed to write row for {}: {}", feature.id, e);
            continue;
        }
        inserted += 1;
    }

    // Terminer le COPY
    pinned.close().await?;
    tx.commit().await?;

    info!("Inserted {} features into {}.{}", inserted, schema, table);
    Ok(inserted)
}

/// Insère des lignes CSV pré-formatées via COPY (pipeline streaming).
///
/// Chaque chunk doit contenir des lignes terminées par `\n` et correspondre
/// exactement au layout de colonnes utilisé par la commande COPY.
pub async fn copy_csv_chunks(
    pool: &Pool,
    schema: &str,
    table: &str,
    dynamic_columns: &[String],
    mut rx: mpsc::Receiver<CopyChunk>,
) -> Result<u64> {
    let mut client = pool.get().await?;
    let tx = client.transaction().await?;

    let copy_sql = if dynamic_columns.is_empty() {
        format!(
            "COPY {}.{} (id, departement, geometry, valid_from, geometry_hash) FROM STDIN WITH (FORMAT csv, DELIMITER '|', QUOTE '\"', ESCAPE '\"', NULL '')",
            schema, table
        )
    } else {
        format!(
            "COPY {}.{} (id, departement, geometry, valid_from, geometry_hash, {}) FROM STDIN WITH (FORMAT csv, DELIMITER '|', QUOTE '\"', ESCAPE '\"', NULL '')",
            schema,
            table,
            dynamic_columns.join(", ")
        )
    };

    let copy_in = tx.copy_in(&copy_sql).await?;
    let mut pinned = std::pin::pin!(copy_in);

    let mut total_rows: u64 = 0;

    while let Some(chunk) = rx.recv().await {
        if chunk.data.is_empty() {
            continue;
        }
        pinned
            .as_mut()
            .send(chunk.data)
            .await
            .context("Failed to send COPY chunk")?;
        total_rows += chunk.rows;
    }

    pinned.close().await?;
    tx.commit().await?;

    Ok(total_rows)
}

/// Échappe une valeur pour CSV (format COPY)
fn escape_csv(value: &str) -> String {
    // Remplacer les caractères problématiques
    // Utiliser E'\\x01' comme quote char pour minimiser les conflits
    value
        .replace('|', " ")
        .replace('\n', " ")
        .replace('\r', " ")
}

/// Ajoute le SRID au WKB pour créer du EWKB
fn add_srid_to_wkb(wkb: &[u8], srid: u32) -> Vec<u8> {
    if wkb.len() < 5 {
        return wkb.to_vec();
    }

    let mut ewkb = Vec::with_capacity(wkb.len() + 4);

    // Byte order
    ewkb.push(wkb[0]);

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

    // Copier le reste du WKB
    ewkb.extend_from_slice(&wkb[5..]);

    ewkb
}
