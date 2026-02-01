//! Définition et implémentation des commandes CLI
//!
//! CLI simplifiée:
//! - `import`: EDIGEO → PostGIS avec versioning
//! - `export`: EDIGEO → GeoJSON (sans DB)

use crate::export::reproject::Reprojector;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use bytes::BytesMut;
use clap::Subcommand;
use futures::stream;
use futures::StreamExt;
use geozero::wkt::WktWriter;
use geozero::GeozeroGeometry;
use rayon::prelude::*;
use tokio::sync::mpsc;
use tracing::{info, warn};

#[derive(Subcommand)]
pub enum Commands {
    /// Import EDIGEO data into PostGIS with temporal versioning
    Import {
        /// Path to EDIGEO archive (.tar.bz2) or directory
        #[arg(short, long)]
        path: PathBuf,

        /// Date of the millésime (YYYY-MM format, e.g., 2024-01)
        #[arg(short, long)]
        date: String,

        /// Target PostgreSQL schema
        #[arg(long, default_value = "cadastre")]
        schema: String,

        /// Config preset name (full/light/bati) or path to a JSON config
        #[arg(long, default_value = "full")]
        config: String,

        /// Drop schema before import
        #[arg(long)]
        drop_schema: bool,

        /// Drop final tables before import (sans supprimer le schema)
        #[arg(long)]
        drop_table: bool,

        /// Skip index creation at the end of the import (faster ingest; create later if needed)
        #[arg(long)]
        skip_indexes: bool,

        /// Target SRID for PostGIS tables (default: 4326 / WGS84)
        #[arg(long, default_value_t = 4326)]
        srid: u32,

        /// Coordinate precision (decimal places). Default: 7 for SRID 4326 (~1cm), 2 for metric SRIDs (~1cm)
        #[arg(long)]
        precision: Option<u8>,

        /// Departement code override (ex: 38, 2A) ou "fromFile"
        #[arg(long)]
        dep: Option<String>,

        /// Maximum number of archives processed concurrently
        #[arg(long, alias = "threads")]
        jobs: Option<usize>,

        /// PostgreSQL host (défaut : env PGHOST / localhost)
        #[arg(long)]
        host: Option<String>,

        /// PostgreSQL database name (défaut : env PGDATABASE / cadastre)
        #[arg(long)]
        database: Option<String>,

        /// PostgreSQL user (défaut : env PGUSER / postgres)
        #[arg(long)]
        user: Option<String>,

        /// PostgreSQL password (défaut : env PGPASSWORD)
        #[arg(long)]
        password: Option<String>,

        /// PostgreSQL port (défaut : env PGPORT / 5432)
        #[arg(long)]
        port: Option<u16>,

        /// SSL mode: disable, prefer, require (défaut : env PGSSLMODE / disable)
        #[arg(long)]
        ssl: Option<String>,
    },

    /// Export EDIGEO to GeoJSON (no database required)
    Export {
        /// Path to EDIGEO archive or directory
        #[arg(short, long)]
        path: PathBuf,

        /// Output directory for GeoJSON files
        #[arg(short, long)]
        output: PathBuf,

        /// Target SRID for reprojection (e.g., 4326 for WGS84)
        #[arg(long)]
        srid: Option<u32>,
    },
}

