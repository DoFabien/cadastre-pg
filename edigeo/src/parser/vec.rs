//! Parser pour les fichiers VEC (données vectorielles)

use std::collections::HashMap;

use memchr::memmem;

use crate::EdigeoError;

/// Référence vers une autre entité
#[derive(Debug, Clone, Default)]
pub struct Reference {
    pub sid: String,
    pub gid: String,
    pub rty: String,
    pub rid: String,
}

/// Point (noeud)
#[derive(Debug, Clone)]
pub struct Point {
    pub id: String,
    pub coords: Vec<(f64, f64)>,
    pub scp: Option<Reference>,
}

/// Arc (segment de ligne)
#[derive(Debug, Clone)]
pub struct Arc {
    pub id: String,
    pub coords: Vec<(f64, f64)>,
    pub scp: Option<Reference>,
}

/// Face (surface)
#[derive(Debug, Clone)]
pub struct Face {
    pub id: String,
    pub scp: Option<Reference>,
    pub arcs: Vec<Arc>,
}

/// Feature (objet métier)
#[derive(Debug, Clone)]
pub struct Feature {
    pub id: String,
    pub scp: Option<Reference>,
    pub attributes: HashMap<String, String>,
    pub qap: Option<String>,
}

/// Link (relation)
#[derive(Debug, Clone)]
pub struct Link {
    pub id: String,
    pub scp: Option<Reference>,
    pub ftp: Vec<Reference>,
}

/// Résultat du parsing d'un fichier VEC
#[derive(Debug, Default)]
pub struct ParsedVec {
    pub pno: HashMap<String, Point>,
    pub par: HashMap<String, Arc>,
    pub pfe: HashMap<String, Face>,
    pub fea: HashMap<String, Feature>,
    pub lnk: HashMap<String, Link>,
}

/// Parse un fichier VEC décodé
pub fn parse(content: &str) -> Result<ParsedVec, EdigeoError> {
    let mut result = ParsedVec::default();

    // Itérer sur les blocs RTYSA03: sans collecter (lazy)
    for block in content.split("RTYSA03:").skip(1) {
        // Parser le bloc directement avec un itérateur de lignes
        let mut lines_iter = block.lines();

        let Some(first_line) = lines_iter.next() else {
            continue;
        };
        let block_type = first_line.trim();

        // L'ID est sur la ligne 2, format RIDSA..:id
        let id = extract_id_from_iter(lines_iter.clone());
        if id.is_empty() {
            continue;
        }

        // Collecter les lignes seulement pour le parsing du bloc
        // (nécessaire car les parsers ont besoin d'un accès aléatoire)
        let lines: Vec<&str> = std::iter::once(first_line).chain(lines_iter).collect();

        match block_type {
            "PNO" => {
                if let Ok(pno) = parse_pno(&lines, &id) {
                    result.pno.insert(id, pno);
                }
            }
            "PAR" => {
                if let Ok(par) = parse_par(&lines, &id) {
                    result.par.insert(id, par);
                }
            }
            "PFE" => {
                if let Ok(pfe) = parse_pfe(&lines, &id) {
                    result.pfe.insert(id, pfe);
                }
            }
            "FEA" => {
                if let Ok(fea) = parse_fea(&lines, &id) {
                    result.fea.insert(id, fea);
                }
            }
            "LNK" => {
                if let Ok(lnk) = parse_lnk(&lines, &id) {
                    result.lnk.insert(id, lnk);
                }
            }
            _ => {}
        }
    }

    // Associer les arcs aux faces via les LNK
    associate_arcs_to_faces(&mut result);

    Ok(result)
}

/// Extrait l'ID d'un bloc (format RIDSA..:id)
fn extract_id(lines: &[&str]) -> String {
    for line in lines.iter().skip(1) {
        if line.starts_with("RIDSA") || line.starts_with("RID") {
            if let Some(pos) = line.find(':') {
                return line[pos + 1..].trim().to_string();
            }
        }
    }
    String::new()
}

/// Extrait l'ID depuis un itérateur de lignes (version optimisée)
fn extract_id_from_iter<'a>(lines: impl Iterator<Item = &'a str>) -> String {
    for line in lines {
        if line.starts_with("RIDSA") || line.starts_with("RID") {
            if let Some(pos) = line.find(':') {
                return line[pos + 1..].trim().to_string();
            }
        }
    }
    String::new()
}

/// Parse une référence (SCP, FTP, QAP, etc.)
/// Format: SID;GID;RTY;RID
/// Optimisé pour éviter les allocations du Vec
#[inline]
fn parse_reference(value: &str) -> Reference {
    let mut parts = value.splitn(5, ';'); // Max 4 parties + reste
    Reference {
        sid: parts.next().unwrap_or("").to_string(),
        gid: parts.next().unwrap_or("").to_string(),
        rty: parts.next().unwrap_or("").to_string(),
        rid: parts.next().unwrap_or("").to_string(),
    }
}

