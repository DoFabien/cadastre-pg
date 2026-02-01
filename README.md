# cadastre-pg

Outil Rust performant pour importer le cadastre EDIGEO vers PostGIS avec support du versioning temporel.

> **Note** : Une version Node.js est disponible sur la branche [`js`](../../tree/js). La version Rust est **3.5x plus rapide** et consomme beaucoup moins de mémoire. La décompression des archives `.tar.bz2` représente ~50% du temps total de traitement.

## Fonctionnalités

- Import EDIGEO → PostGIS avec reprojection à la volée
- **Versioning temporel** : champs `valid_from` / `valid_to` pour suivre l'historique
- **Import incrémental** : skip des archives inchangées (checksum blake3)
- **Déduplication** : skip des features identiques (hash géométrie normalisé)
- Configuration flexible via presets (`full` / `light` / `bati`) ou fichier JSON
- Parallélisation multi-thread avec rayon/tokio

## Installation

### Téléchargement

Des binaires précompilés sont disponibles dans les [Releases](../../releases) pour Linux, macOS et Windows.

> **Note** : Le binaire Windows n'inclut pas la reprojection (PROJ difficile à builder). Linux et macOS ARM ont la reprojection. Pour Windows avec reprojection, compilez depuis les sources avec PROJ installé.

### Compilation depuis les sources

```sh
# Sans reprojection
cargo build --release

# Avec reprojection (nécessite libproj)
cargo build --release --features reproject
```

Le binaire sera disponible dans `target/release/cadastre-pg`.

## Usage

```sh
cadastre-pg import [OPTIONS] --path <PATH> --date <YYYY-MM>
```

### Options

| Option | Description | Défaut |
|--------|-------------|--------|
| `--path`, `-p` | Chemin du répertoire ou archive `.tar.bz2` | **requis** |
| `--date`, `-d` | Date du millésime (format `YYYY-MM`) | **requis** |
| `--schema` | Schéma PostgreSQL cible | `cadastre` |
| `--config` | Preset (`full`/`light`/`bati`) ou chemin JSON | `full` |
| `--srid` | SRID cible | `4326` |
| `--precision` | Précision des coordonnées (décimales) | `7` (4326) / `2` (métrique) |
| `--dep` | Code département (`38`, `2A`) ou `fromFile` | auto |
| `--jobs` | Nombre de threads | max CPU |
| `--drop-schema` | Supprimer le schéma avant import | `false` |
| `--drop-table` | Supprimer les tables avant import | `false` |
| `--skip-indexes` | Ne pas créer les index | `false` |
| `--host` | Hôte PostgreSQL | `$PGHOST` / `localhost` |
| `--database` | Base de données | `$PGDATABASE` / `cadastre` |
| `--user` | Utilisateur | `$PGUSER` / `postgres` |
| `--password` | Mot de passe | `$PGPASSWORD` |
| `--port` | Port | `$PGPORT` / `5432` |
| `--ssl` | Mode SSL : `disable`, `prefer`, `require` | `$PGSSLMODE` / `disable` |

### Exemple

```sh
# Premier import (crée le schéma)
cadastre-pg import \
  --path /data/edigeo/cadastre-dep38-2025-04 \
  --date 2025-04 \
  --schema cadastre \
  --drop-schema

# Import incrémental (trimestre suivant)
cadastre-pg import \
  --path /data/edigeo/cadastre-dep38-2025-09 \
  --date 2025-09 \
  --schema cadastre
```

## Configuration

Les presets sont embarqués dans le binaire :

- **`full`** : toutes les couches (parcelles, sections, communes, bâtiments, subdivisions fiscales, etc.)
- **`light`** : parcelles, sections, communes uniquement
- **`bati`** : bâtiments uniquement

Pour une configuration personnalisée, créer un fichier JSON :

```json
{
  "PARCELLE_id": {
    "table": "parcelles",
    "hash_geom": true,
    "fields": [
      { "source": "IDU", "target": "id", "prefix_dep": true },
      { "source": "TEX", "target": "numero" },
      { "source": "SUPF", "target": "contenance", "data_type": "integer" }
    ]
  }
}
```

## Import incrémental

L'outil optimise les imports successifs :

1. **Skip par checksum d'archive** : si une archive `.tar.bz2` a le même checksum qu'un import précédent, elle est ignorée (pas de décompression ni parsing)

2. **Skip par hash de géométrie** : les features dont la géométrie existe déjà en base sont ignorées

### Exemple de performance

| Scénario | Archives traitées | Temps |
|----------|-------------------|-------|
| Import initial (553 archives) | 553 | ~9s |
| Import incrémental (~24% changé) | ~130 | ~3.7s |
| Re-import identique | 0 | **77ms** |

## Structure des tables

Chaque table créée contient :

- `row_id` : identifiant unique auto-incrémenté
- `id` : identifiant EDIGEO (préfixé du département)
- `departement` : code département
- `geometry` : géométrie PostGIS
- `valid_from` : date de début de validité
- `valid_to` : date de fin de validité (NULL si actif)
- `geometry_hash` : hash blake3 de la géométrie (si `hash_geom: true`)
- Colonnes métier selon la configuration

## Variables d'environnement

La connexion PostgreSQL peut être configurée via :

- `PGHOST`, `PGDATABASE`, `PGUSER`, `PGPASSWORD`, `PGPORT`, `PGSSLMODE`

Ou via un fichier `.env` à la racine du projet.

## Export GeoJSON

Pour exporter sans base de données :

```sh
cadastre-pg export --path /data/archive.tar.bz2 --output /data/geojson/
```

## Licence

MIT
