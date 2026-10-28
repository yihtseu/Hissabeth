// SPDX-License-Identifier: GPL-3.0-or-later
// Dispersion, air, and Buchdahl equations translated from Geopter (Hiiragi, 2021).
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

pub const D_LINE: f64 = 587.5618;
pub const F_LINE: f64 = 486.1327;
pub const C_LINE: f64 = 656.2725;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Glass {
    pub name: String,
    pub catalog: String,
    pub formula: u8,
    pub coefficients: Vec<f64>,
    pub nd: f64,
    pub vd: f64,
    pub wavelength_range: Option<[f64; 2]>, // micrometers
    pub thermal: Option<[f64; 7]>,          // D0,D1,D2,E0,E1,Ltk,Tref
    pub transmission: Vec<[f64; 3]>,        // micrometers, internal transmission, thickness mm
    pub comment: String,
}

pub fn dispersion(formula: u8, c: &[f64], l: f64) -> Result<f64, String> {
    if !l.is_finite() || l <= 0. || c.iter().any(|x| !x.is_finite()) {
        return Err("Finite positive wavelength and finite coefficients required".into());
    }
    let needed = match formula {
        1 | 2 | 3 => 6,
        4 | 9 => 5,
        5 => 3,
        6 | 10 | 12 => 8,
        7 | 8 => 4,
        11 => 10,
        13 => 9,
        _ => return Err(format!("Unknown dispersion formula {formula}")),
    };
    if c.len() < needed {
        return Err(format!("Formula {formula} needs {needed} coefficients"));
    }
    let l2 = l * l;
    let sellmeier = |terms: usize| {
        1. + (0..terms)
            .map(|i| c[2 * i] * l2 / (l2 - c[2 * i + 1]))
            .sum::<f64>()
    };
    let schott = || {
        c[0] + c[1] * l2
            + (2..6)
                .map(|i| c[i] * l.powi(-2 * (i as i32 - 1)))
                .sum::<f64>()
    };
    let n = match formula {
        1 => schott().sqrt(),
        2 => sellmeier(3).sqrt(),
        3 => {
            let h = 1. / (l2 - 0.028);
            c[0] + c[1] * h + c[2] * h * h + c[3] * l2 + c[4] * l.powi(4) + c[5] * l.powi(6)
        }
        4 => (1. + c[0] + c[1] * l2 / (l2 - c[2]) + c[3] * l2 / (l2 - c[4])).sqrt(),
        5 => c[0] + c[1] / l + c[2] / l.powf(3.5),
        6 => sellmeier(4).sqrt(),
        7 => (c[0] + c[1] / (l2 - c[2]) - c[3] * l2).sqrt(),
        8 => (c[0] + c[1] * l2 / (l2 - c[2]) - c[3] * l2).sqrt(),
        9 => (c[0] + c[1] * l2 / (l2 - c[2]) + c[3] * l2 / (l2 - c[4])).sqrt(),
        10 => (schott() + c[6] * l.powi(-10) + c[7] * l.powi(-12)).sqrt(),
        11 => sellmeier(5).sqrt(),
        12 => (schott() + c[6] * l.powi(4) + c[7] * l.powi(6)).sqrt(),
        // Hikari's nine-term polynomial: A0 + A1*l² + A2*l⁴ + A3/l² ... A8/l¹².
        // The source implementation repeats l^-8; the manufacturer equation is used here.
        13 => (c[0]
            + c[1] * l2
            + c[2] * l.powi(4)
            + (3..9)
                .map(|i| c[i] * l.powi(-2 * (i as i32 - 2)))
                .sum::<f64>())
        .sqrt(),
        _ => unreachable!(),
    };
    if n.is_finite() && n > 0. {
        Ok(n)
    } else {
        Err("Dispersion outside its real domain".into())
    }
}

