// SPDX-License-Identifier: GPL-3.0-or-later
// Dispersion, air, and Buchdahl equations translated from Geopter (Hiiragi, 2021).
use super::interval::Real;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

pub const D_LINE: f64 = 587.5618;
pub const F_LINE: f64 = 486.1327;
pub const C_LINE: f64 = 656.2725;

#[derive(schemars::JsonSchema, Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Glass {
    pub name: String,
    pub catalog: String,
    #[schemars(range(min = 1, max = 13))]
    pub formula: u8,
    #[schemars(length(min = 0, max = 20), inner(range(min = -1e15, max = 1e15)))]
    pub coefficients: Vec<f64>,
    #[schemars(range(min = -1e15, max = 1e15))]
    pub nd: f64,
    #[schemars(range(min = -1e15, max = 1e15))]
    pub vd: f64,
    #[schemars(
        schema_with = "schemars::SchemaGenerator::subschema_for::<Option<[f64; 2]>>",
        inner(range(min = 0.0, max = 100.0))
    )]
    pub wavelength_range: Option<[f64; 2]>, // micrometers
    #[schemars(schema_with = "schemars::SchemaGenerator::subschema_for::<Option<[f64; 7]>>", inner(range(min = -1e15, max = 1e15)))]
    pub thermal: Option<[f64; 7]>, // D0,D1,D2,E0,E1,Ltk,Tref
    #[schemars(length(min = 0, max = 10000), inner(inner(range(min = -1e15, max = 1e15))))]
    pub transmission: Vec<[f64; 3]>, // micrometers, internal transmission, thickness mm
    pub comment: String,
}

pub fn dispersion(formula: u8, c: &[f64], l: f64) -> Result<f64, String> {
    dispersion_real(formula, c, l)
}

