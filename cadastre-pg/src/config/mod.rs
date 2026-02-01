//! Configuration du système

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};

/// Configuration principale
#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    #[serde(flatten)]
    pub tables: HashMap<String, TableConfig>,
}

/// Configuration d'une table
#[derive(Debug, Deserialize, Serialize)]
pub struct TableConfig {
    /// Nom de la table PostgreSQL cible
    pub table: String,

    /// Mapping des champs EDIGEO vers colonnes SQL
    pub fields: Vec<FieldMapping>,

    /// Calculer le hash de géométrie pour cette table
    #[serde(default)]
    pub hash_geom: bool,
}

/// Mapping d'un champ
#[derive(Debug, Deserialize, Serialize)]
pub struct FieldMapping {
    /// Nom du champ EDIGEO source
    pub source: String,

    /// Nom de la colonne SQL cible
    pub target: String,

    /// Type de données (text, integer, etc.)
    #[serde(default = "default_type")]
    pub data_type: String,

    /// Préfixer la valeur avec le code département (comme addDep de Node.js)
    #[serde(default)]
    pub prefix_dep: bool,
}

fn default_type() -> String {
    "text".to_string()
}

impl Config {
    /// Charge une configuration depuis un fichier
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .context(format!("Failed to read config file: {}", path.display()))?;

        serde_json::from_str(&content).context("Failed to parse config JSON")
    }

    /// Charge une configuration depuis un preset embarqué
    pub fn from_preset(preset: &str) -> Result<Self> {
        match preset {
            "full" => Self::load_embedded(include_str!("presets/full.json")),
            "light" => Self::load_embedded(include_str!("presets/light.json")),
            "bati" => Self::load_embedded(include_str!("presets/bati.json")),
            _ => anyhow::bail!("Unknown preset: {}. Use: full, light, bati", preset),
        }
    }

    fn load_embedded(json: &str) -> Result<Self> {
        serde_json::from_str(json).context("Failed to parse embedded config")
    }

    /// Récupère la configuration d'une table
    pub fn get_table_config(&self, feature_type: &str) -> Option<&TableConfig> {
        self.tables.get(feature_type)
    }
}
