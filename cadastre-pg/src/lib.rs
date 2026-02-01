//! # cadastre-pg
//!
//! Import de données cadastrales EDIGEO vers PostGIS avec versioning temporel.
//!
//! ## Features
//!
//! - Import dans PostgreSQL/PostGIS avec pool de connexions
//! - Versioning temporel (valid_from/valid_to)
//! - Export GeoJSON standalone
//! - CLI simple
//!
//! ## Usage CLI
//!
//! ```bash
//! # Import EDIGEO vers PostGIS
//! cadastre-pg import --path ./data.tar.bz2 --date 2024-01
//! cadastre-pg import --path ./folder/ --date 2024-04
//!
//! # Export GeoJSON (sans base de données)
//! cadastre-pg export --path ./data.tar.bz2 --output ./geojson/
//! ```

pub mod config;
pub mod export;
pub mod report;
pub mod versioning;

pub use config::Config;
pub use export::pool::{create_pool, DatabaseConfig};
pub use report::{ImportReport, ImportStatus};