/// [`dispersion`] for any [`Real`] wavelength (µm): a point, an interval enclosure,
/// or a dual carrying derivatives. Operations keep the f64 evaluation order.
pub fn dispersion_real<T: Real>(formula: u8, c: &[f64], l: T) -> Result<T, String> {
    if !l.finite() || l.lo() <= 0. || c.iter().any(|x| !x.is_finite()) {
        return Err("Finite positive wavelength and finite coefficients required".into());
    }
    let needed = match formula {
        1..=3 => 6,
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
    let sum = |range: std::ops::Range<usize>, term: &dyn Fn(usize) -> T| {
        range.fold(T::cst(0.), |total, i| total + term(i))
    };
    let sellmeier = |terms: usize| sum(0..terms, &|i| l2 * c[2 * i] / (l2 - c[2 * i + 1])) + 1.;
    let schott = || l2 * c[1] + c[0] + sum(2..6, &|i| l.powi(-2 * (i as i32 - 1)) * c[i]);
    let n = match formula {
        1 => schott().sqrt(),
        2 => sellmeier(3).sqrt(),
        3 => {
            let h = T::cst(1.) / (l2 - 0.028);
            h * c[1] + c[0] + h * c[2] * h + l2 * c[3] + l.powi(4) * c[4] + l.powi(6) * c[5]
        }
        4 => (l2 * c[1] / (l2 - c[2]) + (1. + c[0]) + l2 * c[3] / (l2 - c[4])).sqrt(),
        5 => T::cst(c[1]) / l + c[0] + T::cst(c[2]) / l.powf(3.5),
        6 => sellmeier(4).sqrt(),
        7 => (T::cst(c[1]) / (l2 - c[2]) + c[0] - l2 * c[3]).sqrt(),
        8 => (l2 * c[1] / (l2 - c[2]) + c[0] - l2 * c[3]).sqrt(),
        9 => (l2 * c[1] / (l2 - c[2]) + c[0] + l2 * c[3] / (l2 - c[4])).sqrt(),
        10 => (schott() + l.powi(-10) * c[6] + l.powi(-12) * c[7]).sqrt(),
        11 => sellmeier(5).sqrt(),
        12 => (schott() + l.powi(4) * c[6] + l.powi(6) * c[7]).sqrt(),
        // Hikari's nine-term polynomial: A0 + A1*l² + A2*l⁴ + A3/l² ... A8/l¹².
        // The source implementation repeats l^-8; the manufacturer equation is used here.
        13 => (l2 * c[1]
            + c[0]
            + l.powi(4) * c[2]
            + sum(3..9, &|i| l.powi(-2 * (i as i32 - 2)) * c[i]))
        .sqrt(),
        _ => unreachable!(),
    };
    if n.positive() {
        Ok(n)
    } else {
        Err("Dispersion outside its real domain".into())
    }
}

pub fn air_index(l: f64, temperature: f64, pressure: f64) -> Result<f64, String> {
    air_index_real(l, temperature, pressure)
}

/// [`air_index`] for any [`Real`] wavelength (µm).
pub fn air_index_real<T: Real>(l: T, temperature: f64, pressure: f64) -> Result<T, String> {
    if !l.finite()
        || l.lo() <= 0.
        || !temperature.is_finite()
        || temperature <= -273.15
        || !pressure.is_finite()
        || pressure < 0.
    {
        return Err("Invalid air environment".into());
    }
    let l2 = l * l;
    let refractivity =
        (l2 * 2949810. / (l2 * 146. - 1.) + 6432.8 + l2 * 25540. / (l2 * 41. - 1.)) * 1e-8;
    let n = refractivity / (1. + (temperature - 15.) * 3.4785e-3) * pressure / 101325. + 1.;
    if n.positive() {
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
            1..=3 => 6,
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
        if let Some([lo, hi]) = self.wavelength_range
            && (!lo.is_finite() || !hi.is_finite() || lo <= 0. || hi < lo)
        {
            return Err("Invalid glass wavelength range".into());
        }
        if let Some(thermal) = self.thermal
            && (thermal.iter().any(|v| !v.is_finite()) || thermal[6] <= -273.15)
        {
            return Err("Invalid glass thermal coefficients".into());
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
        self.index_real(nm, environment)
    }
    /// [`Glass::index`] for any [`Real`] wavelength (nm); the whole wavelength
    /// enclosure must lie in the glass's range.
    pub fn index_real<T: Real>(&self, nm: T, environment: Option<(f64, f64)>) -> Result<T, String> {
        let l = nm / 1000.;
        if let Some([lo, hi]) = self.wavelength_range
            && (nm.lo() / 1000. < lo || nm.hi() / 1000. > hi)
        {
            return Err(format!(
                "{} wavelength range is {}–{} nm",
                self.key(),
                lo * 1000.,
                hi * 1000.
            ));
        }
        let Some((temperature, pressure)) = environment else {
            return dispersion_real(self.formula, &self.coefficients, l);
        };
        let tref = self.thermal.map(|t| t[6]).unwrap_or(20.);
        let air = air_index_real(l, temperature, pressure)?;
        let relative_l = l * air / air_index_real(l, tref, 101325.)?;
        let n = dispersion_real(self.formula, &self.coefficients, relative_l)?;
        let delta = if let Some([d0, d1, d2, e0, e1, ltk, _]) = self.thermal {
            let dt = temperature - tref;
            (n * n - 1.) / (n * 2.)
                * (T::cst(e0 * dt + e1 * dt * dt)
                    / (relative_l * relative_l - ltk.signum() * ltk * ltk)
                    + (d0 * dt + d1 * dt * dt + d2 * dt.powi(3)))
        } else if (temperature - tref).abs() > 1e-12 {
            return Err(format!("No thermal coefficients for {}", self.key()));
        } else {
            T::cst(0.)
        };
        let result = (n * air_index_real(relative_l, tref, 101325.)? + delta)
            / air_index_real(relative_l, temperature, pressure)?;
        if result.positive() {
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

#[derive(schemars::JsonSchema, Clone, Debug, Serialize)]
pub struct CatalogWarning {
    pub catalog: String,
    pub glass: String,
    pub line: usize,
    pub message: String,
}
pub struct ParsedCatalog {
    /// Upper-case catalog name every glass in it carries.
    pub catalog: String,
    pub glasses: Vec<Glass>,
    pub warnings: Vec<CatalogWarning>,
}
pub fn parse_agf(catalog: &str, text: &str) -> Result<ParsedCatalog, String> {
    if catalog.is_empty() || catalog.len() > 128 || text.len() > 8_000_000 {
        return Err("Invalid catalog name or catalog too large".into());
    }
    let mut glasses: Vec<Glass> = vec![];
    let mut warnings = vec![];
    // Vendors wrap long CD and TD records: a line that starts with a number
    // continues the record above it.
    let mut records: Vec<(usize, Vec<&str>)> = vec![];
    for (line_no, line) in text.trim_start_matches('\u{feff}').lines().enumerate() {
        let parts: Vec<_> = line.split_whitespace().collect();
        let continues = parts
            .first()
            .is_some_and(|p| p.replace(['d', 'D'], "E").parse::<f64>().is_ok());
        match records.last_mut() {
            Some((_, record)) if continues => record.extend(parts),
            _ if !parts.is_empty() => records.push((line_no, parts)),
            _ => {}
        }
    }
    // Records of a repeated glass are dropped until the next NM.
    let mut skipping = false;
    for (line_no, parts) in records {
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
                skipping = glasses.iter().any(|g| g.name == name);
                if skipping {
                    warnings.push(CatalogWarning {
                        catalog: catalog.to_uppercase(),
                        glass: name,
                        line: line_no + 1,
                        message: "Repeated glass ignored; the first entry is kept".into(),
                    });
                    continue;
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
            "CD" | "TD" | "LD" | "IT" | "GC" if skipping => {}
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
            "CC" | "ED" | "OD" | "MD" | "BD" | "LDATA" => {}
            other => warnings.push(CatalogWarning {
                catalog: catalog.to_uppercase(),
                glass: glasses.last().map(|g| g.name.clone()).unwrap_or_default(),
                line: line_no + 1,
                message: format!("Unknown AGF record {other} ignored"),
            }),
        }
    }
    if glasses.is_empty() {
        return Err("Empty glass catalog".into());
    }
    for g in &glasses {
        g.validate()?;
    }
    Ok(ParsedCatalog {
        catalog: catalog.to_uppercase(),
        glasses,
        warnings,
    })
}

/// The catalogs unit tests start with, standing in for installed ones: only
/// the glasses the tests name, with values from refractiveindex.info (CC0).
#[cfg(test)]
pub fn test_catalogs() -> Vec<ParsedCatalog> {
    [
        (
            "SCHOTT",
            include_str!("../../tests/fixtures/glass/SCHOTT.agf"),
        ),
        ("HOYA", include_str!("../../tests/fixtures/glass/HOYA.agf")),
        (
            "OHARA",
            include_str!("../../tests/fixtures/glass/OHARA.agf"),
        ),
        (
            "NIKON-HIKARI",
            include_str!("../../tests/fixtures/glass/NIKON-HIKARI.agf"),
        ),
    ]
    .into_iter()
    .map(|(name, text)| parse_agf(name, text).expect("Test glass fixture must be valid"))
    .collect()
}

/// The glasses `find` resolves: the catalogs installed in the data directory.
/// The project ships no vendor data; each user installs the catalogs they use.
pub struct Library {
    pub glasses: Vec<Glass>,
    pub warnings: Vec<CatalogWarning>,
    /// Upper-case `PRODUCT_CATALOG` and bare product names; a bare name shared
    /// by several catalogs maps to `None`.
    lookup: HashMap<String, Option<usize>>,
}
impl Library {
    pub fn build(installed: &[ParsedCatalog]) -> Library {
        let mut glasses = vec![];
        let mut warnings = vec![];
        for catalog in installed {
            glasses.extend(catalog.glasses.iter().cloned());
            warnings.extend(catalog.warnings.iter().cloned());
        }
        let mut lookup = HashMap::new();
        for (i, g) in glasses.iter().enumerate() {
            lookup.insert(g.key(), Some(i));
            lookup
                .entry(g.name.clone())
                .and_modify(|v| *v = None)
                .or_insert(Some(i));
        }
        Library {
            glasses,
            warnings,
            lookup,
        }
    }
    pub fn find(&self, name: &str) -> Result<&Glass, String> {
        match self.lookup.get(&name.to_uppercase()) {
            Some(Some(i)) => Ok(&self.glasses[*i]),
            Some(None) => Err(format!("Ambiguous material {name}; use PRODUCT_CATALOG")),
            None => Err(self.missing(name)),
        }
    }
    /// Why `name` does not resolve. A `PRODUCT_CATALOG` name whose catalog
    /// has no glass loaded names the catalog to install.
    fn missing(&self, name: &str) -> String {
        if let Some((glass, catalog)) = name.rsplit_once('_')
            && !glass.is_empty()
            && !catalog.is_empty()
            && !self.has_catalog(catalog)
        {
            let catalog = catalog.to_uppercase();
            return format!("{glass} is in the {catalog} glass catalog, {MISSING_CATALOG}");
        }
        format!("Unknown material: {name}")
    }
    /// Whether any loaded glass belongs to this catalog.
    pub fn has_catalog(&self, catalog: &str) -> bool {
        self.glasses
            .iter()
            .any(|g| g.catalog.eq_ignore_ascii_case(catalog))
    }
}

/// Ends the error for a glass whose catalog is not installed; the desktop
/// app recognises it and offers to open the glass catalog window.
pub const MISSING_CATALOG: &str = "which is not installed";

static LIBRARY: RwLock<Option<Arc<Library>>> = RwLock::new(None);

/// The current glass library. A caller holding it keeps a consistent view
/// while catalogs are installed or removed.
pub fn library() -> Arc<Library> {
    if let Some(library) = LIBRARY.read().unwrap_or_else(|e| e.into_inner()).as_ref() {
        return library.clone();
    }
    LIBRARY
        .write()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert_with(|| {
            #[cfg(test)]
            let installed = test_catalogs();
            #[cfg(not(test))]
            let installed: Vec<ParsedCatalog> = vec![];
            Arc::new(Library::build(&installed))
        })
        .clone()
}
/// Replaces the library with these installed catalogs.
pub fn set_installed(installed: &[ParsedCatalog]) {
    let library = Arc::new(Library::build(installed));
    *LIBRARY.write().unwrap_or_else(|e| e.into_inner()) = Some(library);
}
pub fn find(name: &str) -> Result<Glass, String> {
    library().find(name).cloned()
}
pub fn buchdahl(nd: f64, vd: f64, nm: f64) -> Result<f64, String> {
    buchdahl_real(nd, vd, nm)
}
/// [`buchdahl`] for any [`Real`] wavelength (nm).
pub fn buchdahl_real<T: Real>(nd: f64, vd: f64, nm: T) -> Result<T, String> {
    if !nd.is_finite() || nd <= 1. || !vd.is_finite() || vd <= 0. || !nm.finite() || nm.lo() <= 0. {
        return Err("Invalid Buchdahl model glass".into());
    }
    fn omega<T: Real>(delta: T) -> T {
        delta / (delta * 2.5 + 1.)
    }
    let of = omega((486.133 - 587.562) / 1000.);
    let oc = omega((656.273 - 587.562) / 1000.);
    let (b, m) = (-0.064667, -1.604048);
    let v2 = ((nd - 1.) / vd - b * (of - oc)) / (m * (of - oc) + (of * of - oc * oc));
    let v1 = b + m * v2;
    let o = omega((nm - 587.562) / 1000.);
    let n = o * v1 + nd + o * v2 * o;
    if n.positive() {
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
    refractive_index_real(name, nm, environment)
}
/// [`refractive_index`] for any [`Real`] wavelength (nm).
pub fn refractive_index_real<T: Real>(
    name: &str,
    nm: T,
    environment: Option<(f64, f64)>,
) -> Result<T, String> {
    if !nm.finite() || nm.lo() < 100. || nm.hi() > 20000. {
        return Err("Wavelength must be 100–20000 nm".into());
    }
    if name == "AIR" {
        if let Some((t, p)) = environment {
            air_index_real(nm / 1000., t, p)?;
        }
        return Ok(T::cst(1.));
    }
    if let Some((n, v)) = name.split_once(':') {
        return buchdahl_real(
            n.parse().map_err(|_| "Invalid model index")?,
            v.parse().map_err(|_| "Invalid model Abbe number")?,
            nm,
        );
    }
    if name == "FUSED_SILICA" {
        return dispersion_real(
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
    library().find(name)?.index_real(nm, environment)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tests_load_only_the_fixture() {
        let library = library();
        assert_eq!(library.glasses.len(), 38);
        assert!(library.warnings.is_empty(), "{:?}", library.warnings);
        assert!(!library.has_catalog("CDGM"));
    }
    #[test]
    fn missing_catalog_is_named() {
        let schott = test_catalogs().into_iter().find(|c| c.catalog == "SCHOTT");
        let library = Library::build(&[schott.unwrap()]);
        assert_eq!(library.find("n-sf6_schott").unwrap().name, "N-SF6");
        assert_eq!(library.find("N-SF6").unwrap().catalog, "SCHOTT");
        assert_eq!(
            library.find("S-BSL7_OHARA").unwrap_err(),
            "S-BSL7 is in the OHARA glass catalog, which is not installed"
        );
        assert_eq!(
            library.find("N-FK5_SCHOTT").unwrap_err(),
            "Unknown material: N-FK5_SCHOTT"
        );
        let empty = Library::build(&[]);
        assert_eq!(empty.find("N-BK7").unwrap_err(), "Unknown material: N-BK7");
        assert!(
            empty
                .find("N-BK7_SCHOTT")
                .unwrap_err()
                .ends_with(MISSING_CATALOG)
        );
    }
    #[test]
    fn bk7_matches_datasheet() {
        let g = find("N-BK7_SCHOTT").unwrap();
        assert!((g.index(D_LINE, None).unwrap() - 1.5168).abs() < 1e-5);
        assert!((g.abbe().unwrap() - 64.17).abs() < 0.02);
    }
    #[test]
    fn hikari_matches_catalog_index() {
        // J-BK7A from refractiveindex.info (CC0), in Hikari's nine-term form.
        let text = "NM J-BK7A 13 517642 1.5168 64.13 0 1\n\
            CD 2.27110883 -0.00938988354 -0.000100277081 0.0109572221 \
            0.000120210067 3.31079774e-06 -1.48235581e-08 0 0\nLD 0.365 2.058\n";
        let g = &parse_agf("NIKON-HIKARI", text).unwrap().glasses[0];
        let error = (g.index(D_LINE, None).unwrap() - g.nd).abs();
        assert!(error < 2e-5, "Hikari index error {error}");
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
        assert!(parse_agf("BAD", "hello world\nE").is_err());
    }

    #[test]
    fn vendor_layouts_parse() {
        let text = "CC x\nNM A 2 0 1.5168 64.17 0 1\nMD 82 0.21 580 820 1.19\nBD note\n\
            CD 1.03961212 0.231792344 1.01046945\n0.00600069867 0.0200179144 103.560653\n\
            TD 1 2\n3 4 5 6 7\nLD 0.3 2.5\nE\nNM A 2 0 1.6 50\nCD 9\n";
        let parsed = parse_agf("V", text).unwrap();
        let g = &parsed.glasses[0];
        assert_eq!(g.coefficients.len(), 6);
        assert!(g.thermal.is_some());
        assert_eq!(parsed.glasses.len(), 1);
        assert_eq!(g.nd, 1.5168);
        let lines: Vec<_> = parsed.warnings.iter().map(|w| w.line).collect();
        assert_eq!(lines, [10, 11]);
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
