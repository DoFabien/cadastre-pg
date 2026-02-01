//! Tests d'intégration PostgreSQL
//!
//! Ces tests nécessitent une base PostgreSQL disponible.
//! Configuration via variables d'environnement:
//! - PGHOST, PGPORT, PGUSER, PGPASSWORD, PGDATABASE
//!
//! Exécution:
//! ```bash
//! # Avec PostgreSQL local
//! cargo test --test postgres_integration -- --ignored
//!
//! # Avec Docker
//! docker run -d --name postgres-test -e POSTGRES_PASSWORD=test -p 5432:5432 postgis/postgis
//! PGPASSWORD=test cargo test --test postgres_integration -- --ignored
//! ```

use anyhow::Result;
use deadpool_postgres::{Config, Pool, Runtime};
use tokio_postgres::NoTls;

/// Configuration de test
fn test_config() -> Config {
    let mut cfg = Config::new();
    cfg.host = Some(std::env::var("PGHOST").unwrap_or_else(|_| "localhost".into()));
    cfg.port = Some(
        std::env::var("PGPORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(5432),
    );
    cfg.dbname = Some(std::env::var("PGDATABASE").unwrap_or_else(|_| "cadastre_test".into()));
    cfg.user = Some(std::env::var("PGUSER").unwrap_or_else(|_| "postgres".into()));
    cfg.password = std::env::var("PGPASSWORD").ok();
    cfg
}

/// Crée un pool de connexions de test
async fn create_test_pool() -> Result<Pool> {
    let cfg = test_config();
    let pool = cfg.create_pool(Some(Runtime::Tokio1), NoTls)?;
    Ok(pool)
}

/// Configure la base de test avec le schéma
async fn setup_test_schema(pool: &Pool) -> Result<()> {
    let client = pool.get().await?;

    // Supprimer et recréer le schéma
    client
        .batch_execute(
            r#"
            DROP SCHEMA IF EXISTS cadastre CASCADE;
            CREATE SCHEMA cadastre;

            CREATE EXTENSION IF NOT EXISTS postgis;

            CREATE TABLE cadastre.parcelles (
                row_id BIGSERIAL PRIMARY KEY,
                id TEXT NOT NULL,
                departement VARCHAR(3),
                geometry geometry(Geometry, 2154),
                valid_from DATE NOT NULL,
                valid_to DATE,
                geometry_hash BYTEA,
                created_at TIMESTAMPTZ DEFAULT NOW()
            );

            CREATE TABLE cadastre.sections (
                row_id BIGSERIAL PRIMARY KEY,
                id TEXT NOT NULL,
                departement VARCHAR(3),
                geometry geometry(Geometry, 2154),
                valid_from DATE NOT NULL,
                valid_to DATE,
                geometry_hash BYTEA,
                created_at TIMESTAMPTZ DEFAULT NOW()
            );

            CREATE TABLE cadastre.communes (
                row_id BIGSERIAL PRIMARY KEY,
                id TEXT NOT NULL,
                departement VARCHAR(3),
                geometry geometry(Geometry, 2154),
                valid_from DATE NOT NULL,
                valid_to DATE,
                geometry_hash BYTEA,
                created_at TIMESTAMPTZ DEFAULT NOW()
            );

            CREATE TABLE cadastre.batiments (
                row_id BIGSERIAL PRIMARY KEY,
                id TEXT NOT NULL,
                departement VARCHAR(3),
                geometry geometry(Geometry, 2154),
                valid_from DATE NOT NULL,
                valid_to DATE,
                geometry_hash BYTEA,
                created_at TIMESTAMPTZ DEFAULT NOW()
            );

            CREATE INDEX idx_parcelles_id ON cadastre.parcelles(id);
            CREATE INDEX idx_parcelles_valid ON cadastre.parcelles(valid_from, valid_to);
            CREATE INDEX idx_sections_id ON cadastre.sections(id);
            CREATE INDEX idx_communes_id ON cadastre.communes(id);
            CREATE INDEX idx_batiments_id ON cadastre.batiments(id);
            "#,
        )
        .await?;

    Ok(())
}

/// Test de connexion basique
#[tokio::test]
#[ignore = "Requires PostgreSQL database"]
async fn test_database_connection() {
    let pool = create_test_pool().await.expect("Failed to create pool");
    let client = pool.get().await.expect("Failed to get client");

    let row = client
        .query_one("SELECT 1 as test", &[])
        .await
        .expect("Query failed");
    let value: i32 = row.get("test");
    assert_eq!(value, 1);
}

/// Test de création du schéma
#[tokio::test]
#[ignore = "Requires PostgreSQL database"]
async fn test_schema_creation() {
    let pool = create_test_pool().await.expect("Failed to create pool");
    setup_test_schema(&pool)
        .await
        .expect("Failed to setup schema");

    let client = pool.get().await.expect("Failed to get client");

    // Vérifier que les tables existent
    let tables = client
        .query(
            "SELECT table_name FROM information_schema.tables WHERE table_schema = 'cadastre'",
            &[],
        )
        .await
        .expect("Failed to query tables");

    let table_names: Vec<String> = tables.iter().map(|r| r.get(0)).collect();

    assert!(table_names.contains(&"parcelles".to_string()));
    assert!(table_names.contains(&"sections".to_string()));
    assert!(table_names.contains(&"communes".to_string()));
    assert!(table_names.contains(&"batiments".to_string()));
}

/// Test du marquage temporel
#[tokio::test]
#[ignore = "Requires PostgreSQL database"]
async fn test_temporal_marking() {
    let pool = create_test_pool().await.expect("Failed to create pool");
    setup_test_schema(&pool)
        .await
        .expect("Failed to setup schema");

    let client = pool.get().await.expect("Failed to get client");

    // Insérer quelques entités de test
    client
        .execute(
            "INSERT INTO cadastre.parcelles (id, valid_from, geometry_hash) VALUES ($1, $2, $3)",
            &[&"TEST001", &"2024-01-01", &vec![0u8; 32]],
        )
        .await
        .expect("Failed to insert");

    client
        .execute(
            "INSERT INTO cadastre.parcelles (id, valid_from, geometry_hash) VALUES ($1, $2, $3)",
            &[&"TEST002", &"2024-01-01", &vec![1u8; 32]],
        )
        .await
        .expect("Failed to insert");

    // Marquer toutes les entités comme potentiellement terminées
    let rows = client
        .execute(
            "UPDATE cadastre.parcelles SET valid_to = $1 WHERE valid_to IS NULL",
            &[&"2025-01-01"],
        )
        .await
        .expect("Failed to mark");

    assert_eq!(rows, 2);

    // Réactiver une entité
    client
        .execute(
            "UPDATE cadastre.parcelles SET valid_to = NULL WHERE id = $1",
            &[&"TEST001"],
        )
        .await
        .expect("Failed to reactivate");

    // Vérifier les états
    let active: i64 = client
        .query_one(
            "SELECT COUNT(*) FROM cadastre.parcelles WHERE valid_to IS NULL",
            &[],
        )
        .await
        .expect("Failed to count")
        .get(0);

    let ended: i64 = client
        .query_one(
            "SELECT COUNT(*) FROM cadastre.parcelles WHERE valid_to IS NOT NULL",
            &[],
        )
        .await
        .expect("Failed to count")
        .get(0);

    assert_eq!(active, 1);
    assert_eq!(ended, 1);
}

/// Test du géocodage historique
#[tokio::test]
#[ignore = "Requires PostgreSQL database"]
async fn test_historical_geocoding() {
    let pool = create_test_pool().await.expect("Failed to create pool");
    setup_test_schema(&pool)
        .await
        .expect("Failed to setup schema");

    let client = pool.get().await.expect("Failed to get client");

    // Créer un historique: parcelle créée en 2020, modifiée en 2022, supprimée en 2024
    // Version 1: 2020-01 à 2022-01
    client
        .execute(
            "INSERT INTO cadastre.parcelles (id, valid_from, valid_to, geometry_hash) VALUES ($1, $2, $3, $4)",
            &[&"HIST001", &"2020-01-01", &"2022-01-01", &vec![1u8; 32]],
        )
        .await
        .expect("Failed to insert");

    // Version 2: 2022-01 à 2024-01
    client
        .execute(
            "INSERT INTO cadastre.parcelles (id, valid_from, valid_to, geometry_hash) VALUES ($1, $2, $3, $4)",
            &[&"HIST001", &"2022-01-01", &"2024-01-01", &vec![2u8; 32]],
        )
        .await
        .expect("Failed to insert");

    // Recherche en 2021: doit trouver version 1
    let v1 = client
        .query_opt(
            "SELECT geometry_hash FROM cadastre.parcelles
             WHERE id = $1 AND valid_from <= $2::date AND (valid_to IS NULL OR valid_to > $2::date)",
            &[&"HIST001", &"2021-06-15"],
        )
        .await
        .expect("Query failed");

    assert!(v1.is_some());
    let hash: Vec<u8> = v1.unwrap().get(0);
    assert_eq!(hash[0], 1u8);

    // Recherche en 2023: doit trouver version 2
    let v2 = client
        .query_opt(
            "SELECT geometry_hash FROM cadastre.parcelles
             WHERE id = $1 AND valid_from <= $2::date AND (valid_to IS NULL OR valid_to > $2::date)",
            &[&"HIST001", &"2023-06-15"],
        )
        .await
        .expect("Query failed");

    assert!(v2.is_some());
    let hash: Vec<u8> = v2.unwrap().get(0);
    assert_eq!(hash[0], 2u8);

    // Recherche en 2025: ne doit rien trouver
    let v_none = client
        .query_opt(
            "SELECT geometry_hash FROM cadastre.parcelles
             WHERE id = $1 AND valid_from <= $2::date AND (valid_to IS NULL OR valid_to > $2::date)",
            &[&"HIST001", &"2025-06-15"],
        )
        .await
        .expect("Query failed");

    assert!(v_none.is_none());
}

/// Test de transaction et rollback
#[tokio::test]
#[ignore = "Requires PostgreSQL database"]
async fn test_transaction_rollback() {
    let pool = create_test_pool().await.expect("Failed to create pool");
    setup_test_schema(&pool)
        .await
        .expect("Failed to setup schema");

    // Insérer une entité de référence
    {
        let client = pool.get().await.expect("Failed to get client");
        client
            .execute(
                "INSERT INTO cadastre.parcelles (id, valid_from, geometry_hash) VALUES ($1, $2, $3)",
                &[&"REF001", &"2024-01-01", &vec![0u8; 32]],
            )
            .await
            .expect("Failed to insert");
    }

    // Compte avant
    let count_before: i64 = {
        let client = pool.get().await.expect("Failed to get client");
        client
            .query_one("SELECT COUNT(*) FROM cadastre.parcelles", &[])
            .await
            .expect("Count failed")
            .get(0)
    };

    // Transaction avec rollback
    {
        let mut client = pool.get().await.expect("Failed to get client");
        let tx = client.transaction().await.expect("Failed to start tx");

        tx.execute(
            "INSERT INTO cadastre.parcelles (id, valid_from, geometry_hash) VALUES ($1, $2, $3)",
            &[&"ROLLBACK001", &"2025-01-01", &vec![1u8; 32]],
        )
        .await
        .expect("Failed to insert in tx");

        // Rollback explicite (au lieu de commit)
        tx.rollback().await.expect("Rollback failed");
    }

    // Compte après - doit être identique
    let count_after: i64 = {
        let client = pool.get().await.expect("Failed to get client");
        client
            .query_one("SELECT COUNT(*) FROM cadastre.parcelles", &[])
            .await
            .expect("Count failed")
            .get(0)
    };

    assert_eq!(count_before, count_after, "Rollback should restore state");
}

/// Test d'insertion d'entités avec hash
#[tokio::test]
#[ignore = "Requires PostgreSQL database"]
async fn test_entity_upsert() {
    let pool = create_test_pool().await.expect("Failed to create pool");
    setup_test_schema(&pool)
        .await
        .expect("Failed to setup schema");

    let client = pool.get().await.expect("Failed to get client");

    let hash1 = vec![1u8; 32];
    let hash2 = vec![2u8; 32];

    // Première insertion
    client
        .execute(
            "INSERT INTO cadastre.parcelles (id, valid_from, geometry_hash) VALUES ($1, $2, $3)",
            &[&"UPSERT001", &"2024-01-01", &hash1],
        )
        .await
        .expect("Failed to insert");

    // Vérifier que l'entité existe
    let existing = client
        .query_opt(
            "SELECT geometry_hash FROM cadastre.parcelles WHERE id = $1",
            &[&"UPSERT001"],
        )
        .await
        .expect("Query failed");

    assert!(existing.is_some());
    let stored_hash: Vec<u8> = existing.unwrap().get(0);
    assert_eq!(stored_hash, hash1);

    // Mise à jour avec hash différent
    client
        .execute(
            "UPDATE cadastre.parcelles SET geometry_hash = $1, valid_to = NULL WHERE id = $2",
            &[&hash2, &"UPSERT001"],
        )
        .await
        .expect("Failed to update");

    // Vérifier la mise à jour
    let updated = client
        .query_one(
            "SELECT geometry_hash FROM cadastre.parcelles WHERE id = $1",
            &[&"UPSERT001"],
        )
        .await
        .expect("Query failed");

    let updated_hash: Vec<u8> = updated.get(0);
    assert_eq!(updated_hash, hash2);
}