/// Exécute la commande import
pub async fn cmd_import(
    path: &Path,
    date: &str,
    schema: &str,
    config_spec: &str,
    drop_schema: bool,
    drop_table: bool,
    skip_indexes: bool,
    srid: u32,
    precision: Option<u8>,
    dep: Option<String>,
    host: Option<String>,
    database: Option<String>,
    user: Option<String>,
    password: Option<String>,
    port: Option<u16>,
    ssl: Option<String>,
    jobs: Option<usize>,
) -> Result<()> {
    // Valider le format de date
    validate_date_format(date)?;
    let valid_from = format!("{}-01", date); // YYYY-MM-01

    // Déterminer la précision des coordonnées
    // 4326 (degrés): 7 décimales ≈ 1 cm (6 décimales peut créer des auto-intersections)
    // Métriques (Lambert, UTM): 2 décimales ≈ 1 cm
    let coord_precision = precision.unwrap_or_else(|| if srid == 4326 { 7 } else { 2 });

    info!(
        path = %path.display(),
        date = %date,
        schema = schema,
        config = config_spec,
        drop_schema = drop_schema,
        "Starting import"
    );

    // Collecter les archives
    let archives = collect_archives(path)?;
    let num_archives = archives.len();

    if archives.is_empty() {
        anyhow::bail!("No EDIGEO archives (.tar.bz2) found in {}", path.display());
    }

    let jobs = jobs.unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
    });

    // Charger la configuration (presets Rust "full/light/bati" ou fichier JSON)
    let config = load_import_config(config_spec)?;
    let (table_specs, feature_type_to_table) = build_import_specs(&config)?;

    let dep_label = dep.as_deref().unwrap_or("auto").to_string();
    let dep_override = Arc::new(dep);

    println!("=== Import {} ===", date);
    println!("Path: {}", path.display());
    println!("Archives: {}", archives.len());
    println!("Schema: {}", schema);
    println!("Config: {}", config_spec);
    println!("Jobs: {}", jobs);
    println!("Drop schema: {}", drop_schema);
    println!("Drop tables: {}", drop_table);
    println!("Skip indexes: {}", skip_indexes);
    println!("Target SRID: {}", srid);
    println!("Coordinate precision: {} decimals", coord_precision);
    println!("Departement override: {}", dep_label);

    // Connecter à PostgreSQL
    let mut db_config = crate::export::pool::DatabaseConfig::from_env();
    apply_database_overrides(&mut db_config, host, database, user, password, port, ssl);
    println!(
        "Database: {}@{}:{}/{} (SSL: {:?})",
        db_config.user, db_config.host, db_config.port, db_config.dbname, db_config.ssl_mode
    );

    let pool = crate::export::pool::create_pool(&db_config).await?;
    crate::export::pool::test_connection(&pool).await?;
    println!("Connected to PostgreSQL");

    // Créer le schéma et les tables
    let pg_tables = table_specs
        .iter()
        .map(|t| crate::export::postgres::TableConfig {
            name: t.name.clone(),
            geometry_type: "Geometry".to_string(),
            srid,
            columns: t
                .columns
                .iter()
                .map(|c| crate::export::postgres::ColumnConfig {
                    name: c.name.clone(),
                    pg_type: pg_type_for(&c.data_type).to_string(),
                    source: c.source.clone(),
                })
                .collect(),
        })
        .collect::<Vec<_>>();

    if drop_table {
        crate::export::postgres::drop_tables(&pool, schema, &pg_tables).await?;
    }
    crate::export::postgres::create_schema(&pool, schema, &pg_tables, drop_schema).await?;
    crate::export::postgres::create_staging_tables(&pool, schema, &pg_tables).await?;
    println!("Schema ready");

    // Pre-load existing hashes for incremental import (skip unchanged features)
    let preload_started_at = std::time::Instant::now();
    let mut existing_hashes: HashMap<usize, std::collections::HashSet<[u8; 32]>> = HashMap::new();
    for (idx, table) in table_specs.iter().enumerate() {
        if table.hash_geom {
            let hashes =
                crate::export::postgres::load_existing_hashes(&pool, schema, &table.name).await?;
            if !hashes.is_empty() {
                println!("  {} existing hashes for {}", hashes.len(), table.name);
                existing_hashes.insert(idx, hashes);
            }
        }
    }
    let preload_duration = preload_started_at.elapsed();
    if !existing_hashes.is_empty() {
        println!("Hash preload: {:.2?}", preload_duration);
    }
    let existing_hashes = Arc::new(existing_hashes);

    let copy_started_at = std::time::Instant::now();

    // Démarrer 1 COPY stream par table (réduit drastiquement le nombre de transactions)
    let mut senders: Vec<mpsc::Sender<crate::export::postgres::CopyChunk>> = Vec::new();
    let mut copy_handles: Vec<(String, tokio::task::JoinHandle<Result<u64>>)> = Vec::new();

    for table in &table_specs {
        let (tx, rx) = mpsc::channel::<crate::export::postgres::CopyChunk>(16);
        senders.push(tx);

        let pool = pool.clone();
        let schema = schema.to_string();
        let logical_table = table.name.clone();
        let table_name = format!("_staging_{}", table.name);
        let dynamic_cols = table
            .columns
            .iter()
            .map(|c| c.name.clone())
            .collect::<Vec<_>>();

        copy_handles.push((
            logical_table,
            tokio::spawn(async move {
                crate::export::postgres::copy_csv_chunks(
                    &pool,
                    &schema,
                    &table_name,
                    &dynamic_cols,
                    rx,
                )
                .await
                .with_context(|| format!("COPY failed for {}.{}", schema, table_name))
            }),
        ));
    }

    // Parser et streamer en parallèle
    let senders = Arc::new(senders);
    let table_specs = Arc::new(table_specs);
    let feature_type_to_table = Arc::new(feature_type_to_table);

    let processed = Arc::new(AtomicUsize::new(0));
    let parse_errors = Arc::new(AtomicUsize::new(0));
    let skipped_types = Arc::new(AtomicUsize::new(0));
    let invalid_geometries = Arc::new(AtomicUsize::new(0));
    let skipped_existing = Arc::new(AtomicUsize::new(0));
    let skipped_archives = Arc::new(AtomicUsize::new(0));

    let pool_arc = Arc::new(pool.clone());
    let schema_arc = Arc::new(schema.to_string());

    stream::iter(archives.into_iter())
        .for_each_concurrent(jobs, |archive_path| {
            let senders = Arc::clone(&senders);
            let table_specs = Arc::clone(&table_specs);
            let feature_type_to_table = Arc::clone(&feature_type_to_table);
            let existing_hashes = Arc::clone(&existing_hashes);
            let valid_from = valid_from.clone();
            let processed = Arc::clone(&processed);
            let parse_errors = Arc::clone(&parse_errors);
            let skipped_types = Arc::clone(&skipped_types);
            let invalid_geometries = Arc::clone(&invalid_geometries);
            let skipped_existing = Arc::clone(&skipped_existing);
            let skipped_archives = Arc::clone(&skipped_archives);
            let dep_override = Arc::clone(&dep_override);
            let precision = coord_precision;
            let pool = Arc::clone(&pool_arc);
            let schema = Arc::clone(&schema_arc);

            async move {
                // Calculer le checksum de l'archive pour skip incrémental
                let archive_name = archive_path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
                    .to_string();

                let checksum = match tokio::task::spawn_blocking({
                    let path = archive_path.clone();
                    move || compute_file_checksum(&path)
                })
                .await
                {
                    Ok(Ok(cs)) => cs,
                    Ok(Err(e)) => {
                        warn!("Failed to compute checksum for {}: {}", archive_path.display(), e);
                        String::new()
                    }
                    Err(e) => {
                        warn!("Checksum task failed for {}: {}", archive_path.display(), e);
                        String::new()
                    }
                };

                // Vérifier si l'archive a déjà été importée avec le même checksum
                if !checksum.is_empty() {
                    match crate::export::postgres::is_archive_already_imported(
                        &pool, &schema, &archive_name, &checksum,
                    )
                    .await
                    {
                        Ok(true) => {
                            // Archive déjà importée, skip
                            skipped_archives.fetch_add(1, Ordering::Relaxed);
                            processed.fetch_add(1, Ordering::Relaxed);
                            return;
                        }
                        Ok(false) => {}
                        Err(e) => {
                            warn!("Failed to check archive status: {}", e);
                        }
                    }
                }

                let parse = tokio::task::spawn_blocking({
                    let archive_path = archive_path.clone();
                    move || edigeo::parse(&archive_path)
                })
                .await;

                let result = match parse {
                    Ok(Ok(r)) => r,
                    Ok(Err(e)) => {
                        warn!("Failed to parse {}: {}", archive_path.display(), e);
                        parse_errors.fetch_add(1, Ordering::Relaxed);
                        processed.fetch_add(1, Ordering::Relaxed);
                        return;
                    }
                    Err(e) => {
                        warn!(
                            "Failed to join parse task for {}: {}",
                            archive_path.display(),
                            e
                        );
                        parse_errors.fetch_add(1, Ordering::Relaxed);
                        processed.fetch_add(1, Ordering::Relaxed);
                        return;
                    }
                };

                let departement = match &*dep_override {
                    Some(value) if value.eq_ignore_ascii_case("fromfile") => {
                        derive_dep_from_archive(&archive_path)
                            .unwrap_or_else(|| result.departement.clone())
                    }
                    Some(value) => value.clone(),
                    None => result.departement.clone(),
                };
                let epsg = result.projection.epsg;
                let ewkt_prefix = format!("SRID={};", srid).into_bytes();

                let reprojector = match Reprojector::new(epsg, srid) {
                    Ok(r) => r,
                    Err(e) => {
                        warn!("Failed to build reprojector ({} → {}): {}", epsg, srid, e);
                        parse_errors.fetch_add(1, Ordering::Relaxed);
                        processed.fetch_add(1, Ordering::Relaxed);
                        return;
                    }
                };

                const BATCH_SIZE: u64 = 5000;

                let table_count = table_specs.len();
                let mut buffers: Vec<BytesMut> =
                    (0..table_count).map(|_| BytesMut::new()).collect();
                let mut buffer_rows: Vec<u64> = vec![0; table_count];

                let mut wkt_buf: Vec<u8> = Vec::with_capacity(1024);

                // Extraire les valeurs calculées (comme Node.js)
                let commune_idu = result
                    .features
                    .get("COMMUNE_id")
                    .and_then(|f| f.first())
                    .and_then(|f| f.properties.get("IDU"))
                    .cloned()
                    .unwrap_or_default();

                let section_idu = result
                    .features
                    .get("SECTION_id")
                    .and_then(|f| f.first())
                    .and_then(|f| f.properties.get("IDU"))
                    .cloned()
                    .unwrap_or_default();

                // Contexte calculé pour l'archive
                let computed_context = ComputedContext {
                    commune_id: commune_idu,
                    section_id: section_idu,
                };

                for (feature_type, features) in result.features {
                    let key = normalize_feature_type(&feature_type);
                    let Some(&table_idx) = feature_type_to_table.get(&key) else {
                        skipped_types.fetch_add(1, Ordering::Relaxed);
                        continue;
                    };

                    let table = &table_specs[table_idx];
                    let buf = &mut buffers[table_idx];

                    // Allocations amorties par table / archive
                    if buf.capacity() < 64 * 1024 {
                        buf.reserve(64 * 1024);
                    }

                    for feature in features {
                        let geometry = match reprojector.transform_geometry(&feature.geometry) {
                            Ok(g) => round_geometry_coords(&g, precision),
                            Err(e) => {
                                warn!(
                                    "Failed to reproject {} ({} → {}): {}",
                                    feature.id, epsg, srid, e
                                );
                                parse_errors.fetch_add(1, Ordering::Relaxed);
                                continue;
                            }
                        };

                        // Skip si le hash existe déjà (import incrémental)
                        if table.hash_geom {
                            if let Some(hash_set) = existing_hashes.get(&table_idx) {
                                let hash = crate::versioning::diff::geometry_hash(&geometry);
                                if hash_set.contains(&hash) {
                                    skipped_existing.fetch_add(1, Ordering::Relaxed);
                                    continue;
                                }
                            }
                        }

                        if let Err(e) = write_copy_row(
                            buf,
                            &feature,
                            &geometry,
                            &departement,
                            &valid_from,
                            &ewkt_prefix,
                            &mut wkt_buf,
                            table,
                            &computed_context,
                        ) {
                            let msg = e.to_string();
                            if msg.contains("Geometry not valid for PostGIS ingestion") {
                                invalid_geometries.fetch_add(1, Ordering::Relaxed);
                            } else {
                                warn!(
                                    "Failed to encode row ({} / {}): {}",
                                    feature_type, feature.id, msg
                                );
                            }
                            continue;
                        }
                        buffer_rows[table_idx] += 1;

                        // Flush quand on atteint BATCH_SIZE rows (5000)
                        if buffer_rows[table_idx] >= BATCH_SIZE {
                            let rows = buffer_rows[table_idx];
                            buffer_rows[table_idx] = 0;
                            let data = buf.split().freeze();
                            if rows > 0 && !data.is_empty() {
                                if let Err(e) = senders[table_idx]
                                    .send(crate::export::postgres::CopyChunk { data, rows })
                                    .await
                                {
                                    warn!("Failed to send COPY chunk: {}", e);
                                    break;
                                }
                            }
                        }
                    }
                }

                // Flush final
                for (idx, buf) in buffers.into_iter().enumerate() {
                    let rows = buffer_rows[idx];
                    if rows == 0 || buf.is_empty() {
                        continue;
                    }
                    let data = buf.freeze();
                    if let Err(e) = senders[idx]
                        .send(crate::export::postgres::CopyChunk { data, rows })
                        .await
                    {
                        warn!("Failed to send final COPY chunk: {}", e);
                    }
                }

                // Enregistrer le checksum de l'archive après import réussi
                if !checksum.is_empty() {
                    if let Err(e) = crate::export::postgres::record_archive_checksum(
                        &pool, &schema, &archive_name, &checksum,
                    )
                    .await
                    {
                        warn!("Failed to record archive checksum: {}", e);
                    }
                }

                let done = processed.fetch_add(1, Ordering::Relaxed) + 1;
                if done % 100 == 0 {
                    info!(processed = done, "Import progress");
                }
            }
        })
        .await;

    // Fermer les channels pour terminer les COPY
    drop(senders);

    let mut staged_by_table: HashMap<String, u64> = HashMap::new();
    let mut total_staged: u64 = 0;
    for (table, handle) in copy_handles {
        let staged = handle.await??;
        staged_by_table.insert(table, staged);
        total_staged += staged;
    }

    let copy_duration = copy_started_at.elapsed();

    // Merge staging -> final (DO NOTHING sur doublons)
    let merge_started_at = std::time::Instant::now();
    let mut inserted_by_table: HashMap<String, u64> = HashMap::new();
    let mut merged_total: u64 = 0;
    for table in table_specs.iter() {
        let dynamic_cols = table
            .columns
            .iter()
            .map(|c| c.name.clone())
            .collect::<Vec<_>>();
        let inserted = crate::export::postgres::merge_staging_into_table(
            &pool,
            schema,
            &table.name,
            &dynamic_cols,
        )
        .await?;
        inserted_by_table.insert(table.name.clone(), inserted);
        merged_total += inserted;
        info!(
            table = table.name.as_str(),
            inserted = inserted,
            "Merged staging table"
        );
    }

    crate::export::postgres::drop_staging_tables(&pool, schema, &pg_tables).await?;

    // Indexes après merge (beaucoup plus rapide que maintenir des indexes pendant l'import)
    let merge_duration = merge_started_at.elapsed();

    let indexes_started_at = std::time::Instant::now();
    if !skip_indexes {
        for table in table_specs.iter() {
            crate::export::postgres::create_indexes(&pool, schema, &table.name).await?;
        }
    }
    let indexes_duration = indexes_started_at.elapsed();

    let total_errors = parse_errors.load(Ordering::Relaxed);
    let total_skipped_types = skipped_types.load(Ordering::Relaxed);
    let total_invalid_geometries = invalid_geometries.load(Ordering::Relaxed);
    let total_skipped_existing = skipped_existing.load(Ordering::Relaxed);
    let total_skipped_archives = skipped_archives.load(Ordering::Relaxed);

    println!("\n\n=== Summary ===");
    println!("Date: {}", date);
    if total_skipped_archives > 0 {
        println!(
            "Skipped archives (unchanged): {}/{}",
            total_skipped_archives, num_archives
        );
    }
    if total_skipped_existing > 0 {
        println!("Skipped features (already exist): {}", total_skipped_existing);
    }
    println!("Rows staged: {}", total_staged);
    println!("Rows inserted: {}", merged_total);
    println!("Copy duration: {:.2?}", copy_duration);
    println!("Merge duration: {:.2?}", merge_duration);
    if skip_indexes {
        println!("Indexes: skipped");
    } else {
        println!("Indexes duration: {:.2?}", indexes_duration);
    }

    println!("\nPer-table:");
    for table in table_specs.iter() {
        let staged = staged_by_table.get(&table.name).copied().unwrap_or(0);
        let inserted = inserted_by_table.get(&table.name).copied().unwrap_or(0);
        let duplicates = staged.saturating_sub(inserted);
        println!(
            "- {}: staged {}, inserted {}, duplicates {}",
            table.name, staged, inserted, duplicates
        );
    }

    if total_errors > 0 {
        println!("Parse errors: {}", total_errors);
    }
    if total_skipped_types > 0 {
        println!(
            "Skipped feature groups (unconfigured): {}",
            total_skipped_types
        );
    }
    if total_invalid_geometries > 0 {
        println!("Skipped invalid geometries: {}", total_invalid_geometries);
    }

    info!(
        "Import complete: {} inserted, {} parse errors",
        merged_total, total_errors
    );

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ColumnSpec {
    name: String,
    source: String,
    data_type: String,
    prefix_dep: bool,
}

