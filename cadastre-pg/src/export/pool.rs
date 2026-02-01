//! Pool de connexions PostgreSQL

use anyhow::{Context, Result};
use deadpool_postgres::{Config, Pool, PoolConfig, Runtime, Timeouts};
use std::time::Duration;
use tokio_postgres::NoTls;

/// Configuration de la base de données
#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    pub host: String,
    pub port: u16,
    pub dbname: String,
    pub user: String,
    pub password: Option<String>,
    pub pool_size: usize,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            host: "localhost".into(),
            port: 5432,
            dbname: "cadastre".into(),
            user: "postgres".into(),
            password: None,
            pool_size: 16,
        }
    }
}

impl DatabaseConfig {
    /// Charge la configuration depuis les variables d'environnement
    pub fn from_env() -> Self {
        Self {
            host: std::env::var("PGHOST").unwrap_or_else(|_| "localhost".into()),
            port: std::env::var("PGPORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(5432),
            dbname: std::env::var("PGDATABASE").unwrap_or_else(|_| "cadastre".into()),
            user: std::env::var("PGUSER").unwrap_or_else(|_| "postgres".into()),
            password: std::env::var("PGPASSWORD").ok(),
            pool_size: std::env::var("POOL_SIZE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(16),
        }
    }
}

/// Crée un pool de connexions
pub async fn create_pool(config: &DatabaseConfig) -> Result<Pool> {
    let mut cfg = Config::new();
    cfg.host = Some(config.host.clone());
    cfg.port = Some(config.port);
    cfg.dbname = Some(config.dbname.clone());
    cfg.user = Some(config.user.clone());
    cfg.password = config.password.clone();

    cfg.pool = Some(PoolConfig {
        max_size: config.pool_size,
        timeouts: Timeouts {
            wait: Some(Duration::from_secs(30)),
            create: Some(Duration::from_secs(10)),
            recycle: Some(Duration::from_secs(30)),
        },
        ..Default::default()
    });

    cfg.create_pool(Some(Runtime::Tokio1), NoTls)
        .context("Failed to create database pool")
}

/// Teste la connexion à la base
pub async fn test_connection(pool: &Pool) -> Result<()> {
    let client = pool
        .get()
        .await
        .context("Failed to get connection from pool")?;
    client
        .execute("SELECT 1", &[])
        .await
        .context("Connection test failed")?;
    Ok(())
}
