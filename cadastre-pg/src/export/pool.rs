//! Pool de connexions PostgreSQL

use anyhow::{Context, Result};
use deadpool_postgres::{Config, Pool, PoolConfig, Runtime, Timeouts};
use std::time::Duration;
use tokio_postgres::NoTls;
use tokio_postgres_rustls::MakeRustlsConnect;

/// Mode SSL pour la connexion PostgreSQL
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SslMode {
    /// Pas de SSL (défaut)
    #[default]
    Disable,
    /// SSL préféré mais non requis
    Prefer,
    /// SSL requis
    Require,
}

impl std::str::FromStr for SslMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "disable" | "off" | "false" | "no" => Ok(SslMode::Disable),
            "prefer" => Ok(SslMode::Prefer),
            "require" | "on" | "true" | "yes" => Ok(SslMode::Require),
            _ => Err(format!("Invalid SSL mode: {}. Use: disable, prefer, require", s)),
        }
    }
}

/// Configuration de la base de données
#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    pub host: String,
    pub port: u16,
    pub dbname: String,
    pub user: String,
    pub password: Option<String>,
    pub pool_size: usize,
    pub ssl_mode: SslMode,
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
            ssl_mode: SslMode::Disable,
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
            ssl_mode: std::env::var("PGSSLMODE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or_default(),
        }
    }
}

/// Crée la configuration TLS pour rustls
fn make_tls_connector() -> Result<MakeRustlsConnect> {
    let root_store = rustls::RootCertStore::from_iter(
        webpki_roots::TLS_SERVER_ROOTS.iter().cloned()
    );

    let config = rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();

    Ok(MakeRustlsConnect::new(config))
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

    match config.ssl_mode {
        SslMode::Disable => {
            cfg.create_pool(Some(Runtime::Tokio1), NoTls)
                .context("Failed to create database pool")
        }
        SslMode::Prefer | SslMode::Require => {
            let tls = make_tls_connector()?;
            cfg.create_pool(Some(Runtime::Tokio1), tls)
                .context("Failed to create database pool with TLS")
        }
    }
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