#[derive(Debug, Clone)]
struct TableSpec {
    name: String,
    columns: Vec<ColumnSpec>,
    hash_geom: bool,
}

fn load_import_config(spec: &str) -> Result<crate::config::Config> {
    match spec {
        "full" | "light" | "bati" => crate::config::Config::from_preset(spec),
        _ => crate::config::Config::load(Path::new(spec)),
    }
}

fn normalize_feature_type(feature_type: &str) -> String {
    feature_type.trim().to_uppercase()
}

fn is_reserved_column(name: &str) -> bool {
    matches!(
        name,
        "row_id"
            | "id"
            | "departement"
            | "geometry"
            | "valid_from"
            | "valid_to"
            | "geometry_hash"
            | "created_at"
            | "updated_at"
    )
}

fn pg_type_for(data_type: &str) -> &'static str {
    match data_type.to_ascii_lowercase().as_str() {
        "text" => "TEXT",
        "varchar" => "TEXT",
        "integer" | "int" => "INTEGER",
        "smallint" => "SMALLINT",
        "bigint" => "BIGINT",
        "float" | "double" | "double precision" => "DOUBLE PRECISION",
        "boolean" | "bool" => "BOOLEAN",
        "date" => "DATE",
        _ => "TEXT",
    }
}

fn apply_database_overrides(
    config: &mut crate::export::pool::DatabaseConfig,
    host: Option<String>,
    database: Option<String>,
    user: Option<String>,
    password: Option<String>,
    port: Option<u16>,
    ssl: Option<String>,
) {
    if let Some(host) = host {
        config.host = host;
    }
    if let Some(database) = database {
        config.dbname = database;
    }
    if let Some(user) = user {
        config.user = user;
    }
    if let Some(password) = password {
        config.password = Some(password);
    }
    if let Some(port) = port {
        config.port = port;
    }
    if let Some(ssl) = ssl {
        if let Ok(mode) = ssl.parse() {
            config.ssl_mode = mode;
        }
    }
}

