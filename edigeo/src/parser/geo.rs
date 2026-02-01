//! Parser pour les fichiers GEO (projection)

use memchr::memmem;

use crate::types::Projection;
use crate::EdigeoError;

/// Mapping des projections EDIGEO vers EPSG
const PROJECTIONS: &[(&str, u32)] = &[
    ("LAMB93", 2154),
    ("RGF93CC42", 3942),
    ("RGF93CC43", 3943),
    ("RGF93CC44", 3944),
    ("RGF93CC45", 3945),
    ("RGF93CC46", 3946),
    ("RGF93CC47", 3947),
    ("RGF93CC48", 3948),
    ("RGF93CC49", 3949),
    ("RGF93CC50", 3950),
    ("GUAD48UTM20", 2970),
    ("MART38UTM20", 2973),
    ("RGFG95UTM22", 2972),
    ("RGR92UTM", 2975),
    ("RGM04", 32738),
];

/// Parse un fichier GEO pour extraire la projection
pub fn parse(data: &[u8]) -> Result<Projection, EdigeoError> {
    // Convertir en string pour le parsing
    let content = String::from_utf8_lossy(data);

    // Rechercher le pattern RELSA qui contient le code projection
    let finder = memmem::Finder::new(b"RELSA");

    if let Some(pos) = finder.find(data) {
        let start = pos + 5;
        if let Some(colon_offset) = data[start..].iter().position(|&b| b == b':') {
            let value_start = start + colon_offset + 1;
            let value_end = data[value_start..]
                .iter()
                .position(|&b| b == b'\r' || b == b'\n')
                .map(|p| value_start + p)
                .unwrap_or(data.len());

            let proj_code = std::str::from_utf8(&data[value_start..value_end])
                .unwrap_or("")
                .trim();

            // Chercher dans le mapping
            for &(name, epsg) in PROJECTIONS {
                if proj_code.eq_ignore_ascii_case(name) {
                    return Ok(Projection { epsg, name });
                }
            }

            // Projection non reconnue
            return Err(EdigeoError::UnknownProjection(proj_code.to_string()));
        }
    }

    // Chercher aussi dans les lignes complètes (fallback strict)
    for line in content.lines() {
        // Chercher uniquement les lignes contenant un code projection connu
        for &(name, epsg) in PROJECTIONS {
            // Match strict: le nom doit être un mot complet
            if line.contains(name) && !line.contains(&format!("{}X", name)) {
                return Ok(Projection { epsg, name });
            }
        }
    }

    // AC3: erreur explicite si projection non trouvée
    Err(EdigeoError::UnknownProjection(
        "No projection found in GEO file".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_lamb93() {
        let data = b"RELSACC:LAMB93\r\n";
        let result = parse(data).unwrap();
        assert_eq!(result.epsg, 2154);
        assert_eq!(result.name, "LAMB93");
    }

    #[test]
    fn test_parse_rgf93cc46() {
        let data = b"RELSACC:RGF93CC46\r\n";
        let result = parse(data).unwrap();
        assert_eq!(result.epsg, 3946);
    }

    #[test]
    fn test_parse_unknown_projection_returns_error() {
        let data = b"RELSACC:UNKNOWN_PROJ\r\n";
        let result = parse(data);
        assert!(result.is_err());
        match result {
            Err(EdigeoError::UnknownProjection(msg)) => {
                assert!(msg.contains("UNKNOWN_PROJ"));
            }
            _ => panic!("Expected UnknownProjection error"),
        }
    }

    #[test]
    fn test_parse_empty_geo_returns_error() {
        let data = b"Some random content without RELSA\r\n";
        let result = parse(data);
        assert!(result.is_err());
    }
}
