//! Export vers GeoJSON avec geozero (streaming, zero-copy)

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use anyhow::{Context, Result};
use geozero::geojson::GeoJsonWriter;
use geozero::GeozeroGeometry;

use edigeo::{Feature, Projection};

/// Exporte des features en GeoJSON (streaming avec geozero)
pub fn export_to_geojson(
    features: &[Feature],
    projection: &Projection,
    output_path: &Path,
) -> Result<()> {
    let file = File::create(output_path)
        .context(format!("Failed to create file: {}", output_path.display()))?;
    let mut writer = BufWriter::new(file);

    // Header FeatureCollection avec CRS
    write!(
        writer,
        r#"{{"type":"FeatureCollection","crs":{{"type":"name","properties":{{"name":"urn:ogc:def:crs:EPSG::{}"}}}},"features":["#,
        projection.epsg
    )?;

    // Écrire chaque feature
    for (i, feature) in features.iter().enumerate() {
        if i > 0 {
            write!(writer, ",")?;
        }
        write_feature(&mut writer, feature)?;
    }

    // Footer
    write!(writer, "]}}")?;
    writer.flush()?;

    Ok(())
}

/// Écrit une feature en GeoJSON
fn write_feature<W: Write>(writer: &mut W, feature: &Feature) -> Result<()> {
    // Start feature
    write!(
        writer,
        r#"{{"type":"Feature","id":"{}","#,
        escape_json(&feature.id)
    )?;

    // Geometry via geozero (efficace, zero-copy)
    write!(writer, r#""geometry":"#)?;
    let mut geom_buf = Vec::new();
    let mut geom_writer = GeoJsonWriter::new(&mut geom_buf);
    feature.geometry.process_geom(&mut geom_writer)?;
    writer.write_all(&geom_buf)?;

    // Properties
    write!(
        writer,
        r#","properties":{{"_id":"{}""#,
        escape_json(&feature.id)
    )?;
    for (key, value) in &feature.properties {
        write!(
            writer,
            r#","{}":"{}""#,
            escape_json(key),
            escape_json(value)
        )?;
    }
    write!(writer, "}}}}")?;

    Ok(())
}

/// Échappe une chaîne pour JSON
fn escape_json(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            c if c.is_control() => {
                result.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => result.push(c),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo::{Geometry, Point};
    use std::collections::HashMap;
    use std::io::Cursor;

    #[test]
    fn test_write_feature() {
        let feature = Feature {
            id: "test_123".to_string(),
            geometry: Geometry::Point(Point::new(1.0, 2.0)),
            properties: HashMap::new(),
            feature_type: "TEST".to_string(),
        };

        let mut buffer = Cursor::new(Vec::new());
        write_feature(&mut buffer, &feature).unwrap();

        let json = String::from_utf8(buffer.into_inner()).unwrap();
        assert!(json.contains(r#""id":"test_123""#));
        assert!(json.contains(r#""type":"Feature""#));
        // geozero outputs "Point" directly
        assert!(json.contains("Point") || json.contains("coordinates"));
    }

    #[test]
    fn test_escape_json() {
        assert_eq!(escape_json("hello"), "hello");
        assert_eq!(escape_json("hello\"world"), "hello\\\"world");
        assert_eq!(escape_json("line\nbreak"), "line\\nbreak");
    }

    #[test]
    fn test_export_to_geojson() {
        let features = vec![Feature {
            id: "001".to_string(),
            geometry: Geometry::Point(Point::new(5.0, 47.0)),
            properties: [("name".to_string(), "Test".to_string())]
                .into_iter()
                .collect(),
            feature_type: "TEST".to_string(),
        }];
        let projection = Projection {
            epsg: 4326,
            name: "",
        };

        let temp_dir = std::env::temp_dir();
        let output_path = temp_dir.join("test_geozero.geojson");

        export_to_geojson(&features, &projection, &output_path).unwrap();

        let content = std::fs::read_to_string(&output_path).unwrap();
        assert!(content.contains(r#""type":"FeatureCollection""#));
        assert!(content.contains("EPSG::4326"));
        assert!(content.contains(r#""id":"001""#));

        std::fs::remove_file(output_path).ok();
    }
}