fn build_import_specs(
    config: &crate::config::Config,
) -> Result<(Vec<TableSpec>, HashMap<String, usize>)> {
    let mut tables: Vec<TableSpec> = Vec::new();
    let mut table_index_by_name: HashMap<String, usize> = HashMap::new();
    let mut feature_type_to_table: HashMap<String, usize> = HashMap::new();

    for (feature_type, table_cfg) in &config.tables {
        let idx = *table_index_by_name
            .entry(table_cfg.table.clone())
            .or_insert_with(|| {
                let idx = tables.len();
                tables.push(TableSpec {
                    name: table_cfg.table.clone(),
                    columns: Vec::new(),
                    hash_geom: table_cfg.hash_geom,
                });
                idx
            });

        let cols = table_cfg
            .fields
            .iter()
            .filter(|f| !is_reserved_column(f.target.as_str()))
            .map(|f| ColumnSpec {
                name: f.target.clone(),
                source: f.source.clone(),
                data_type: f.data_type.clone(),
                prefix_dep: f.prefix_dep,
            })
            .collect::<Vec<_>>();

        if tables[idx].columns.is_empty() {
            tables[idx].columns = cols;
        } else if tables[idx].columns != cols {
            anyhow::bail!(
                "Conflicting table configs for '{}': multiple feature types map to the same table with different column layouts",
                tables[idx].name
            );
        }

        tables[idx].hash_geom = tables[idx].hash_geom || table_cfg.hash_geom;

        let ft = normalize_feature_type(feature_type);
        feature_type_to_table.insert(ft.clone(), idx);

        // Tolérance: si on reçoit des types sans suffixe `_id`, on mappe aussi.
        if let Some(stripped) = ft.strip_suffix("_ID") {
            feature_type_to_table.insert(stripped.to_string(), idx);
        }
    }

    Ok((tables, feature_type_to_table))
}

