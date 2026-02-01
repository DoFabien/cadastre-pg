//! Parser pour les fichiers THF (métadonnées)

use encoding_rs::Encoding;
use memchr::memmem;

use crate::types::ThfData;
use crate::EdigeoError;

/// Parse un fichier THF pour extraire l'encodage et l'année
pub fn parse(data: &[u8]) -> Result<ThfData, EdigeoError> {
    let encoding = parse_encoding(data)?;
    let year = parse_year(data)?;

    Ok(ThfData { encoding, year })
}

/// Extrait l'encodage depuis le champ CSET
fn parse_encoding(data: &[u8]) -> Result<&'static Encoding, EdigeoError> {
    // Recherche SIMD du pattern "CSET"
    let finder = memmem::Finder::new(b"CSET");

    if let Some(pos) = finder.find(data) {
        // Trouver la valeur après ":"
        let start = pos + 4; // Après "CSET"
        if let Some(colon_offset) = data[start..].iter().position(|&b| b == b':') {
            let value_start = start + colon_offset + 1;
            // Trouver la fin de ligne
            let value_end = data[value_start..]
                .iter()
                .position(|&b| b == b'\r' || b == b'\n')
                .map(|p| value_start + p)
                .unwrap_or(data.len());

            let cset = std::str::from_utf8(&data[value_start..value_end])
                .unwrap_or("")
                .trim();

            return Ok(cset_to_encoding(cset));
        }
    }

    // Par défaut: ISO-8859-1
    Ok(encoding_rs::ISO_8859_15)
}

/// Mappe les codes CSET EDIGEO vers les encodages
fn cset_to_encoding(cset: &str) -> &'static Encoding {
    match cset.to_uppercase().as_str() {
        "IRV" | "646-FRANCE" | "8859-1" => encoding_rs::ISO_8859_15, // EDIGEO français utilise Latin-9
        "8859-2" => encoding_rs::ISO_8859_2,
        "8859-3" => encoding_rs::ISO_8859_3,
        "8859-4" => encoding_rs::ISO_8859_4,
        "8859-5" => encoding_rs::ISO_8859_5,
        "8859-6" => encoding_rs::ISO_8859_6,
        "8859-7" => encoding_rs::ISO_8859_7,
        "8859-8" => encoding_rs::ISO_8859_8,
        "8859-9" => encoding_rs::WINDOWS_1254, // Turkish (ISO-8859-9 compatible)
        "8859-15" => encoding_rs::ISO_8859_15, // Latin-9 explicite
        _ => encoding_rs::ISO_8859_15,         // Par défaut: Latin-9 (français)
    }
}

/// Extrait l'année depuis le champ TDASD
fn parse_year(data: &[u8]) -> Result<u16, EdigeoError> {
    // Recherche SIMD du pattern "TDASD"
    let finder = memmem::Finder::new(b"TDASD");

    if let Some(pos) = finder.find(data) {
        let start = pos + 5; // Après "TDASD"
        if let Some(colon_offset) = data[start..].iter().position(|&b| b == b':') {
            let value_start = start + colon_offset + 1;

            // Les 4 premiers caractères sont l'année
            if value_start + 4 <= data.len() {
                let year_str =
                    std::str::from_utf8(&data[value_start..value_start + 4]).unwrap_or("2020");

                if let Ok(year) = year_str.parse::<u16>() {
                    return Ok(year);
                }
            }
        }
    }

    // Par défaut: année courante
    Ok(2020)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_encoding_iso8859_1() {
        // Format avec suffixe "CC" (réel dans les fichiers EDIGEO)
        let data = b"CSETCC:8859-1\r\n";
        let result = parse_encoding(data).unwrap();
        assert_eq!(result.name(), "ISO-8859-15");
    }

    #[test]
    fn test_parse_encoding_simple_format() {
        // Format simplifié documenté
        let data = b"CSET:8859-1\r\n";
        let result = parse_encoding(data).unwrap();
        assert_eq!(result.name(), "ISO-8859-15");
    }

    #[test]
    fn test_parse_encoding_irv() {
        let data = b"CSETCC:IRV\r\n";
        let result = parse_encoding(data).unwrap();
        assert_eq!(result.name(), "ISO-8859-15");
    }

    #[test]
    fn test_parse_encoding_8859_15_explicit() {
        let data = b"CSETCC:8859-15\r\n";
        let result = parse_encoding(data).unwrap();
        assert_eq!(result.name(), "ISO-8859-15");
    }

    #[test]
    fn test_parse_year() {
        let data = b"TDASDP:20260115\r\n";
        let result = parse_year(data).unwrap();
        assert_eq!(result, 2026);
    }

    #[test]
    fn test_parse_default_encoding_when_missing() {
        let data = b"OTHER:value\r\n";
        let result = parse_encoding(data).unwrap();
        // Fallback: ISO-8859-15 (Latin-9 français)
        assert_eq!(result.name(), "ISO-8859-15");
    }
}
