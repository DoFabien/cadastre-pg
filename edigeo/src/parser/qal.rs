//! Parser pour les fichiers QAL (qualité)

use std::collections::HashMap;

use crate::types::Quality;
use crate::EdigeoError;

/// Parse un fichier QAL pour extraire les informations de qualité
pub fn parse(data: &[u8]) -> Result<HashMap<String, Quality>, EdigeoError> {
    if data.is_empty() {
        return Ok(HashMap::new());
    }

    let content = String::from_utf8_lossy(data);
    let mut qualities = HashMap::new();

    // Splitter par blocs RTYSA03:QUP
    let blocks: Vec<&str> = content.split("RTYSA03:").collect();

    for block in blocks.iter().skip(1) {
        // Vérifier que c'est un bloc QUP (actualité)
        if !block.starts_with("QUP") {
            continue;
        }

        let lines: Vec<&str> = block.lines().collect();
        if lines.len() < 2 {
            continue;
        }

        // Extraire l'ID de l'objet (ligne 2: RIDSA:xxx)
        let id = lines
            .get(1)
            .and_then(|line| line.split(':').nth(1))
            .map(|s| s.trim().to_string());

        let Some(object_id) = id else {
            continue;
        };

        let mut quality = Quality::default();

        for line in &lines[2..] {
            if line.is_empty() {
                continue;
            }

            let parts: Vec<&str> = line.splitn(2, ':').collect();
            if parts.len() != 2 {
                continue;
            }

            let (key, value) = (parts[0], parts[1].trim());

            // Parser les différents champs
            if key.starts_with("ODA") {
                quality.create_date = Some(value.to_string());
            } else if key.starts_with("UDA") {
                quality.update_date = Some(value.to_string());
            } else if key.starts_with("UTY") {
                quality.update_type = Some(value.to_string());
            }
        }

        qualities.insert(object_id, quality);
    }

    Ok(qualities)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_qal() {
        let data = b"RTYSA03:QUP\r\nRIDSA:Objet_123\r\nODASA:20260115\r\nUDASA:20260120\r\n";
        let result = parse(data).unwrap();

        assert!(result.contains_key("Objet_123"));
        let quality = result.get("Objet_123").unwrap();
        assert_eq!(quality.create_date.as_deref(), Some("20260115"));
        assert_eq!(quality.update_date.as_deref(), Some("20260120"));
    }
}