/// Parse des coordonnées au format EDIGEO: +X;+Y; ou +X;+Y
/// Optimisé pour éviter les allocations
#[inline]
fn parse_coords(value: &str) -> Option<(f64, f64)> {
    // Format: +881824.53;+6663821.17; (avec trailing ;)
    // Utiliser find au lieu de split().collect()
    let semicolon_pos = value.find(';')?;

    let x_str = value[..semicolon_pos].trim().trim_start_matches('+');
    let rest = &value[semicolon_pos + 1..];

    // Trouver la fin de Y (soit le prochain ; soit la fin)
    let y_end = rest.find(';').unwrap_or(rest.len());
    let y_str = rest[..y_end].trim().trim_start_matches('+');

    let x = fast_parse_f64(x_str)?;
    let y = fast_parse_f64(y_str)?;
    Some((x, y))
}

/// Parse f64 optimisé pour les coordonnées EDIGEO (format simple: digits.digits)
/// Utilise fast-float pour un parsing 4-10x plus rapide que std::parse
#[inline]
fn fast_parse_f64(s: &str) -> Option<f64> {
    fast_float::parse(s).ok()
}

/// Extrait la clé d'une ligne (premiers caractères jusqu'à ce qu'on trouve un modèle)
fn extract_key(line: &str) -> &str {
    // Les clés sont des patterns comme SCPCP, CORCC, ATPCP, ATVST, etc.
    // On veut extraire juste les 3 premiers caractères pour le matching
    if line.len() >= 3 {
        &line[..3]
    } else {
        line
    }
}

/// Extrait la valeur après ':'
fn extract_value(line: &str) -> Option<&str> {
    line.find(':').map(|pos| line[pos + 1..].trim())
}

/// Parse un bloc PNO (point)
fn parse_pno(lines: &[&str], id: &str) -> Result<Point, EdigeoError> {
    let mut pno = Point {
        id: id.to_string(),
        coords: Vec::new(),
        scp: None,
    };

    for line in lines.iter().skip(1) {
        if line.is_empty() {
            continue;
        }

        let key = extract_key(line);
        let Some(value) = extract_value(line) else {
            continue;
        };

        match key {
            "SCP" => pno.scp = Some(parse_reference(value)),
            "COR" => {
                if let Some(coord) = parse_coords(value) {
                    pno.coords.push(coord);
                }
            }
            _ => {}
        }
    }

    Ok(pno)
}

/// Parse un bloc PAR (arc)
fn parse_par(lines: &[&str], id: &str) -> Result<Arc, EdigeoError> {
    let mut par = Arc {
        id: id.to_string(),
        coords: Vec::new(),
        scp: None,
    };

    for line in lines.iter().skip(1) {
        if line.is_empty() {
            continue;
        }

        let key = extract_key(line);
        let Some(value) = extract_value(line) else {
            continue;
        };

        match key {
            "SCP" => par.scp = Some(parse_reference(value)),
            "COR" => {
                if let Some(coord) = parse_coords(value) {
                    par.coords.push(coord);
                }
            }
            _ => {}
        }
    }

    Ok(par)
}

/// Parse un bloc PFE (face)
fn parse_pfe(lines: &[&str], id: &str) -> Result<Face, EdigeoError> {
    let mut pfe = Face {
        id: id.to_string(),
        scp: None,
        arcs: Vec::new(),
    };

    for line in lines.iter().skip(1) {
        if line.is_empty() {
            continue;
        }

        let key = extract_key(line);
        let Some(value) = extract_value(line) else {
            continue;
        };

        if key == "SCP" {
            pfe.scp = Some(parse_reference(value));
        }
    }

    Ok(pfe)
}

/// Parse un bloc FEA (feature)
fn parse_fea(lines: &[&str], id: &str) -> Result<Feature, EdigeoError> {
    let mut fea = Feature {
        id: id.to_string(),
        scp: None,
        attributes: HashMap::new(),
        qap: None,
    };

    let mut current_attr_key: Option<String> = None;

    for line in lines.iter().skip(1) {
        if line.is_empty() {
            continue;
        }

        let key = extract_key(line);
        let Some(value) = extract_value(line) else {
            continue;
        };

        match key {
            "SCP" => fea.scp = Some(parse_reference(value)),
            "ATP" => {
                // ATPCP - référence d'attribut
                let reference = parse_reference(value);
                // Extraire le nom de l'attribut depuis RID (ex: TEX2_id -> TEX2, IDU_id -> IDU)
                current_attr_key = Some(reference.rid.trim_end_matches("_id").to_string());
            }
            "TEX" => {
                // TEXT - indication d'encodage, on l'ignore mais on garde current_attr_key
                // La valeur viendra dans le prochain ATV
            }
            "ATV" => {
                // ATVST, ATVSA, ATVSR - valeur d'attribut
                if let Some(attr_key) = current_attr_key.take() {
                    fea.attributes.insert(attr_key, value.to_string());
                }
            }
            "QAP" => {
                // QAPCP - référence qualité
                let reference = parse_reference(value);
                fea.qap = Some(reference.rid);
            }
            _ => {}
        }
    }

    Ok(fea)
}