fn derive_dep_from_archive(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let rest = name.strip_prefix("edigeo-")?;
    let mut chars = rest.chars();
    let mut dep = String::new();

    while dep.len() < 2 {
        match chars.next() {
            Some(c) if c.is_ascii_digit() => {
                dep.push(c);
            }
            Some(c @ 'A') | Some(c @ 'B') if dep == "2" => {
                dep.push(c);
                break;
            }
            _ => break,
        }
    }

    if dep == "97" || dep == "98" {
        if let Some(c) = chars.next() {
            if c.is_ascii_digit() {
                dep.push(c);
            }
        }
    }

    if dep.len() >= 2 {
        Some(dep)
    } else {
        None
    }
}

/// Parse un nombre EDIGEO qui peut avoir des formats spéciaux:
/// - "+1895." → 1895.0
/// - "01" → 1.0
/// - "+45.5" → 45.5
fn parse_edigeo_number(raw: &str) -> Option<f64> {
    let v = raw.trim();
    if v.is_empty() {
        return None;
    }

    // Retirer le + au début et le . à la fin si orphelin
    let cleaned = v
        .trim_start_matches('+')
        .trim_end_matches(|c: char| c == '.' && !v.contains('.'));

    // Cas spécial: "1895." → "1895"
    let cleaned = if cleaned.ends_with('.') && !cleaned.contains('e') {
        &cleaned[..cleaned.len() - 1]
    } else {
        cleaned
    };

    cleaned.parse::<f64>().ok()
}

