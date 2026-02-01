//! Tests d'intégration avec de vraies archives EDIGEO

use std::path::Path;

#[test]
fn test_parse_real_archive() {
    let fixture_path = Path::new("../../fixtures/39001/edigeo-39001000AB01.tar.bz2");

    if !fixture_path.exists() {
        eprintln!("Fixture not found, skipping test");
        return;
    }

    let result = edigeo::parse(fixture_path);

    match result {
        Ok(parse_result) => {
            println!("Year: {}", parse_result.year);
            println!("Projection EPSG: {}", parse_result.projection.epsg);
            println!("Feature types:");
            for (feature_type, features) in &parse_result.features {
                println!("  {}: {} features", feature_type, features.len());
            }

            // Vérifications
            assert!(parse_result.year >= 2020, "Year should be recent");
            assert_eq!(parse_result.projection.epsg, 2154, "Should be Lambert 93");
            assert!(
                !parse_result.features.is_empty(),
                "Should have at least one feature type"
            );

            // Vérifier qu'on a des parcelles ou des communes
            let has_parcelles = parse_result.features.contains_key("PARCELLE_id");
            let has_communes = parse_result.features.contains_key("COMMUNE_id");
            assert!(
                has_parcelles || has_communes,
                "Should have PARCELLE_id or COMMUNE_id features"
            );

            // Vérifier que les features ont des géométries valides
            for (_, features) in &parse_result.features {
                for feature in features {
                    assert!(!feature.id.is_empty(), "Feature should have an ID");
                    // La géométrie est toujours présente (c'est un champ non optionnel)
                }
            }

            if !parse_result.errors.is_empty() {
                println!("Non-fatal errors:");
                for err in &parse_result.errors {
                    println!("  {:?}", err);
                }
            }
        }
        Err(e) => {
            panic!("Failed to parse archive: {:?}", e);
        }
    }
}

#[test]
fn test_parse_multiple_archives() {
    let fixtures_dir = Path::new("../../fixtures/39001");

    if !fixtures_dir.exists() {
        eprintln!("Fixtures directory not found, skipping test");
        return;
    }

    let archives: Vec<_> = std::fs::read_dir(fixtures_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "bz2"))
        .collect();

    let mut total_features = 0;
    let mut success_count = 0;

    for entry in &archives {
        let path = entry.path();
        match edigeo::parse(&path) {
            Ok(result) => {
                success_count += 1;
                let count: usize = result.features.values().map(|v| v.len()).sum();
                total_features += count;
                println!(
                    "{}: {} features",
                    path.file_name().unwrap().to_string_lossy(),
                    count
                );
            }
            Err(e) => {
                eprintln!("Failed to parse {}: {:?}", path.display(), e);
            }
        }
    }

    println!(
        "\nParsed {}/{} archives, {} total features",
        success_count,
        archives.len(),
        total_features
    );

    assert!(success_count > 0, "Should parse at least one archive");
}