/// Parse un bloc LNK (link)
fn parse_lnk(lines: &[&str], id: &str) -> Result<Link, EdigeoError> {
    let mut lnk = Link {
        id: id.to_string(),
        scp: None,
        ftp: Vec::new(),
    };

    for line in lines.iter().skip(1) {
        if line.is_empty() {
            continue;
        }

        let key = extract_key(line);
        let Some(value) = extract_value(line) else {
            continue;
        };

        match key {
            "SCP" => lnk.scp = Some(parse_reference(value)),
            "FTP" => lnk.ftp.push(parse_reference(value)),
            _ => {}
        }
    }

    Ok(lnk)
}

/// Associe les arcs aux faces via les relations LNK
fn associate_arcs_to_faces(result: &mut ParsedVec) {
    // Collecter les associations arc->face
    let mut associations: Vec<(String, String)> = Vec::new();

    for lnk in result.lnk.values() {
        if let Some(ref scp) = lnk.scp {
            // Les relations de composition face-arc
            if scp.rid.contains("RCO_FAC") {
                let arc_ref = lnk.ftp.iter().find(|r| r.rty == "PAR");
                let face_ref = lnk.ftp.iter().find(|r| r.rty == "PFE");

                if let (Some(arc_ref), Some(face_ref)) = (arc_ref, face_ref) {
                    associations.push((face_ref.rid.clone(), arc_ref.rid.clone()));
                }
            }
        }
    }

    // Appliquer les associations
    for (face_id, arc_id) in associations {
        if let Some(arc) = result.par.get(&arc_id) {
            if let Some(face) = result.pfe.get_mut(&face_id) {
                face.arcs.push(arc.clone());
            }
        }
    }
}

/// Parse rapide avec SIMD pour trouver le type d'une feature
pub fn find_feature_type(content: &[u8], feature_id: &str) -> Option<String> {
    let finder = memmem::Finder::new(feature_id.as_bytes());

    if let Some(pos) = finder.find(content) {
        // Chercher SCPCP après cette position
        let search_start = pos;
        let search_end = (pos + 500).min(content.len());
        let slice = &content[search_start..search_end];

        if let Some(scp_pos) = memmem::find(slice, b"SCPCP") {
            let line_start = scp_pos;
            let line_end = slice[line_start..]
                .iter()
                .position(|&b| b == b'\r' || b == b'\n')
                .map(|p| line_start + p)
                .unwrap_or(slice.len());

            if let Ok(line) = std::str::from_utf8(&slice[line_start..line_end]) {
                if let Some(colon_pos) = line.find(':') {
                    let value = &line[colon_pos + 1..];
                    let parts: Vec<&str> = value.split(';').collect();
                    if let Some(rid) = parts.get(3) {
                        // Conserver le type tel quel (ex: PARCELLE_id) pour compat config
                        return Some((*rid).to_string());
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_reference() {
        let reference = parse_reference("EDAB01;SeSD;PGE;Noeud_123");
        assert_eq!(reference.sid, "EDAB01");
        assert_eq!(reference.gid, "SeSD");
        assert_eq!(reference.rty, "PGE");
        assert_eq!(reference.rid, "Noeud_123");
    }

    #[test]
    fn test_parse_coords() {
        let coords = parse_coords("+881824.53;+6663821.17;");
        assert!(coords.is_some());
        let (x, y) = coords.unwrap();
        assert!((x - 881824.53).abs() < 0.01);
        assert!((y - 6663821.17).abs() < 0.01);
    }

    #[test]
    fn test_parse_coords_no_plus() {
        let coords = parse_coords("881824.53;6663821.17");
        assert!(coords.is_some());
        let (x, y) = coords.unwrap();
        assert!((x - 881824.53).abs() < 0.01);
        assert!((y - 6663821.17).abs() < 0.01);
    }

    #[test]
    fn test_extract_id() {
        let lines = vec!["PAR", "RIDSA11:Arc_1625270", "SCPCP28:..."];
        let id = extract_id(&lines);
        assert_eq!(id, "Arc_1625270");
    }
}
