//! # cadastre-pg
//!
//! Export de données cadastrales EDIGEO vers PostGIS ou GeoJSON avec versioning temporel.
//!
//! ## Features
//!
//! - Export vers PostgreSQL/PostGIS avec pool de connexions
//! - Versioning temporel (valid_from/valid_to)
//! - Export GeoJSON standalone
//! - Reprojection légère en Rust pur (sans dépendances externes)
//! - CLI simple
//!
//! ## Usage CLI
//!
//! ```bash
//! # Export EDIGEO vers PostGIS (défaut)
//! cadastre-pg -p ./data.tar.bz2 -d 2024-01
//!
//! # Export vers GeoJSON (sans base de données)
//! cadastre-pg to-geojson -p ./data.tar.bz2 -o ./geojson/
//! ```

pub mod config;
pub mod export;
pub mod reproject_lite;
pub mod report;
pub mod versioning;

pub use config::Config;
pub use export::pool::{create_pool, DatabaseConfig};
pub use reproject_lite::{ReprojectorLite, SmartReprojector};
pub use report::{ImportReport, ImportStatus};
