# edigeo

Bibliothèque Rust pour parser le format EDIGEO (données cadastrales françaises - norme AFNOR NF Z 52000).

## Fonctionnalités

- Parse les archives `.tar.bz2` EDIGEO directement
- Extrait les géométries (parcelles, bâtiments, sections, etc.)
- Supporte tous les types de géométries (Point, LineString, Polygon, Multi*)
- Détection automatique de la projection (EPSG)
- Parsing SIMD optimisé pour les performances
- Réparation automatique des géométries invalides

## Installation

```toml
[dependencies]
edigeo = "0.1"
```

## Usage

```rust
use edigeo::parse;

fn main() -> anyhow::Result<()> {
    // Parser une archive EDIGEO
    let result = parse("path/to/edigeo-archive.tar.bz2")?;

    // Afficher la projection détectée
    println!("Projection: EPSG:{}", result.projection.epsg);
    println!("Département: {}", result.departement);

    // Parcourir les features par type
    for (feature_type, features) in &result.features {
        println!("{}: {} features", feature_type, features.len());

        for feature in features {
            println!("  - ID: {}", feature.id);
            // feature.geometry est un geo::Geometry
            // feature.properties contient les attributs
        }
    }

    Ok(())
}
```

## Types de features supportés

| Type EDIGEO | Description |
|-------------|-------------|
| `COMMUNE_id` | Communes |
| `SECTION_id` | Sections cadastrales |
| `PARCELLE_id` | Parcelles |
| `BATIMENT_id` | Bâtiments |
| `SUBDFISC_id` | Subdivisions fiscales |
| `LIEUDIT_id` | Lieux-dits |
| `NUMVOIE_id` | Numéros de voie |
| `TSURF_id` | Surfaces topographiques |
| `TLINE_id` | Lignes topographiques |
| `TPOINT_id` | Points topographiques |

## Format EDIGEO

Le format EDIGEO (Échange de Données Informatisées dans le domaine de l'information GÉOgraphique) est le format standard français pour les données cadastrales. Il est défini par la norme AFNOR NF Z 52000.

Les données peuvent être téléchargées sur [cadastre.data.gouv.fr](https://cadastre.data.gouv.fr/datasets/plan-cadastral-informatise).

## Licence

MIT