/// Arrondit les coordonnées d'une géométrie à la précision spécifiée
fn round_geometry_coords(geom: &geo::Geometry, decimals: u8) -> geo::Geometry {
    use geo::{Coord, Geometry, LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon};

    let factor = 10_f64.powi(decimals as i32);

    let round_coord = |c: &Coord| -> Coord {
        Coord {
            x: (c.x * factor).round() / factor,
            y: (c.y * factor).round() / factor,
        }
    };

    let round_line = |ls: &LineString| -> LineString {
        LineString::new(ls.0.iter().map(round_coord).collect())
    };

    match geom {
        Geometry::Point(p) => {
            let c = round_coord(&p.0);
            Geometry::Point(Point::from(c))
        }
        Geometry::LineString(ls) => Geometry::LineString(round_line(ls)),
        Geometry::Polygon(poly) => {
            let exterior = round_line(poly.exterior());
            let interiors: Vec<LineString> = poly.interiors().iter().map(round_line).collect();
            Geometry::Polygon(Polygon::new(exterior, interiors))
        }
        Geometry::MultiPoint(mp) => {
            let points: Vec<Point> = mp.0.iter().map(|p| Point::from(round_coord(&p.0))).collect();
            Geometry::MultiPoint(MultiPoint::new(points))
        }
        Geometry::MultiLineString(mls) => {
            let lines: Vec<LineString> = mls.0.iter().map(round_line).collect();
            Geometry::MultiLineString(MultiLineString::new(lines))
        }
        Geometry::MultiPolygon(mpoly) => {
            let polys: Vec<Polygon> = mpoly.0.iter().map(|poly| {
                let exterior = round_line(poly.exterior());
                let interiors: Vec<LineString> = poly.interiors().iter().map(round_line).collect();
                Polygon::new(exterior, interiors)
            }).collect();
            Geometry::MultiPolygon(MultiPolygon::new(polys))
        }
        // Pour les autres types, on retourne tel quel
        other => other.clone(),
    }
}

/// Contexte calculé par archive (valeurs dérivées comme Node.js)
struct ComputedContext {
    /// IDU de la première commune de l'archive
    commune_id: String,
    /// IDU de la première section de l'archive
    section_id: String,
}

fn push_csv_text_field(buf: &mut BytesMut, value: &str) {
    buf.extend_from_slice(b"\"");
    for b in value.as_bytes() {
        match *b {
            b'"' => buf.extend_from_slice(b"\"\""),
            b'\n' | b'\r' => buf.extend_from_slice(b" "),
            _ => buf.extend_from_slice(&[*b]),
        }
    }
    buf.extend_from_slice(b"\"");
}

fn write_copy_row(
    buf: &mut BytesMut,
    feature: &edigeo::Feature,
    geometry: &geo::Geometry,
    departement: &str,
    valid_from: &str,
    ewkt_prefix: &[u8],
    wkt_buf: &mut Vec<u8>,
    table: &TableSpec,
    computed: &ComputedContext,
) -> Result<()> {
    let start_len = buf.len();

    let res: Result<()> = (|| {
        // id (préfixé avec le département pour unicité France entière)
        let prefixed_id = format!("{}{}", departement, feature.id);
        push_csv_text_field(buf, &prefixed_id);
        buf.extend_from_slice(b"|");

        // departement
        push_csv_text_field(buf, departement);
        buf.extend_from_slice(b"|");

        // geometry (EWKT: SRID=...;WKT)
        if !geometry_ok_for_postgis(geometry) {
            anyhow::bail!("Geometry not valid for PostGIS ingestion (too few points)");
        }
        wkt_buf.clear();
        {
            let mut writer = WktWriter::new(&mut *wkt_buf);
            geometry
                .process_geom(&mut writer)
                .context("Failed to encode geometry to WKT")?;
        }
        buf.extend_from_slice(b"\"");
        buf.extend_from_slice(ewkt_prefix);
        buf.extend_from_slice(&wkt_buf[..]);
        buf.extend_from_slice(b"\"");
        buf.extend_from_slice(b"|");

        // valid_from (date ISO)
        buf.extend_from_slice(valid_from.as_bytes());
        buf.extend_from_slice(b"|");

        // geometry_hash (optionnel selon config)
        if table.hash_geom {
            let hash = crate::versioning::diff::geometry_hash(geometry);
            let hash_hex = hex::encode(hash);
            buf.extend_from_slice(b"\\x");
            buf.extend_from_slice(hash_hex.as_bytes());
        }

        // Colonnes dynamiques
        for col in &table.columns {
            buf.extend_from_slice(b"|");

            // Valeurs calculées (comme Node.js "const")
            let raw_value: &str = match col.source.as_str() {
                "IDU_COMMUNE" => &computed.commune_id,
                "IDU_SECTION" => &computed.section_id,
                _ => feature
                    .properties
                    .get(&col.source)
                    .map(String::as_str)
                    .unwrap_or(""),
            };

            // Appliquer le préfixe département si demandé (comme addDep de Node.js)
            let final_value: std::borrow::Cow<str> = if col.prefix_dep && !raw_value.is_empty() {
                std::borrow::Cow::Owned(format!("{}{}", departement, raw_value))
            } else {
                std::borrow::Cow::Borrowed(raw_value)
            };
            let raw = final_value.as_ref();

            match col.data_type.to_ascii_lowercase().as_str() {
                "integer" | "int" | "smallint" | "bigint" => {
                    // EDIGEO peut avoir des formats comme "+1895." → on nettoie
                    if let Some(n) = parse_edigeo_number(raw) {
                        buf.extend_from_slice(n.trunc().to_string().as_bytes());
                    }
                }
                "float" | "double" | "double precision" => {
                    if let Some(n) = parse_edigeo_number(raw) {
                        buf.extend_from_slice(n.to_string().as_bytes());
                    }
                }
                _ => push_csv_text_field(buf, raw),
            }
        }

        buf.extend_from_slice(b"\n");
        Ok(())
    })();

    if res.is_err() {
        buf.truncate(start_len);
    }

    res
}

