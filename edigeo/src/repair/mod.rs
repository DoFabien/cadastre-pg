//! Réparation et construction des géométries

pub mod fallback;
pub mod ring;
pub mod topology;

use std::collections::HashMap;

use geo::{Coord, Geometry, LineString, Point};
use tracing::warn;

use crate::parser::vec::{ParsedVec, Reference};
use crate::types::{Feature, Quality};
use crate::EdigeoError;

/// Construit les géométries à partir des entités VEC parsées
pub fn build_geometries(
    parsed: &ParsedVec,
    quality: &HashMap<String, Quality>,
) -> Result<Vec<Feature>, EdigeoError> {
    let mut features = Vec::new();

    // Parcourir les LNK pour trouver les associations FEA -> géométrie
    for lnk in parsed.lnk.values() {
        let Some(ref scp) = lnk.scp else {
            continue;
        };

        if scp.rty != "REL" {
            continue;
        }

        // Trouver la FEA associée
        let fea_ref = lnk.ftp.iter().find(|r| r.rty == "FEA");
        let Some(fea_ref) = fea_ref else {
            continue;
        };

        let Some(fea) = parsed.fea.get(&fea_ref.rid) else {
            continue;
        };

        // Déterminer le type de géométrie
        let pfe_refs: Vec<&Reference> = lnk.ftp.iter().filter(|r| r.rty == "PFE").collect();
        let par_refs: Vec<&Reference> = lnk.ftp.iter().filter(|r| r.rty == "PAR").collect();
        let pno_refs: Vec<&Reference> = lnk.ftp.iter().filter(|r| r.rty == "PNO").collect();

        let geometry = if !pfe_refs.is_empty() {
            // Polygon depuis PFE
            build_polygon_from_pfe(parsed, &pfe_refs, &fea.id)?
        } else if !par_refs.is_empty() {
            // LineString depuis PAR
            build_linestring_from_par(parsed, &par_refs)
        } else if !pno_refs.is_empty() {
            // Point depuis PNO
            build_point_from_pno(parsed, &pno_refs)
        } else {
            continue;
        };

        let Some(geometry) = geometry else {
            continue;
        };

        // Construire les propriétés
        let mut properties = fea.attributes.clone();

        // Ajouter les informations de qualité si disponibles
        if let Some(qap_id) = &fea.qap {
            if let Some(q) = quality.get(qap_id) {
                if let Some(ref date) = q.create_date {
                    properties.insert("createDate".to_string(), date.clone());
                }
                if let Some(ref date) = q.update_date {
                    properties.insert("updateDate".to_string(), date.clone());
                }
            }
        }

        // Déterminer le type de feature
        //
        // On conserve le suffixe `_id` pour rester compatible avec les fichiers de configuration
        // (hérités de la version Node.js) qui référencent des types comme `PARCELLE_id`.
        let feature_type = fea
            .scp
            .as_ref()
            .map(|s| s.rid.to_string())
            .unwrap_or_else(|| "UNKNOWN".to_string());

        // Utiliser IDU comme ID si disponible (format cadastral), sinon ID interne
        let feature_id = fea
            .attributes
            .get("IDU")
            .filter(|s| !s.is_empty())
            .cloned()
            .unwrap_or_else(|| fea.id.clone());

        features.push(Feature {
            id: feature_id,
            geometry,
            properties,
            feature_type,
        });
    }

    Ok(features)
}

/// Construit un Point depuis des références PNO
fn build_point_from_pno(parsed: &ParsedVec, refs: &[&Reference]) -> Option<Geometry> {
    let pno_ref = refs.first()?;
    let pno = parsed.pno.get(&pno_ref.rid)?;

    if pno.coords.is_empty() {
        return None;
    }

    let (x, y) = pno.coords[0];
    Some(Geometry::Point(Point::new(x, y)))
}

/// Construit un LineString depuis des références PAR
fn build_linestring_from_par(parsed: &ParsedVec, refs: &[&Reference]) -> Option<Geometry> {
    if refs.len() == 1 {
        let par = parsed.par.get(&refs[0].rid)?;
        let coords: Vec<Coord> = par.coords.iter().map(|&(x, y)| Coord { x, y }).collect();
        if coords.len() < 2 {
            return None;
        }
        Some(Geometry::LineString(LineString::new(coords)))
    } else {
        // MultiLineString
        let lines: Vec<Vec<Coord>> = refs
            .iter()
            .filter_map(|r| parsed.par.get(&r.rid))
            .map(|par| {
                par.coords
                    .iter()
                    .map(|&(x, y)| Coord { x, y })
                    .collect::<Vec<_>>()
            })
            .filter(|coords| coords.len() >= 2)
            .collect();

        if lines.is_empty() {
            None
        } else {
            Some(Geometry::MultiLineString(geo::MultiLineString::new(
                lines.into_iter().map(LineString::new).collect(),
            )))
        }
    }
}

/// Construit un Polygon depuis des références PFE
fn build_polygon_from_pfe(
    parsed: &ParsedVec,
    refs: &[&Reference],
    entity_id: &str,
) -> Result<Option<Geometry>, EdigeoError> {
    if refs.is_empty() {
        return Ok(None);
    }

    // Collecter les arcs de toutes les faces référencées
    let mut all_arcs: Vec<Vec<Coord>> = Vec::new();

    for pfe_ref in refs {
        if let Some(face) = parsed.pfe.get(&pfe_ref.rid) {
            for arc in &face.arcs {
                let coords: Vec<Coord> = arc.coords.iter().map(|&(x, y)| Coord { x, y }).collect();
                if !coords.is_empty() {
                    all_arcs.push(coords);
                }
            }
        }
    }

    if all_arcs.is_empty() {
        return Ok(None);
    }

    // Tenter de reconstruire les rings
    match ring::reconstruct_rings(&all_arcs) {
        Ok(rings) => {
            if rings.is_empty() {
                return Ok(None);
            }

            // Organiser en polygones avec trous
            let polygons = topology::organize_rings(rings);

            if polygons.len() == 1 {
                Ok(Some(Geometry::Polygon(
                    polygons.into_iter().next().unwrap(),
                )))
            } else {
                Ok(Some(Geometry::MultiPolygon(geo::MultiPolygon::new(
                    polygons,
                ))))
            }
        }
        Err(_) => {
            // Fallback: convex hull
            warn!(entity_id = %entity_id, "Ring reconstruction failed, using convex hull");
            match fallback::convex_hull_fallback(&all_arcs) {
                Ok(polygon) => Ok(Some(Geometry::Polygon(polygon))),
                Err(e) => {
                    warn!(entity_id = %entity_id, error = %e, "Convex hull fallback failed");
                    Ok(None)
                }
            }
        }
    }
}