pub fn air_index(l: f64, temperature: f64, pressure: f64) -> Result<f64, String> {
    if !l.is_finite()
        || l <= 0.
        || !temperature.is_finite()
        || temperature <= -273.15
        || !pressure.is_finite()
        || pressure < 0.
    {
        return Err("Invalid air environment".into());
    }
    let l2 = l * l;
    let refractivity =
        (6432.8 + 2949810. * l2 / (146. * l2 - 1.) + 25540. * l2 / (41. * l2 - 1.)) * 1e-8;
    let n = 1. + refractivity / (1. + (temperature - 15.) * 3.4785e-3) * pressure / 101325.;
    if n.is_finite() && n > 0. {
        Ok(n)
    } else {
        Err("Air formula outside its real domain".into())
    }
}
impl Glass {
    pub fn validate(&self) -> Result<(), String> {
        if self.name.is_empty()
            || self.catalog.is_empty()
            || self.name.len() > 128
            || self.catalog.len() > 128
            || !self.nd.is_finite()
            || self.nd <= 0.
            || !self.vd.is_finite()
            || self.vd < 0.
            || self.coefficients.iter().any(|v| !v.is_finite())
            || self.coefficients.len() > 20
        {
            return Err("Invalid glass metadata".into());
        }
        let minimum = match self.formula {
            1 | 2 | 3 => 6,
            4 | 9 => 5,
            5 => 3,
            6 | 10 | 12 => 8,
            7 | 8 => 4,
            11 => 10,
            13 => 9,
            _ => return Err("Unknown glass dispersion formula".into()),
        };
        if self.coefficients.len() < minimum {
            return Err(format!(
                "{} needs at least {minimum} dispersion coefficients",
                self.key()
            ));
        }
        if let Some([lo, hi]) = self.wavelength_range {
            if !lo.is_finite() || !hi.is_finite() || lo <= 0. || hi < lo {
                return Err("Invalid glass wavelength range".into());
            }
        }
        if let Some(thermal) = self.thermal {
            if thermal.iter().any(|v| !v.is_finite()) || thermal[6] <= -273.15 {
                return Err("Invalid glass thermal coefficients".into());
            }
        }
        if self.transmission.iter().any(|p| {
            !p.iter().all(|v| v.is_finite())
                || p[0] <= 0.
                || !(0.0..=1.0).contains(&p[1])
                || p[2] <= 0.
        }) {
            return Err("Invalid internal transmission data".into());
        }
        Ok(())
    }
    pub fn key(&self) -> String {
        format!("{}_{}", self.name, self.catalog)
    }
    pub fn index(&self, nm: f64, environment: Option<(f64, f64)>) -> Result<f64, String> {
        let l = nm / 1000.;
        if let Some([lo, hi]) = self.wavelength_range {
            if l < lo || l > hi {
                return Err(format!(
                    "{} wavelength range is {}–{} nm",
                    self.key(),
                    lo * 1000.,
                    hi * 1000.
                ));
            }
        }
        let Some((temperature, pressure)) = environment else {
            return dispersion(self.formula, &self.coefficients, l);
        };
        let tref = self.thermal.map(|t| t[6]).unwrap_or(20.);
        let air = air_index(l, temperature, pressure)?;
        let relative_l = l * air / air_index(l, tref, 101325.)?;
        let n = dispersion(self.formula, &self.coefficients, relative_l)?;
        let delta = if let Some([d0, d1, d2, e0, e1, ltk, _]) = self.thermal {
            let dt = temperature - tref;
            (n * n - 1.) / (2. * n)
                * (d0 * dt
                    + d1 * dt * dt
                    + d2 * dt.powi(3)
                    + (e0 * dt + e1 * dt * dt)
                        / (relative_l * relative_l - ltk.signum() * ltk * ltk))
        } else if (temperature - tref).abs() > 1e-12 {
            return Err(format!("No thermal coefficients for {}", self.key()));
        } else {
            0.
        };
        let result = (n * air_index(relative_l, tref, 101325.)? + delta)
            / air_index(relative_l, temperature, pressure)?;
        if result.is_finite() && result > 0. {
            Ok(result)
        } else {
            Err("Invalid thermal refractive index".into())
        }
    }
    pub fn abbe(&self) -> Result<f64, String> {
        Ok((self.index(D_LINE, None)? - 1.)
            / (self.index(F_LINE, None)? - self.index(C_LINE, None)?))
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct CatalogWarning {
    pub catalog: String,
    pub glass: String,
    pub line: usize,
    pub message: String,
}
pub struct ParsedCatalog {
    pub glasses: Vec<Glass>,
    pub warnings: Vec<CatalogWarning>,
}
pub fn parse_agf(catalog: &str, text: &str) -> Result<ParsedCatalog, String> {
    if catalog.is_empty() || catalog.len() > 128 || text.len() > 8_000_000 {
        return Err("Invalid catalog name or catalog too large".into());
    }
    let mut glasses: Vec<Glass> = vec![];
    let mut warnings = vec![];
    for (line_no, line) in text.trim_start_matches('\u{feff}').lines().enumerate() {
        let parts: Vec<_> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }
        let err = |m: &str| format!("AGF line {}: {m}", line_no + 1);
        let number = |i: usize| -> Result<f64, String> {
            let n = parts
                .get(i)
                .ok_or_else(|| err("Missing numeric column"))?
                .replace(['d', 'D'], "E")
                .parse::<f64>()
                .map_err(|_| err("Malformed number"))?;
            if n.is_finite() {
                Ok(n)
            } else {
                Err(err("Nonfinite number"))
            }
        };
        match parts[0] {
            "NM" => {
                if parts.len() < 6 {
                    return Err(err("Incomplete NM record"));
                }
                let f = number(2)?;
                if f.fract() != 0. || !(1.0..=13.0).contains(&f) {
                    return Err(err("Unknown dispersion formula"));
                }
                let name = parts[1].to_uppercase();
                if glasses.iter().any(|g| g.name == name) {
                    return Err(err("Duplicate glass name"));
                }
                glasses.push(Glass {
                    name,
                    catalog: catalog.to_uppercase(),
                    formula: f as u8,
                    coefficients: vec![],
                    nd: number(4)?,
                    vd: number(5)?,
                    wavelength_range: None,
                    thermal: None,
                    transmission: vec![],
                    comment: String::new(),
                });
            }
            "CD" | "TD" | "LD" | "IT" | "GC" => {
                let g = glasses
                    .last_mut()
                    .ok_or_else(|| err("Glass data before NM record"))?;
                match parts[0] {
                    "CD" => {
                        g.coefficients = (1..parts.len())
                            .map(number)
                            .collect::<Result<Vec<_>, _>>()?;
                    }
                    "TD" => {
                        if parts.len() < 8 {
                            warnings.push(CatalogWarning {
                                catalog: catalog.to_uppercase(),
                                glass: g.name.clone(),
                                line: line_no + 1,
                                message: "Incomplete thermal record omitted".into(),
                            });
                            continue;
                        }
                        let values = (1..8).map(number).collect::<Result<Vec<_>, _>>()?;
                        g.thermal = Some(values.try_into().unwrap());
                    }
                    "LD" => {
                        let lo = number(1)?;
                        let hi = number(2)?;
                        if lo <= 0. || hi < lo {
                            return Err(err("Invalid wavelength range"));
                        }
                        g.wavelength_range = Some([lo, hi]);
                    }
                    "IT" => {
                        if parts.len() != 4 {
                            warnings.push(CatalogWarning {
                                catalog: catalog.to_uppercase(),
                                glass: g.name.clone(),
                                line: line_no + 1,
                                message: "Incomplete internal transmission record omitted".into(),
                            });
                            continue;
                        }
                        let entry = [number(1)?, number(2)?, number(3)?];
                        if entry[0] <= 0. || !(0.0..=1.0).contains(&entry[1]) || entry[2] <= 0. {
                            warnings.push(CatalogWarning {
                                catalog: catalog.to_uppercase(),
                                glass: g.name.clone(),
                                line: line_no + 1,
                                message: "Nonphysical internal transmission record omitted".into(),
                            });
                            continue;
                        }
                        g.transmission.push(entry);
                    }
                    "GC" => g.comment = parts[1..].join(" "),
                    _ => unreachable!(),
                }
            }
            // AGF metadata records do not change the dispersion calculation.
            "CC" | "ED" | "OD" | "LDATA" => {}
            other => return Err(err(&format!("Unknown AGF record {other}"))),
        }
    }
    if glasses.is_empty() {
        return Err("Empty glass catalog".into());
    }
    for g in &glasses {
        g.validate()?;
    }
    Ok(ParsedCatalog { glasses, warnings })
}

fn library() -> &'static ParsedCatalog {
    static GLASSES: OnceLock<ParsedCatalog> = OnceLock::new();
    GLASSES.get_or_init(|| {
        let sources = [
            ("SCHOTT", include_str!("../../data/glass/SCHOTT.agf")),
            ("HOYA", include_str!("../../data/glass/HOYA.agf")),
            ("OHARA", include_str!("../../data/glass/OHARA.AGF")),
            ("CDGM", include_str!("../../data/glass/CDGM.AGF")),
            (
                "NIKON-HIKARI",
                include_str!("../../data/glass/NIKON-HIKARI.agf"),
            ),
            ("INFRARED", include_str!("../../data/glass/INFRARED.AGF")),
        ];
        let mut all = ParsedCatalog {
            glasses: vec![],
            warnings: vec![],
        };
        for (name, data) in sources {
            let parsed = parse_agf(name, data).expect("Embedded AGF catalog must be valid");
            all.glasses.extend(parsed.glasses);
            all.warnings.extend(parsed.warnings);
        }
        all
    })
}
pub fn catalogs() -> &'static [Glass] {
    &library().glasses
}
pub fn catalog_warnings() -> &'static [CatalogWarning] {
    &library().warnings
}
pub fn find(name: &str) -> Result<&'static Glass, String> {
    static LOOKUP: OnceLock<std::collections::HashMap<String, Option<usize>>> = OnceLock::new();
    let map = LOOKUP.get_or_init(|| {
        let mut map = std::collections::HashMap::new();
        for (i, g) in catalogs().iter().enumerate() {
            map.insert(g.key(), Some(i));
            map.entry(g.name.clone())
                .and_modify(|v| *v = None)
                .or_insert(Some(i));
        }
        map
    });
    match map.get(&name.to_uppercase()) {
        Some(Some(i)) => Ok(&catalogs()[*i]),
        Some(None) => Err(format!("Ambiguous material {name}; use PRODUCT_CATALOG")),
        None => Err(format!("Unknown material: {name}")),
    }
}
pub fn buchdahl(nd: f64, vd: f64, nm: f64) -> Result<f64, String> {
    if !nd.is_finite() || nd <= 1. || !vd.is_finite() || vd <= 0. || !nm.is_finite() || nm <= 0. {
        return Err("Invalid Buchdahl model glass".into());
    }
    let omega = |delta: f64| delta / (1. + 2.5 * delta);
    let of = omega((486.133 - 587.562) / 1000.);
    let oc = omega((656.273 - 587.562) / 1000.);
    let (b, m) = (-0.064667, -1.604048);
    let v2 = ((nd - 1.) / vd - b * (of - oc)) / (m * (of - oc) + (of * of - oc * oc));
    let v1 = b + m * v2;
    let o = omega((nm - 587.562) / 1000.);
    let n = nd + v1 * o + v2 * o * o;
    if n.is_finite() && n > 0. {
        Ok(n)
    } else {
        Err("Buchdahl dispersion outside its real domain".into())
    }
}
pub fn refractive_index(
    name: &str,
    nm: f64,
    environment: Option<(f64, f64)>,
) -> Result<f64, String> {
    if !nm.is_finite() || !(100.0..=20000.0).contains(&nm) {
        return Err("Wavelength must be 100–20000 nm".into());
    }
    if name == "AIR" {
        if let Some((t, p)) = environment {
            air_index(nm / 1000., t, p)?;
        }
        return Ok(1.);
    }
    if let Some((n, v)) = name.split_once(':') {
        return buchdahl(
            n.parse().map_err(|_| "Invalid model index")?,
            v.parse().map_err(|_| "Invalid model Abbe number")?,
            nm,
        );
    }
    if name == "FUSED_SILICA" {
        return dispersion(
            2,
            &[
                0.6961663,
                0.00467914826,
                0.4079426,
                0.0135120631,
                0.8974794,
                97.9340025,
            ],
            nm / 1000.,
        );
    }
    find(name)?.index(nm, environment)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_catalogs_parse() {
        assert_eq!(catalogs().len(), 1508);
    }
    #[test]
    fn bk7_matches_datasheet() {
        let g = find("N-BK7_SCHOTT").unwrap();
        assert!((g.index(D_LINE, None).unwrap() - 1.5168).abs() < 1e-5);
        assert!((g.abbe().unwrap() - 64.17).abs() < 0.02);
    }
    #[test]
    fn hikari_matches_catalog_index() {
        let mut worst: f64 = 0.;
        let mut count = 0;
        for g in catalogs().iter().filter(|g| g.formula == 13) {
            worst = worst.max((g.index(D_LINE, None).unwrap() - g.nd).abs());
            count += 1;
        }
        assert_eq!(count, 173);
        assert!(worst < 2e-5, "Hikari maximum index error {worst}");
    }
    #[test]
    fn buchdahl_preserves_nd_and_vd() {
        let nd = 1.691;
        let vd = 54.71;
        assert!((buchdahl(nd, vd, 587.562).unwrap() - nd).abs() < 1e-12);
        let v =
            (nd - 1.) / (buchdahl(nd, vd, 486.133).unwrap() - buchdahl(nd, vd, 656.273).unwrap());
        assert!((v - vd).abs() < 1e-9);
    }
    #[test]
    fn malformed_agf_is_rejected() {
        assert!(parse_agf("BAD", "CD 1 2 3").is_err());
        assert!(parse_agf("BAD", "NM A 2 0 1.5 60\nCD x").is_err());
        assert!(parse_agf("BAD", "NM A 99 0 1.5 60").is_err());
    }
    #[test]
    fn air_pressure_and_thermal() {
        assert_eq!(air_index(0.587, 20., 0.).unwrap(), 1.);
        let g = find("N-BK7_SCHOTT").unwrap();
        let n = g.index(D_LINE, Some((20., 101325.))).unwrap();
        assert!((n - g.index(D_LINE, None).unwrap()).abs() < 1e-12);
        assert!((g.index(D_LINE, Some((80., 101325.))).unwrap() - n).abs() > 1e-7);
    }
}