fn geometry_ok_for_postgis(geom: &geo::Geometry) -> bool {
    use geo::{Geometry, LineString, MultiLineString, MultiPolygon, Polygon};

    fn ring_ok(r: &LineString) -> bool {
        // LinearRing: >= 4 points, first == last
        if r.0.len() < 4 {
            return false;
        }
        match (r.0.first(), r.0.last()) {
            (Some(a), Some(b)) => a == b,
            _ => false,
        }
    }

    fn polygon_ok(p: &Polygon) -> bool {
        ring_ok(p.exterior()) && p.interiors().iter().all(ring_ok)
    }

    fn multilines_ok(mls: &MultiLineString) -> bool {
        mls.0.iter().all(|ls| ls.0.len() >= 2)
    }

    fn multipoly_ok(mp: &MultiPolygon) -> bool {
        mp.0.iter().all(polygon_ok)
    }

    match geom {
        Geometry::Point(_) => true,
        Geometry::MultiPoint(_) => true,
        Geometry::LineString(ls) => ls.0.len() >= 2,
        Geometry::MultiLineString(mls) => multilines_ok(mls),
        Geometry::Polygon(p) => polygon_ok(p),
        Geometry::MultiPolygon(mp) => multipoly_ok(mp),
        Geometry::GeometryCollection(gc) => gc.0.iter().all(geometry_ok_for_postgis),
        _ => true,
    }
}

/// Exécute la commande export
pub async fn cmd_export(path: &Path, output: &Path, target_srid: Option<u32>) -> Result<()> {
    info!(
        "Export: path={}, output={}, srid={:?}",
        path.display(),
        output.display(),
        target_srid
    );

    std::fs::create_dir_all(output)?;

    if path.is_dir() {
        export_directory(path, output, target_srid)?;
    } else {
        export_single_archive(path, output, target_srid)?;
    }

    Ok(())
}

/// Valide le format de date YYYY-MM
fn validate_date_format(date: &str) -> Result<()> {
    if date.len() != 7 || date.chars().nth(4) != Some('-') {
        anyhow::bail!(
            "Invalid date format: '{}'. Expected YYYY-MM (e.g., 2024-01)",
            date
        );
    }

    let year: u32 = date[0..4]
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid year in date: {}", date))?;
    let month: u32 = date[5..7]
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid month in date: {}", date))?;

    if !(1900..=2100).contains(&year) {
        anyhow::bail!("Year out of range: {}", year);
    }
    if !(1..=12).contains(&month) {
        anyhow::bail!("Month must be 01-12, got: {:02}", month);
    }

    Ok(())
}

/// Exporte une seule archive EDIGEO vers GeoJSON
fn export_single_archive(path: &Path, output: &Path, target_srid: Option<u32>) -> Result<()> {
    let parse_result = edigeo::parse(path)?;

    info!(
        "Parsed: {} feature types, projection=EPSG:{}",
        parse_result.features.len(),
        parse_result.projection.epsg
    );

    // Créer le reprojector si nécessaire
    #[cfg(feature = "reproject")]
    let reprojector = target_srid
        .filter(|&srid| srid != parse_result.projection.epsg)
        .map(|srid| crate::export::reproject::Reprojector::new(parse_result.projection.epsg, srid))
        .transpose()?;

    let output_epsg = target_srid.unwrap_or(parse_result.projection.epsg);

    for (feature_type, features) in &parse_result.features {
        let filename = feature_type.to_lowercase();
        let output_file = output.join(format!("{}.geojson", filename));

        #[cfg(feature = "reproject")]
        if let Some(ref reproj) = reprojector {
            // Reprojeter et exporter
            let reprojected: Result<Vec<edigeo::Feature>> = features
                .iter()
                .map(|f| {
                    let new_geom = reproj.transform_geometry(&f.geometry)?;
                    Ok(edigeo::Feature {
                        id: f.id.clone(),
                        geometry: new_geom,
                        properties: f.properties.clone(),
                        feature_type: f.feature_type.clone(),
                    })
                })
                .collect();
            let reprojected = reprojected?;
            let proj = edigeo::Projection {
                epsg: output_epsg,
                name: "",
            };
            crate::export::geojson::export_to_geojson(&reprojected, &proj, &output_file)?;
        } else {
            crate::export::geojson::export_to_geojson(
                features,
                &parse_result.projection,
                &output_file,
            )?;
        }

        #[cfg(not(feature = "reproject"))]
        crate::export::geojson::export_to_geojson(
            features,
            &parse_result.projection,
            &output_file,
        )?;

        info!(
            "Exported {} {} to {}",
            features.len(),
            feature_type,
            output_file.display()
        );
    }

    println!(
        "Export complete: {} types to {} (EPSG:{})",
        parse_result.features.len(),
        output.display(),
        output_epsg
    );

    Ok(())
}

/// Exporte un dossier d'archives EDIGEO en parallèle
fn export_directory(path: &Path, output: &Path, target_srid: Option<u32>) -> Result<()> {
    let archives = collect_archives(path)?;

    if archives.is_empty() {
        anyhow::bail!("No EDIGEO archives found in {}", path.display());
    }

    info!("Found {} archives to export", archives.len());

    let success_count = Arc::new(AtomicUsize::new(0));
    let error_count = Arc::new(AtomicUsize::new(0));
    let feature_count = Arc::new(AtomicUsize::new(0));

    archives.par_iter().for_each(|archive_path| {
        match process_archive_for_export(archive_path, output, target_srid) {
            Ok(count) => {
                success_count.fetch_add(1, Ordering::Relaxed);
                feature_count.fetch_add(count, Ordering::Relaxed);
            }
            Err(e) => {
                warn!("Failed to export {}: {}", archive_path.display(), e);
                error_count.fetch_add(1, Ordering::Relaxed);
            }
        }
    });

    let success = success_count.load(Ordering::Relaxed);
    let errors = error_count.load(Ordering::Relaxed);
    let features = feature_count.load(Ordering::Relaxed);

    let epsg_info = target_srid
        .map(|s| format!(" (EPSG:{})", s))
        .unwrap_or_default();

    println!(
        "Export complete: {}/{} archives, {} features{}",
        success,
        archives.len(),
        features,
        epsg_info
    );

    if errors > 0 {
        warn!("{} archives failed", errors);
    }

    Ok(())
}

/// Traite une archive pour l'export
fn process_archive_for_export(
    archive_path: &Path,
    output: &Path,
    target_srid: Option<u32>,
) -> Result<usize> {
    let parse_result = edigeo::parse(archive_path)
        .with_context(|| format!("Failed to parse {}", archive_path.display()))?;

    let archive_name = get_archive_basename(archive_path);
    let archive_output = output.join(&archive_name);
    std::fs::create_dir_all(&archive_output)?;

    // Créer le reprojector si nécessaire
    #[cfg(feature = "reproject")]
    let reprojector = target_srid
        .filter(|&srid| srid != parse_result.projection.epsg)
        .map(|srid| crate::export::reproject::Reprojector::new(parse_result.projection.epsg, srid))
        .transpose()?;

    let output_epsg = target_srid.unwrap_or(parse_result.projection.epsg);

    let mut total_features = 0;

    for (feature_type, features) in &parse_result.features {
        let filename = feature_type.to_lowercase();
        let output_file = archive_output.join(format!("{}.geojson", filename));

        #[cfg(feature = "reproject")]
        if let Some(ref reproj) = reprojector {
            let reprojected: Result<Vec<edigeo::Feature>> = features
                .iter()
                .map(|f| {
                    let new_geom = reproj.transform_geometry(&f.geometry)?;
                    Ok(edigeo::Feature {
                        id: f.id.clone(),
                        geometry: new_geom,
                        properties: f.properties.clone(),
                        feature_type: f.feature_type.clone(),
                    })
                })
                .collect();
            let reprojected = reprojected?;
            let proj = edigeo::Projection {
                epsg: output_epsg,
                name: "",
            };
            crate::export::geojson::export_to_geojson(&reprojected, &proj, &output_file)?;
        } else {
            crate::export::geojson::export_to_geojson(
                features,
                &parse_result.projection,
                &output_file,
            )?;
        }

        #[cfg(not(feature = "reproject"))]
        crate::export::geojson::export_to_geojson(
            features,
            &parse_result.projection,
            &output_file,
        )?;

        total_features += features.len();
    }

    Ok(total_features)
}

/// Extrait le nom de base d'une archive (sans .tar.bz2, .tar, .bz2)
fn get_archive_basename(path: &Path) -> String {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");

    // Supprimer les extensions connues
    let name = name
        .strip_suffix(".tar.bz2")
        .or_else(|| name.strip_suffix(".bz2"))
        .or_else(|| name.strip_suffix(".tar"))
        .unwrap_or(name);

    name.to_string()
}

/// Collecte récursivement les archives EDIGEO
fn collect_archives(path: &Path) -> Result<Vec<PathBuf>> {
    let mut archives = Vec::new();

    if path.is_file() {
        if path.extension().map_or(false, |ext| ext == "bz2") {
            archives.push(path.to_path_buf());
        }
        return Ok(archives);
    }

    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let entry_path = entry.path();

        if entry_path.is_dir() {
            archives.extend(collect_archives(&entry_path)?);
        } else if entry_path.extension().map_or(false, |ext| ext == "bz2") {
            archives.push(entry_path);
        }
    }

    Ok(archives)
}

/// Calcule le checksum blake3 d'un fichier
fn compute_file_checksum(path: &Path) -> Result<String> {
    use std::fs::File;
    use std::io::Read;

    let mut file = File::open(path).with_context(|| format!("Cannot open {}", path.display()))?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0u8; 65536]; // 64KB buffer

    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    Ok(hasher.finalize().to_hex().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_date_format_valid() {
        assert!(validate_date_format("2024-01").is_ok());
        assert!(validate_date_format("2024-04").is_ok());
        assert!(validate_date_format("2024-12").is_ok());
        assert!(validate_date_format("2025-07").is_ok());
    }

    #[test]
    fn test_validate_date_format_invalid() {
        assert!(validate_date_format("2024").is_err());
        assert!(validate_date_format("2024-1").is_err());
        assert!(validate_date_format("2024-001").is_err());
        assert!(validate_date_format("24-01").is_err());
        assert!(validate_date_format("2024/01").is_err());
        assert!(validate_date_format("").is_err());
    }

    #[test]
    fn test_validate_date_format_invalid_month() {
        assert!(validate_date_format("2024-00").is_err());
        assert!(validate_date_format("2024-13").is_err());
        assert!(validate_date_format("2024-99").is_err());
    }

    #[test]
    fn test_validate_date_format_year_range() {
        assert!(validate_date_format("1899-01").is_err());
        assert!(validate_date_format("2101-01").is_err());
        assert!(validate_date_format("1900-01").is_ok());
        assert!(validate_date_format("2100-12").is_ok());
    }

    #[test]
    fn test_get_archive_basename() {
        use std::path::Path;

        assert_eq!(
            get_archive_basename(Path::new("edigeo-39001000AH01.tar.bz2")),
            "edigeo-39001000AH01"
        );
        assert_eq!(
            get_archive_basename(Path::new("/path/to/archive.bz2")),
            "archive"
        );
        assert_eq!(get_archive_basename(Path::new("file.tar")), "file");
        assert_eq!(
            get_archive_basename(Path::new("noextension")),
            "noextension"
        );
    }
}
