// ZMX (Zemax OpticStudio sequential) text exchange.
// Supported subset: UNIT MM, STANDARD/EVENASPH/ODDASPHE/TOROIDAL/ZERNFRIN
// profiles, circular CLAP apertures, catalog/MODEL/MIRROR glasses,
// FLD/WAVM/ENPD/FNUM/OBNA, symmetric VCX/VCY vignetting, TEM/PRES. Constructs
// that change the optical model but have no native representation produce
// explicit errors.
use super::*;
use std::collections::BTreeMap;
use std::fmt::Write as _;

fn num(token: Option<&str>, label: &str) -> Result<f64, String> {
    let token = token.ok_or_else(|| format!("ZMX {label}: missing number"))?;
    let value: f64 = token
        .replace(',', ".")
        .parse()
        .map_err(|_| format!("ZMX {label}: malformed number {token}"))?;
    if value.is_finite() {
        Ok(value)
    } else {
        Err(format!("ZMX {label}: nonfinite number"))
    }
}
fn int(token: Option<&str>, label: &str) -> Result<usize, String> {
    token
        .ok_or_else(|| format!("ZMX {label}: missing integer"))?
        .parse()
        .map_err(|_| format!("ZMX {label}: malformed integer"))
}
/// ZMX wavelengths are micrometers; snap the nm conversion so stored values
/// survive a ÷1000/×1000 exchange unchanged (0.001 pm resolution).
fn snap_nm(nm: f64) -> f64 {
    (nm * 1e9).round() / 1e9
}

#[derive(Default)]
struct ZmxSurface {
    kind: String,
    curvature: f64,
    thickness: Option<f64>,
    conic: f64,
    glass: Option<String>,
    model: Option<(f64, f64)>,
    mirror: bool,
    stop: bool,
    label: String,
    clip: Option<f64>,
    params: BTreeMap<usize, f64>,
}
impl ZmxSurface {
    #[allow(clippy::type_complexity)]
    fn profile(
        &self,
        surface: usize,
    ) -> Result<
        (
            f64,
            f64,
            Vec<f64>,
            Vec<f64>,
            Option<Toroid>,
            Option<SagZernike>,
        ),
        String,
    > {
        let radius = if self.curvature == 0. {
            0.
        } else {
            1. / self.curvature
        };
        let (mut even, mut odd) = (vec![], vec![]);
        let (mut toroid, mut zernike) = (None, None);
        match self.kind.as_str() {
            "STANDARD" => {
                if self.params.values().any(|v| *v != 0.) {
                    return Err(format!(
                        "ZMX surface {surface}: STANDARD with nonzero PARM is unsupported"
                    ));
                }
            }
            "EVENASPH" => {
                // PARM 1..8 hold the r^2..r^16 coefficients; the native even
                // list begins at r^4, so a nonzero r^2 term is not portable.
                if self.params.get(&1).is_some_and(|v| *v != 0.) {
                    return Err(format!(
                        "ZMX surface {surface}: even-asphere r^2 term cannot be represented; fold it into the curvature"
                    ));
                }
                for slot in 2..=8 {
                    even.push(self.params.get(&slot).copied().unwrap_or(0.));
                }
                while even.last() == Some(&0.) {
                    even.pop();
                }
            }
            "ODDASPHE" => {
                // PARM 1..8 hold beta_1..beta_8 on r^1..r^8; the native odd
                // list begins at r^3, so nonzero r^1/r^2 terms are not portable.
                for slot in [1, 2] {
                    if self.params.get(&slot).is_some_and(|v| *v != 0.) {
                        return Err(format!(
                            "ZMX surface {surface}: odd-asphere r^{slot} term cannot be represented"
                        ));
                    }
                }
                for slot in 3..=8 {
                    odd.push(self.params.get(&slot).copied().unwrap_or(0.));
                }
                while odd.last() == Some(&0.) {
                    odd.pop();
                }
            }
            "TOROIDAL" => {
                // PARM 1 is the radius of revolution; PARM 2..8 hold the
                // y^2..y^14 coefficients. The native even_y list begins at
                // y^4, so a nonzero y^2 term is not portable.
                if self.params.get(&2).is_some_and(|v| *v != 0.) {
                    return Err(format!(
                        "ZMX surface {surface}: toroidal y^2 term cannot be represented; fold it into the curvature"
                    ));
                }
                let mut even_y = vec![];
                for slot in 3..=8 {
                    even_y.push(self.params.get(&slot).copied().unwrap_or(0.));
                }
                while even_y.last() == Some(&0.) {
                    even_y.pop();
                }
                toroid = Some(Toroid {
                    revolution: self.params.get(&1).copied().unwrap_or(0.),
                    even_y,
                });
            }
            "ZERNFRIN" => {
                // PARM 9/10 decenter the Zernike terms, PARM 13 counts them,
                // PARM 14 is the normalization radius, PARM 15..51 hold the
                // Fringe Z1..Z37 coefficients in lens units (mm).
                for slot in [9, 10] {
                    if self.params.get(&slot).is_some_and(|v| *v != 0.) {
                        return Err(format!(
                            "ZMX surface {surface}: decentered Zernike sag (PARM {slot}) is unsupported"
                        ));
                    }
                }
                let terms = self.params.get(&13).copied().unwrap_or(37.);
                if terms < 0. || terms > 37. || terms.fract() != 0. {
                    return Err(format!(
                        "ZMX surface {surface}: Zernike term count must be an integer in 0..=37"
                    ));
                }
                let normalize = self.params.get(&14).copied().unwrap_or(1.);
                if normalize <= 0. {
                    return Err(format!(
                        "ZMX surface {surface}: Zernike normalization radius must be positive"
                    ));
                }
                let mut coefficients = vec![];
                for slot in 15..15 + terms as usize {
                    coefficients.push(self.params.get(&slot).copied().unwrap_or(0.));
                }
                while coefficients.last() == Some(&0.) {
                    coefficients.pop();
                }
                zernike = Some(SagZernike {
                    normalize,
                    coefficients,
                });
            }
            "COORDBRK" => {
                // Handled separately as a dummy surface carrying CoordinateTransform.
            }
            other => {
                return Err(format!(
                    "ZMX surface {surface}: unsupported surface type {other}"
                ));
            }
        }
        Ok((radius, self.conic, even, odd, toroid, zernike))
    }
}

/// Parse a ZMX sequential prescription into a native system.
pub fn import(text: &str) -> Result<System, String> {
    let mut name = String::new();
    let mut notes: Vec<String> = vec![];
    let mut pupil: Option<Pupil> = None;
    let mut catalogs: Vec<String> = vec![];
    let mut field_type = FieldType::Angle;
    let mut declared_fields: Option<usize> = None;
    let mut declared_waves: Option<usize> = None;
    let mut xs: Vec<f64> = vec![];
    let mut ys: Vec<f64> = vec![];
    let mut field_weights: Vec<f64> = vec![];
    let mut vdx: Vec<f64> = vec![];
    let mut vdy: Vec<f64> = vec![];
    let mut vcx: Vec<f64> = vec![];
    let mut vcy: Vec<f64> = vec![];
    let mut van: Vec<f64> = vec![];
    let mut wave_slots: BTreeMap<usize, (f64, f64)> = BTreeMap::new();
    let mut legacy_waves: Vec<f64> = vec![];
    let mut legacy_weights: Vec<f64> = vec![];
    let mut primary: Option<usize> = None;
    let mut real_aiming = false;
    let mut temperature = 20.;
    let mut pressure = 101325.;
    let mut surfaces: Vec<ZmxSurface> = vec![];
    let mut seen_header = false;
    // Multi-configuration editor rows: (code, surface, 1-based config, value tokens).
    let mut mnum: Option<(usize, Option<usize>)> = None;
    let mut conf_active: Option<usize> = None;
    let mut mce: Vec<(String, usize, usize, Vec<String>)> = vec![];
    for (line_no, raw) in text.trim_start_matches('\u{feff}').lines().enumerate() {
        let tokens: Vec<&str> = raw.split_whitespace().collect();
        if tokens.is_empty() {
            continue;
        }
        let at = |i: usize| tokens.get(i).copied();
        let err = |m: String| format!("ZMX line {}: {m}", line_no + 1);
        let column = |target: &mut Vec<f64>, start: usize| -> Result<(), String> {
            *target = tokens[start..]
                .iter()
                .map(|t| num(Some(t), "column"))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(())
        };
        match tokens[0] {
            "MODE" => {
                if at(1) != Some("SEQ") {
                    return Err(err("only sequential MODE SEQ files are supported".into()));
                }
                seen_header = true;
            }
            "UNIT" => {
                if at(1).map(str::to_uppercase).as_deref() != Some("MM") {
                    return Err(err(
                        "only MM lens units are supported; rescale the file first".into(),
                    ));
                }
            }
            "NAME" => name = tokens[1..].join(" "),
            "NOTE" => notes.push(tokens.get(2..).unwrap_or_default().join(" ")),
            "ENPD" => {
                pupil = Some(Pupil {
                    kind: PupilType::EntrancePupilDiameter,
                    value: num(at(1), "ENPD")?,
                })
            }
            "FNUM" => {
                pupil = Some(Pupil {
                    kind: PupilType::FNumber,
                    value: num(at(1), "FNUM")?,
                })
            }
            "OBNA" => {
                if int(at(2), "OBNA type")? != 0 {
                    return Err(err("object cone angle is unsupported".into()));
                }
                pupil = Some(Pupil {
                    kind: PupilType::ObjectNa,
                    value: num(at(1), "OBNA")?,
                });
            }
            "FLOA" => return Err(err("float-by-stop-size aperture is unsupported".into())),
            "GCAT" => catalogs = tokens[1..].iter().map(|c| c.to_uppercase()).collect(),
            "FTYP" => {
                field_type = match int(at(1), "field type")? {
                    0 => FieldType::Angle,
                    1 => FieldType::ObjectHeight,
                    2 | 3 => FieldType::ImageHeight,
                    _ => return Err(err("unsupported field type".into())),
                };
                if int(at(2), "telecentric flag").unwrap_or(0) != 0 {
                    return Err(err("telecentric object space is unsupported".into()));
                }
                declared_fields = int(at(3), "field count").ok();
                declared_waves = int(at(4), "wavelength count").ok();
                if int(at(7), "afocal flag").unwrap_or(0) != 0 {
                    return Err(err("afocal image space is unsupported".into()));
                }
            }
            "XFLN" | "XFLD" => column(&mut xs, 1)?,
            "YFLN" | "YFLD" => column(&mut ys, 1)?,
            "FWGN" | "FWGT" => column(&mut field_weights, 1)?,
            "VDXN" => column(&mut vdx, 1)?,
            "VDYN" => column(&mut vdy, 1)?,
            "VCXN" => column(&mut vcx, 1)?,
            "VCYN" => column(&mut vcy, 1)?,
            "VANN" => column(&mut van, 1)?,
            "WAVM" => {
                let slot = int(at(1), "wavelength slot")?;
                let value = snap_nm(num(at(2), "wavelength")? * 1000.);
                let weight = if tokens.len() > 3 {
                    num(at(3), "wavelength weight")?
                } else {
                    1.
                };
                wave_slots.insert(slot, (value, weight));
            }
            "WAVL" => column(&mut legacy_waves, 1)?,
            "WWGT" => column(&mut legacy_weights, 1)?,
            "PWAV" => primary = Some(int(at(1), "primary wavelength")?),
            "RAIM" => real_aiming = int(at(1), "ray aiming")? == 2,
            "TEM" => temperature = num(at(1), "temperature")?,
            "PRES" => pressure = num(at(1), "pressure")? * 101325.,
            "SURF" => {
                let index = int(at(1), "surface index")?;
                if index != surfaces.len() {
                    return Err(err(format!(
                        "surface index {index} out of order; expected {}",
                        surfaces.len()
                    )));
                }
                surfaces.push(ZmxSurface {
                    kind: "STANDARD".into(),
                    ..ZmxSurface::default()
                });
            }
            "TYPE" => {
                let surface = surfaces
                    .last_mut()
                    .ok_or_else(|| err("TYPE before any SURF".into()))?;
                surface.kind = at(1)
                    .ok_or_else(|| err("TYPE needs a surface type".into()))?
                    .to_uppercase();
                if !["STANDARD", "EVENASPH", "ODDASPHE", "TOROIDAL", "ZERNFRIN"]
                    .contains(&surface.kind.as_str())
                {
                    return Err(err(format!("unsupported surface type {}", surface.kind)));
                }
            }
            "CURV" => {
                let surface = surfaces
                    .last_mut()
                    .ok_or_else(|| err("CURV before any SURF".into()))?;
                surface.curvature = num(at(1), "CURV")?;
            }
            "DISZ" => {
                let surface = surfaces
                    .last_mut()
                    .ok_or_else(|| err("DISZ before any SURF".into()))?;
                surface.thickness = match at(1) {
                    Some("INFINITY") => Some(f64::INFINITY),
                    _ => Some(num(at(1), "DISZ")?),
                };
            }
            "CONF" => conf_active = Some(int(at(1), "CONF configuration")?),
            "MNUM" => {
                let count = int(at(1), "MNUM configuration count")?;
                if count == 0 || count > 64 {
                    return Err(err("MNUM configuration count outside 1..=64".into()));
                }
                let active = match at(2) {
                    Some(t) => Some(int(Some(t), "MNUM active configuration")?),
                    None => None,
                };
                mnum = Some((count, active));
            }
            "MOFF" => {} // Unused placeholder operand row.
            // Multi-configuration operand rows: CODE surface config value flags...
            "THIC" | "CRVT" | "GLSS" | "SDIA" | "XFIE" | "YFIE" | "WAVE" => {
                let surface = int(at(1), "operand surface")?;
                let config = int(at(2), "operand configuration")?;
                if config == 0 {
                    return Err(err("operand configuration must be 1-based".into()));
                }
                mce.push((
                    tokens[0].to_string(),
                    surface,
                    config,
                    tokens[3..].iter().map(|s| s.to_string()).collect(),
                ));
            }
            // Operand form of CONI (surface CONI lines carry exactly one value).
            "CONI" if tokens.len() > 2 => {
                let surface = int(at(1), "operand surface")?;
                let config = int(at(2), "operand configuration")?;
                if config == 0 {
                    return Err(err("operand configuration must be 1-based".into()));
                }
                mce.push((
                    "CONI".to_string(),
                    surface,
                    config,
                    tokens[3..].iter().map(|s| s.to_string()).collect(),
                ));
            }
            "CONI" => {
                let surface = surfaces
                    .last_mut()
                    .ok_or_else(|| err("CONI before any SURF".into()))?;
                surface.conic = num(at(1), "CONI")?;
            }
            "PARM" => {
                let surface = surfaces
                    .last_mut()
                    .ok_or_else(|| err("PARM before any SURF".into()))?;
                let slot = int(at(1), "PARM slot")?;
                surface.params.insert(slot, num(at(2), "PARM value")?);
            }
            "GLAS" => {
                let surface = surfaces
                    .last_mut()
                    .ok_or_else(|| err("GLAS before any SURF".into()))?;
                let glass = at(1).ok_or_else(|| err("GLAS needs a name".into()))?;
                match glass.to_uppercase().as_str() {
                    "MIRROR" => surface.mirror = true,
                    "MODEL" => {
                        surface.model =
                            Some((num(at(4), "model index")?, num(at(5), "model Abbe")?));
                    }
                    _ => surface.glass = Some(glass.into()),
                }
            }
            "STOP" => {
                let surface = surfaces
                    .last_mut()
                    .ok_or_else(|| err("STOP before any SURF".into()))?;
                if surface.stop {
                    return Err(err("duplicate STOP".into()));
                }
                surface.stop = true;
            }
            "COMM" => {
                if let Some(surface) = surfaces.last_mut() {
                    surface.label = tokens[1..].join(" ");
                }
            }
            "CLAP" => {
                let surface = surfaces
                    .last_mut()
                    .ok_or_else(|| err("CLAP before any SURF".into()))?;
                if int(at(1), "CLAP type")? != 0 {
                    return Err(err("only circular apertures are supported".into()));
                }
                if num(at(2), "CLAP minimum")? != 0. {
                    return Err(err("aperture obscurations are unsupported".into()));
                }
                surface.clip = Some(num(at(3), "CLAP radius")?);
            }
            "OBDC" => {
                if num(at(1), "OBDC x")? != 0. || num(at(2), "OBDC y")? != 0. {
                    return Err(err("decentered apertures are unsupported".into()));
                }
            }
            "FLAP" => return Err(err("floating apertures are unsupported".into())),
            "MCON" => return Err(err("multi-configuration files are unsupported".into())),
            "DIAM" => {} // Draw semi-diameter; native sizes are derived.
            // Metadata with no optical content.
            "VERS" | "DBDT" | "PFIL" | "LANG" | "GFAC" | "SDMA" | "OMMA" | "ROPD" | "HYPR"
            | "PICB" | "GSCH" | "GSTD" | "PUPD" | "PUSH" | "POLS" | "GLRS" | "FIMP" | "MEMA"
            | "ENVD" | "ENPP" | "EXPD" | "EXPP" | "HIDE" | "MIRR" | "SLAB" | "POPS" | "BLNK"
            | "SQPD" | "OPDX" | "DMFS" | "FTIM" | "RSAM" | "DISM" => {}
            other => return Err(err(format!("unsupported ZMX keyword {other}"))),
        }
    }
    if !seen_header {
        return Err("ZMX file is missing the MODE SEQ header".into());
    }
    if surfaces.len() < 2 {
        return Err("ZMX file needs at least object and image surfaces".into());
    }
    let pupil = pupil.ok_or("ZMX file defines no system aperture (ENPD/FNUM/OBNA)")?;
    if !pupil.value.is_finite() || pupil.value <= 0. {
        return Err("ZMX system aperture must be positive".into());
    }
    let take = |column: &[f64]| -> Vec<f64> {
        match declared_fields {
            Some(n) => column.iter().copied().take(n).collect(),
            None => column.to_vec(),
        }
    };
    let (xs, ys) = (take(&xs), take(&ys));
    if xs.len() != ys.len() {
        return Err("ZMX field columns have unequal lengths".into());
    }
    let field_weights = take(&field_weights);
    for (label, column) in [
        ("VDX", take(&vdx)),
        ("VDY", take(&vdy)),
        ("VAN", take(&van)),
    ] {
        if column.iter().any(|v| *v != 0.) {
            return Err(format!(
                "ZMX vignetting {label} decenters/angles have no native representation"
            ));
        }
    }
    let (vcx, vcy) = (take(&vcx), take(&vcy));
    let at_field =
        |column: &[f64], i: usize, default: f64| column.get(i).copied().unwrap_or(default);
    let mut fields = vec![];
    for i in 0..xs.len() {
        fields.push(Field {
            x: xs[i],
            y: ys[i],
            weight: at_field(&field_weights, i, 1.),
            vignetting: [
                at_field(&vcy, i, 0.),
                at_field(&vcy, i, 0.),
                at_field(&vcx, i, 0.),
                at_field(&vcx, i, 0.),
            ],
            color: Field::meridional(0.).color,
        });
    }
    if fields.is_empty() {
        return Err("ZMX file defines no fields".into());
    }
    let mut wavelengths: Vec<(f64, f64)> = if wave_slots.is_empty() {
        legacy_waves
            .iter()
            .enumerate()
            .map(|(i, w)| {
                (
                    snap_nm(w * 1000.),
                    legacy_weights.get(i).copied().unwrap_or(1.),
                )
            })
            .collect()
    } else {
        wave_slots
            .iter()
            .filter(|(slot, _)| declared_waves.is_none_or(|n| **slot <= n))
            .map(|(_, v)| *v)
            .collect()
    };
    if wavelengths.is_empty() {
        return Err("ZMX file defines no wavelengths".into());
    }
    let reference_wavelength = match primary {
        Some(p) if p >= 1 && p <= wavelengths.len() => p - 1,
        _ => 0,
    };
    let resolve = |name: &str| -> Result<String, String> {
        for catalog in &catalogs {
            if let Ok(glass) = material::find(&format!("{}_{}", name.to_uppercase(), catalog)) {
                return Ok(glass.key());
            }
        }
        material::find(name).map(|g| g.key())
    };
    let endpoint = |surface: &ZmxSurface, index: usize| -> Result<EndpointSurface, String> {
        let (radius, conic, even, odd, toroid, zernike) = surface.profile(index)?;
        if toroid.is_some() || zernike.is_some() {
            return Err(format!(
                "ZMX surface {index}: toroidal or Zernike object/image surfaces are unsupported"
            ));
        }
        let mut endpoint = EndpointSurface {
            radius,
            conic,
            even,
            odd,
            ..EndpointSurface::plane()
        };
        if let Some(clip) = surface.clip {
            if clip <= 0. {
                return Err(format!("ZMX surface {index}: aperture must be positive"));
            }
            endpoint.aperture = clip;
            endpoint.clear_aperture = true;
        }
        Ok(endpoint)
    };
    let object = &surfaces[0];
    if object.mirror {
        return Err("ZMX object surface cannot be a mirror".into());
    }
    let object_material = match (&object.glass, object.model) {
        (Some(name), _) => resolve(name).map_err(|e| format!("ZMX object material {name}: {e}"))?,
        (None, Some((nd, vd))) => format!("{nd}:{vd}"),
        (None, None) => "AIR".into(),
    };
    let object_distance = match object.thickness {
        Some(f64::INFINITY) => None,
        Some(distance) => Some(distance),
        None => return Err("ZMX object surface is missing DISZ".into()),
    };
    let mut stop_index = None;
    let mut native = vec![];
    for (i, surface) in surfaces[1..surfaces.len() - 1].iter().enumerate() {
        let (radius, conic, even, odd, toroid, zernike) = surface.profile(i + 1)?;
        let medium = native
            .last()
            .map(|s: &Surface| s.material.clone())
            .unwrap_or_else(|| object_material.clone());
        let (material, interaction, transform) = if surface.kind == "COORDBRK" {
            let tr = CoordinateTransform {
                decenter: [
                    surface.params.get(&1).copied().unwrap_or(0.),
                    surface.params.get(&2).copied().unwrap_or(0.),
                ],
                tilt: [
                    surface.params.get(&3).copied().unwrap_or(0.),
                    surface.params.get(&4).copied().unwrap_or(0.),
                    surface.params.get(&5).copied().unwrap_or(0.),
                ],
                tilt_first: surface.params.get(&6).copied().unwrap_or(0.) != 0.,
                return_after: false,
            };
            (medium, Interaction::Dummy, Some(tr))
        } else if surface.mirror {
            (medium, Interaction::Reflect, None)
        } else {
            match (&surface.glass, surface.model) {
                (Some(name), _) => (
                    resolve(name)
                        .map_err(|e| format!("ZMX surface {} glass {name}: {e}", i + 1))?,
                    Interaction::Refract,
                    None,
                ),
                (None, Some((nd, vd))) => (format!("{nd}:{vd}"), Interaction::Refract, None),
                (None, None) => ("AIR".into(), Interaction::Refract, None),
            }
        };
        let thickness = surface
            .thickness
            .filter(|t| t.is_finite())
            .ok_or_else(|| format!("ZMX surface {}: missing or infinite DISZ", i + 1))?;
        let mut row = Surface::sphere(radius, thickness, &material, surface.clip.unwrap_or(1.));
        row.label = surface.label.clone();
        row.conic = conic;
        row.even = even;
        row.odd = odd;
        row.toroid = toroid;
        row.zernike = zernike;
        row.transform = transform;
        row.interaction = interaction;
        row.clear_aperture = surface.clip.is_some();
        if surface.stop {
            stop_index = Some(i);
        }
        native.push(row);
    }
    let image = surfaces.last().unwrap();
    if image.thickness.is_some_and(|t| t != 0.) {
        return Err("ZMX image surface thickness must be zero or absent".into());
    }
    // Multi-configuration operands. A config-1 row rebases the shared working
    // value; rows of later configurations become overrides where they differ.
    let mut configurations: Vec<Configuration> = vec![];
    let mut active_configuration = 0;
    let mut object_distance = object_distance;
    if !mce.is_empty() || mnum.is_some() {
        let (count, mnum_active) = mnum.ok_or("ZMX operand rows require MNUM")?;
        let active = conf_active.or(mnum_active).unwrap_or(1);
        if active == 0 || active > count {
            return Err("ZMX active configuration outside 1..=MNUM".into());
        }
        configurations = (1..=count)
            .map(|i| Configuration {
                name: format!("Config {i}"),
                overrides: vec![],
            })
            .collect();
        active_configuration = active - 1;
        type OperandRows = Vec<(usize, Vec<String>)>;
        let mut groups: Vec<(String, usize, OperandRows)> = vec![];
        for (code, surface, config, value) in mce {
            if config > count {
                return Err(format!(
                    "ZMX operand {code} references configuration {config} above MNUM {count}"
                ));
            }
            match groups
                .iter_mut()
                .find(|(c, s, _)| *c == code && *s == surface)
            {
                Some((_, _, rows)) => rows.push((config, value)),
                None => groups.push((code, surface, vec![(config, value)])),
            }
        }
        for (code, surface, rows) in groups {
            if surface > native.len() + 1 {
                return Err(format!(
                    "ZMX operand {code} references surface {surface} outside the prescription"
                ));
            }
            match code.as_str() {
                "THIC" => {
                    if surface == 0 {
                        let parse = |v: &[String]| -> Result<Option<f64>, String> {
                            let d = num(v.first().map(String::as_str), "THIC value")?;
                            Ok(if d.abs() >= 1e10 { None } else { Some(d) })
                        };
                        if let Some((_, v)) = rows.iter().find(|(c, _)| *c == 1) {
                            object_distance = parse(v)?;
                        }
                        for (c, v) in &rows {
                            if *c >= 2 {
                                let value = parse(v)?;
                                if value != object_distance {
                                    configurations[*c - 1]
                                        .overrides
                                        .push(Override::ObjectDistance { value });
                                }
                            }
                        }
                    } else if surface > native.len() {
                        for (_, v) in &rows {
                            if num(v.first().map(String::as_str), "THIC value")? != 0. {
                                return Err("ZMX image thickness operand must be zero".into());
                            }
                        }
                    } else {
                        let index = surface - 1;
                        if let Some((_, v)) = rows.iter().find(|(c, _)| *c == 1) {
                            native[index].thickness = num(v.first().map(String::as_str), "THIC")?;
                        }
                        let base = native[index].thickness;
                        for (c, v) in &rows {
                            if *c >= 2 {
                                let value = num(v.first().map(String::as_str), "THIC")?;
                                if value != base {
                                    configurations[*c - 1].overrides.push(Override::Thickness {
                                        surface: index,
                                        value,
                                    });
                                }
                            }
                        }
                    }
                }
                "CRVT" | "CONI" | "SDIA" => {
                    if surface == 0 || surface > native.len() {
                        return Err(format!("ZMX endpoint {code} operands are unsupported"));
                    }
                    let index = surface - 1;
                    let get = |s: &Surface| match code.as_str() {
                        "CRVT" => {
                            if s.radius == 0. {
                                0.
                            } else {
                                1. / s.radius
                            }
                        }
                        "CONI" => s.conic,
                        _ => s.aperture,
                    };
                    let set = |s: &mut Surface, value: f64| match code.as_str() {
                        "CRVT" => {
                            s.radius = if value == 0. { 0. } else { 1. / value };
                        }
                        "CONI" => s.conic = value,
                        _ => s.aperture = value,
                    };
                    if let Some((_, v)) = rows.iter().find(|(c, _)| *c == 1) {
                        let value = num(v.first().map(String::as_str), &code)?;
                        set(&mut native[index], value);
                    }
                    let base = get(&native[index]);
                    for (c, v) in &rows {
                        if *c >= 2 {
                            let value = num(v.first().map(String::as_str), &code)?;
                            if value != base {
                                let operand = match code.as_str() {
                                    "CRVT" => Override::Radius {
                                        surface: index,
                                        value: if value == 0. { 0. } else { 1. / value },
                                    },
                                    "CONI" => Override::Conic {
                                        surface: index,
                                        value,
                                    },
                                    _ => Override::Aperture {
                                        surface: index,
                                        value,
                                    },
                                };
                                configurations[*c - 1].overrides.push(operand);
                            }
                        }
                    }
                }
                "GLSS" => {
                    if surface == 0 || surface > native.len() {
                        return Err("ZMX endpoint GLSS operands are unsupported".into());
                    }
                    let index = surface - 1;
                    let name = |v: &[String]| {
                        v.first()
                            .map(|s| s.trim_matches('"').to_string())
                            .unwrap_or_default()
                    };
                    if let Some((_, v)) = rows.iter().find(|(c, _)| *c == 1) {
                        let glass = name(v);
                        if glass.eq_ignore_ascii_case("mirror") {
                            return Err("ZMX mirror configuration operands are unsupported".into());
                        }
                        native[index].material = resolve(&glass)
                            .map_err(|e| format!("ZMX GLSS operand glass {glass}: {e}"))?;
                    }
                    let base = native[index].material.clone();
                    for (c, v) in &rows {
                        if *c >= 2 {
                            let glass = name(v);
                            if glass.eq_ignore_ascii_case("mirror") {
                                return Err(
                                    "ZMX mirror configuration operands are unsupported".into()
                                );
                            }
                            let material = resolve(&glass)
                                .map_err(|e| format!("ZMX GLSS operand glass {glass}: {e}"))?;
                            if material != base {
                                configurations[*c - 1].overrides.push(Override::Material {
                                    surface: index,
                                    material,
                                });
                            }
                        }
                    }
                }
                "XFIE" | "YFIE" => {
                    let stored = match (fields.get(surface.wrapping_sub(1)), code.as_str()) {
                        (Some(field), "XFIE") => field.x,
                        (Some(field), _) => field.y,
                        (None, _) => {
                            return Err(format!(
                                "ZMX operand {code} references field {surface} outside the specification"
                            ));
                        }
                    };
                    for (_, v) in &rows {
                        if num(v.first().map(String::as_str), &code)? != stored {
                            return Err(
                                "ZMX per-configuration field operands are unsupported".into()
                            );
                        }
                    }
                }
                "WAVE" => {
                    let stored = wavelengths
                        .get(surface.wrapping_sub(1))
                        .ok_or_else(|| {
                            format!(
                                "ZMX operand WAVE references wavelength {surface} outside the specification"
                            )
                        })?
                        .0;
                    for (_, v) in &rows {
                        let value = snap_nm(num(v.first().map(String::as_str), "WAVE")? * 1000.);
                        if value != stored {
                            return Err(
                                "ZMX per-configuration wavelength operands are unsupported".into(),
                            );
                        }
                    }
                }
                _ => unreachable!(),
            }
        }
    }
    let system = System {
        bench: None,
        name,
        note: notes.join("\n"),
        surfaces: native,
        wavelengths: wavelengths
            .drain(..)
            .map(|(value, weight)| Wavelength::new(value, weight))
            .collect(),
        reference_wavelength,
        fields,
        field_type,
        pupil,
        pupil_mapping: if real_aiming {
            PupilMapping::RealStop
        } else {
            PupilMapping::ReferenceEntrancePupil
        },
        object_distance,
        object_material,
        object_label: if object.label.is_empty() {
            "Object".into()
        } else {
            object.label.clone()
        },
        image_label: if image.label.is_empty() {
            "Image".into()
        } else {
            image.label.clone()
        },
        object_surface: endpoint(object, 0)?,
        image_surface: endpoint(image, surfaces.len() - 1)?,
        stop_index: stop_index.ok_or("ZMX file marks no STOP surface")?,
        temperature,
        pressure,
        custom_glasses: vec![],
        configurations,
        active_configuration,
    };
    let mut system = system;
    system.update()?;
    Ok(system)
}

fn f(v: f64) -> String {
    format!("{v}")
}
fn trimmed(coefficients: &[f64]) -> &[f64] {
    let mut end = coefficients.len();
    while end > 0 && coefficients[end - 1] == 0. {
        end -= 1;
    }
    &coefficients[..end]
}

/// Serialize a native system as a ZMX sequential prescription.
pub fn export(s: &System) -> Result<String, String> {
    if s.bench.is_some() {
        return Err("ZMX cannot store the free-space bench; export native JSON".into());
    }
    if s.surfaces
        .iter()
        .any(|surface| !surface.gap_solve.is_fixed())
    {
        return Err("ZMX does not store gap solve relationships; export native JSON".into());
    }
    if !s.custom_glasses.is_empty() {
        return Err("ZMX export cannot embed custom glass catalogs; export native JSON".into());
    }
    if s.pupil.kind == PupilType::ImageNa {
        return Err("ZMX has no image-space NA aperture; use an f-number pupil".into());
    }
    if s.fields.iter().any(|field| {
        field.vignetting[0] != field.vignetting[1] || field.vignetting[2] != field.vignetting[3]
    }) {
        return Err("ZMX stores only symmetric VCX/VCY vignetting; export native JSON".into());
    }
    let glass = |medium: &str, where_: &str| -> Result<Option<String>, String> {
        match where_ {
            _ if medium == "AIR" => Ok(None),
            _ if medium == "FUSED_SILICA" => Err(format!(
                "ZMX catalogs have no FUSED_SILICA entry on {where_}; export native JSON"
            )),
            _ if medium.contains(':') => {
                let (nd, vd) = medium.split_once(':').unwrap();
                Ok(Some(format!("GLAS MODEL 0 0 {nd} {vd}")))
            }
            _ => {
                let glass = material::find(medium)
                    .map_err(|e| format!("ZMX export {where_} material {medium}: {e}"))?;
                Ok(Some(format!(
                    "GLAS {} 0 0 {} {}",
                    glass.name,
                    f(glass.nd),
                    f(glass.vd)
                )))
            }
        }
    };
    let profile = |even: &[f64],
                   odd: &[f64],
                   conic: f64,
                   curvature: f64,
                   toroid: Option<&Toroid>,
                   zernike: Option<&SagZernike>|
     -> Result<String, String> {
        let (even, odd) = (trimmed(even), trimmed(odd));
        if even.len() > 7 {
            return Err("ZMX EVENASPH stores at most seven r^4..r^16 coefficients".into());
        }
        if odd.len() > 6 {
            return Err("ZMX ODDASPHE stores at most six r^3..r^8 coefficients".into());
        }
        if toroid.is_some() && zernike.is_some() {
            return Err(
                "ZMX cannot combine toroidal and Zernike sag on one surface; export native JSON"
                    .into(),
            );
        }
        let mut out = String::new();
        if let Some(t) = toroid {
            let even_y = trimmed(&t.even_y);
            if even_y.len() > 6 {
                return Err("ZMX TOROIDAL stores at most six y^4..y^14 coefficients".into());
            }
            out += "  TYPE TOROIDAL\n";
            let _ = writeln!(out, "  PARM 1 {}", f(t.revolution));
            for (i, a) in even_y.iter().enumerate() {
                if *a != 0. {
                    let _ = writeln!(out, "  PARM {} {}", i + 3, f(*a));
                }
            }
        } else if let Some(z) = zernike {
            if !even.is_empty() || !odd.is_empty() {
                return Err(
                    "ZMX cannot combine asphere coefficients with Zernike sag; export native JSON"
                        .into(),
                );
            }
            let coefficients = trimmed(&z.coefficients);
            out += "  TYPE ZERNFRIN\n";
            let _ = writeln!(out, "  PARM 13 {}", coefficients.len());
            let _ = writeln!(out, "  PARM 14 {}", f(z.normalize));
            for (i, a) in coefficients.iter().enumerate() {
                if *a != 0. {
                    let _ = writeln!(out, "  PARM {} {}", i + 15, f(*a));
                }
            }
        } else if !odd.is_empty() {
            out += "  TYPE ODDASPHE\n";
            for (i, a) in odd.iter().enumerate() {
                if *a != 0. {
                    let _ = writeln!(out, "  PARM {} {}", i + 3, f(*a));
                }
            }
        } else if !even.is_empty() {
            out += "  TYPE EVENASPH\n";
            for (i, a) in even.iter().enumerate() {
                if *a != 0. {
                    let _ = writeln!(out, "  PARM {} {}", i + 2, f(*a));
                }
            }
        } else {
            out += "  TYPE STANDARD\n";
        }
        let _ = writeln!(out, "  CURV {curvature}");
        if conic != 0. {
            let _ = writeln!(out, "  CONI {}", f(conic));
        }
        Ok(out)
    };
    let mut out = String::new();
    out += "VERS 26102\nMODE SEQ\n";
    let _ = writeln!(out, "NAME {}", s.name);
    if !s.note.is_empty() {
        for (i, line) in s.note.lines().enumerate() {
            let _ = writeln!(out, "NOTE {i} {line}");
        }
    }
    out += "UNIT MM X W X CM MR CPMM\n";
    let mut catalogs = vec![];
    for medium in std::iter::once(&s.object_material)
        .chain(s.surfaces.iter().map(|surface| &surface.material))
    {
        if medium != "AIR" && medium != "FUSED_SILICA" && !medium.contains(':') {
            let catalog = material::find(medium)
                .map_err(|e| format!("ZMX export material {medium}: {e}"))?
                .catalog
                .clone();
            if !catalogs.contains(&catalog) {
                catalogs.push(catalog);
            }
        }
    }
    if !catalogs.is_empty() {
        let _ = writeln!(out, "GCAT {}", catalogs.join(" "));
    }
    match s.pupil.kind {
        PupilType::EntrancePupilDiameter => {
            let _ = writeln!(out, "ENPD {}", f(s.pupil.value));
        }
        PupilType::FNumber => {
            let _ = writeln!(out, "FNUM {} 0", f(s.pupil.value));
        }
        PupilType::ObjectNa => {
            let _ = writeln!(out, "OBNA {} 0", f(s.pupil.value));
        }
        PupilType::ImageNa => unreachable!(),
    }
    let _ = writeln!(
        out,
        "RAIM {} 0",
        if s.pupil_mapping == PupilMapping::RealStop {
            2
        } else {
            0
        }
    );
    let _ = writeln!(
        out,
        "FTYP {} 0 {} {} 0 0 0",
        match s.field_type {
            FieldType::Angle => 0,
            FieldType::ObjectHeight => 1,
            FieldType::ImageHeight => 2,
        },
        s.fields.len(),
        s.wavelengths.len()
    );
    let column = |values: &[f64]| values.iter().map(|v| f(*v)).collect::<Vec<_>>().join(" ");
    let _ = writeln!(
        out,
        "XFLN {}",
        column(&s.fields.iter().map(|f| f.x).collect::<Vec<_>>())
    );
    let _ = writeln!(
        out,
        "YFLN {}",
        column(&s.fields.iter().map(|f| f.y).collect::<Vec<_>>())
    );
    let _ = writeln!(
        out,
        "FWGN {}",
        column(&s.fields.iter().map(|f| f.weight).collect::<Vec<_>>())
    );
    if s.fields
        .iter()
        .any(|f| f.vignetting.iter().any(|v| *v != 0.))
    {
        let _ = writeln!(
            out,
            "VCYN {}",
            column(&s.fields.iter().map(|f| f.vignetting[0]).collect::<Vec<_>>())
        );
        let _ = writeln!(
            out,
            "VCXN {}",
            column(&s.fields.iter().map(|f| f.vignetting[2]).collect::<Vec<_>>())
        );
    }
    for (i, wave) in s.wavelengths.iter().enumerate() {
        let um = ((wave.value / 1000.) * 1e12).round() / 1e12;
        let _ = writeln!(out, "WAVM {} {} {}", i + 1, f(um), f(wave.weight));
    }
    let _ = writeln!(out, "PWAV {}", s.reference_wavelength + 1);
    let _ = writeln!(out, "TEM {}", f(s.temperature));
    let _ = writeln!(out, "PRES {}", f(s.pressure / 101325.));
    let endpoint_block = |endpoint: &EndpointSurface, label: &str| -> Result<String, String> {
        let mut block = profile(
            &endpoint.even,
            &endpoint.odd,
            endpoint.conic,
            if endpoint.radius == 0. {
                0.
            } else {
                1. / endpoint.radius
            },
            None,
            None,
        )?;
        if !label.is_empty() {
            let _ = writeln!(block, "  COMM {label}");
        }
        if endpoint.clear_aperture {
            let _ = writeln!(block, "  CLAP 0 0 {}", f(endpoint.aperture));
        }
        Ok(block)
    };
    out += "SURF 0\n";
    out += &endpoint_block(&s.object_surface, &s.object_label)?;
    match s.object_distance {
        Some(distance) => {
            let _ = writeln!(out, "  DISZ {}", f(distance));
        }
        None => out += "  DISZ INFINITY\n",
    }
    if let Some(line) = glass(&s.object_material, "object")? {
        let _ = writeln!(out, "  {line}");
    }
    for (i, surface) in s.surfaces.iter().enumerate() {
        let _ = writeln!(out, "SURF {}", i + 1);
        if i == s.stop_index {
            out += "  STOP\n";
        }
        if let Some(tr) = &surface.transform {
            if surface.interaction != Interaction::Dummy
                || tr.return_after
                || !surface.profile().is_plane()
            {
                return Err(
                    "ZMX export supports coordinate transforms only on persistent plane COORDBRK dummy surfaces; export native JSON"
                        .into(),
                );
            }
            out += "  TYPE COORDBRK\n";
            if tr.decenter[0] != 0. {
                let _ = writeln!(out, "  PARM 1 {}", f(tr.decenter[0]));
            }
            if tr.decenter[1] != 0. {
                let _ = writeln!(out, "  PARM 2 {}", f(tr.decenter[1]));
            }
            if tr.tilt[0] != 0. {
                let _ = writeln!(out, "  PARM 3 {}", f(tr.tilt[0]));
            }
            if tr.tilt[1] != 0. {
                let _ = writeln!(out, "  PARM 4 {}", f(tr.tilt[1]));
            }
            if tr.tilt[2] != 0. {
                let _ = writeln!(out, "  PARM 5 {}", f(tr.tilt[2]));
            }
            if tr.tilt_first {
                out += "  PARM 6 1\n";
            }
        } else {
            out += &profile(
                &surface.even,
                &surface.odd,
                surface.conic,
                if surface.radius == 0. {
                    0.
                } else {
                    1. / surface.radius
                },
                surface.toroid.as_ref(),
                surface.zernike.as_ref(),
            )?;
            match surface.interaction {
                Interaction::Reflect => out += "  GLAS MIRROR\n",
                Interaction::Refract => {
                    if let Some(line) = glass(&surface.material, "surface")? {
                        let _ = writeln!(out, "  {line}");
                    }
                }
                Interaction::Dummy => {
                    return Err(
                        "ZMX has no dummy surfaces; remove them or export native JSON".into(),
                    );
                }
            }
        }
        if !surface.label.is_empty() {
            let _ = writeln!(out, "  COMM {}", surface.label);
        }
        let _ = writeln!(out, "  DISZ {}", f(surface.thickness));
        if surface.clear_aperture {
            let _ = writeln!(out, "  CLAP 0 0 {}", f(surface.aperture));
        }
        let _ = writeln!(out, "  DIAM {} 0 0 0 1 \"\"", f(surface.semi_diameter));
    }
    let _ = writeln!(out, "SURF {}", s.surfaces.len() + 1);
    out += &endpoint_block(&s.image_surface, &s.image_label)?;
    if !s.configurations.is_empty() {
        // Multi-configuration editor: one row per operand per configuration;
        // columns not covered by an override take the base working value.
        let _ = writeln!(out, "CONF {} 0 0 0 0 0 0 0 0 0", s.active_configuration + 1);
        let _ = writeln!(
            out,
            "MNUM {} {}",
            s.configurations.len(),
            s.active_configuration + 1
        );
        // Group overrides by (kind, surface) in first-appearance order.
        let mut groups: Vec<(&str, usize)> = vec![];
        for configuration in &s.configurations {
            for operand in &configuration.overrides {
                let (kind, surface) = match operand {
                    Override::Thickness { surface, .. } => ("THIC", *surface),
                    Override::Radius { surface, .. } => ("CRVT", *surface),
                    Override::Conic { surface, .. } => ("CONI", *surface),
                    Override::Aperture { surface, .. } => ("SDIA", *surface),
                    Override::Material { surface, .. } => ("GLSS", *surface),
                    Override::ObjectDistance { .. } => ("THIC", usize::MAX),
                };
                if !groups.contains(&(kind, surface)) {
                    groups.push((kind, surface));
                }
            }
        }
        let flags = "0 0 0 1 1 1.000000000000E+000 0.000000000000E+000";
        for (kind, surface) in groups {
            for (c, configuration) in s.configurations.iter().enumerate() {
                let covered =
                    configuration
                        .overrides
                        .iter()
                        .find(|operand| match (kind, operand) {
                            ("THIC", Override::Thickness { surface: s2, .. }) => *s2 == surface,
                            ("CRVT", Override::Radius { surface: s2, .. }) => *s2 == surface,
                            ("CONI", Override::Conic { surface: s2, .. }) => *s2 == surface,
                            ("SDIA", Override::Aperture { surface: s2, .. }) => *s2 == surface,
                            ("GLSS", Override::Material { surface: s2, .. }) => *s2 == surface,
                            ("THIC", Override::ObjectDistance { .. }) => surface == usize::MAX,
                            _ => false,
                        });
                match (kind, covered) {
                    ("GLSS", _) => {
                        let medium = match covered {
                            Some(Override::Material { material, .. }) => material.clone(),
                            _ => s.surfaces[surface].material.clone(),
                        };
                        if medium.contains(':') {
                            return Err(
                                "ZMX GLSS operands cannot name model glasses; export native JSON"
                                    .into(),
                            );
                        }
                        let name = material::find(&medium)
                            .map_err(|e| format!("ZMX GLSS operand material {medium}: {e}"))?
                            .name
                            .clone();
                        let _ = writeln!(out, "GLSS {} {} {}", surface + 1, c + 1, name);
                    }
                    ("THIC", _) if surface == usize::MAX => {
                        let distance = match covered {
                            Some(Override::ObjectDistance { value }) => *value,
                            _ => s.object_distance,
                        };
                        let value = distance.unwrap_or(1e10);
                        let _ = writeln!(out, "THIC 0 {} {} {}", c + 1, f(value), flags);
                    }
                    _ => {
                        let value = match covered {
                            Some(Override::Thickness { value, .. })
                            | Some(Override::Conic { value, .. })
                            | Some(Override::Aperture { value, .. }) => *value,
                            Some(Override::Radius { value, .. }) => {
                                if *value == 0. {
                                    0.
                                } else {
                                    1. / value
                                }
                            }
                            _ => {
                                let base = &s.surfaces[surface];
                                match kind {
                                    "THIC" => base.thickness,
                                    "CRVT" => {
                                        if base.radius == 0. {
                                            0.
                                        } else {
                                            1. / base.radius
                                        }
                                    }
                                    "CONI" => base.conic,
                                    _ => base.aperture,
                                }
                            }
                        };
                        let _ = writeln!(
                            out,
                            "{kind} {} {} {} {}",
                            surface + 1,
                            c + 1,
                            f(value),
                            flags
                        );
                    }
                }
            }
        }
        out += "MOFF   0   1 \"\" 0 0 0 1 1 0 0.0 \"\" 0\n";
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(s: &System) -> System {
        let text = export(s).unwrap();
        import(&text).unwrap_or_else(|e| panic!("{e}\n{text}"))
    }
    /// ZMX stores no field/wavelength colors; compare everything else.
    fn without_colors(s: &System) -> serde_json::Value {
        let mut v = serde_json::to_value(s).unwrap();
        for f in v["fields"].as_array_mut().unwrap() {
            f["color"] = serde_json::json!(Field::meridional(0.).color);
        }
        v
    }

    #[test]
    fn default_singlet_round_trips_exactly() {
        let mut s = System::default();
        s.update().unwrap();
        let restored = round_trip(&s);
        assert_eq!(without_colors(&restored), without_colors(&s));
        let efl = |s: &System| s.paraxial(s.reference()).unwrap()["efl"].as_f64().unwrap();
        assert!((efl(&restored) - efl(&s)).abs() < 1e-9);
    }

    #[test]
    fn finite_object_asphere_and_model_glass_round_trip() {
        let mut s = System {
            object_distance: Some(300.),
            fields: vec![Field {
                x: 1.,
                ..Field::meridional(2.)
            }],
            field_type: FieldType::ObjectHeight,
            pupil: Pupil {
                kind: PupilType::FNumber,
                value: 4.,
            },
            ..Default::default()
        };
        s.surfaces[0].conic = -0.5;
        s.surfaces[0].even = vec![1e-5, -2e-8];
        s.surfaces[1].odd = vec![0., 1e-7];
        s.surfaces[1].material = "1.691:54.71".into();
        s.object_surface.radius = -1000.;
        s.image_surface.radius = 200.;
        s.image_surface.clear_aperture = true;
        s.image_surface.aperture = 8.;
        s.fields[0].vignetting = [0.1, 0.1, 0.2, 0.2];
        s.update().unwrap();
        let restored = round_trip(&s);
        assert_eq!(without_colors(&restored), without_colors(&s));
    }

    #[test]
    fn mirror_system_round_trip_with_mended_medium() {
        let mut s = System {
            surfaces: vec![
                Surface::sphere(-100., -30., "AIR", 20.),
                Surface::sphere(0., 30., "AIR", 20.),
            ],
            ..Default::default()
        };
        s.surfaces[0].interaction = Interaction::Reflect;
        s.surfaces[0].conic = -1.;
        s.update().unwrap();
        let text = export(&s).unwrap();
        assert!(text.contains("GLAS MIRROR"));
        let restored = import(&text).unwrap();
        assert_eq!(restored.surfaces[0].interaction, Interaction::Reflect);
        assert_eq!(restored.surfaces[0].material, "AIR");
        assert_eq!(without_colors(&restored), without_colors(&s));
    }

    #[test]
    fn imports_minimal_zmx_singlet() {
        let text = "MODE SEQ\nNAME Test singlet\nUNIT MM X W X CM MR CPMM\nGCAT SCHOTT\nENPD 10\nFTYP 0 0 2 1 0 0 0\nXFLN 0 0\nYFLN 0 3\nFWGN 1 1\nWAVM 1 0.5875618 1\nPWAV 1\nSURF 0\n  CURV 0\n  DISZ INFINITY\nSURF 1\n  STOP\n  TYPE STANDARD\n  CURV 0.02\n  GLAS N-BK7 0 0 1.5168 64.17\n  DISZ 5\n  CLAP 0 0 6\nSURF 2\n  CURV -0.02\n  DISZ 46.64\nSURF 3\n  CURV 0\n";
        let s = import(text).unwrap();
        assert_eq!(s.name, "Test singlet");
        assert_eq!(s.surfaces.len(), 2);
        assert_eq!(s.surfaces[0].material, "N-BK7_SCHOTT");
        assert_eq!(s.surfaces[0].radius, 50.);
        assert_eq!(s.stop_index, 0);
        assert!(s.object_distance.is_none());
        assert_eq!(s.wavelengths[0].value, 587.5618);
        assert_eq!(s.fields[1].y, 3.);
        assert!(s.surfaces[0].clear_aperture);
        assert_eq!(s.surfaces[0].aperture, 6.);
        assert!(!s.surfaces[1].clear_aperture);
        let efl = s.paraxial(s.reference()).unwrap()["efl"].as_f64().unwrap();
        assert!((efl - 50.).abs() < 1.);
    }

    #[test]
    fn unsupported_constructs_error_explicitly() {
        let frame = |surface_block: &str, header: &str| {
            format!(
                "MODE SEQ\n{header}ENPD 10\nFTYP 0 0 1 1 0 0 0\nXFLN 0\nYFLN 0\nWAVM 1 0.5875618 1\nSURF 0\n  DISZ INFINITY\nSURF 1\n  STOP\n{surface_block}  DISZ 10\nSURF 2\n"
            )
        };
        let mm = "UNIT MM X W X CM MR CPMM\n";
        for (text, fragment) in [
            ("MODE NSQ\n".to_string(), "MODE SEQ"),
            (frame("  CURV 0.01\n", "UNIT CM X W X CM MR CPMM\n"), "MM"),
            (
                frame("  CURV 0.01\n", &format!("{mm}FTYP 4 0 1 1 0 0 0\n")),
                "field type",
            ),
            (
                frame("  TYPE TOROIDAL\n  PARM 2 1e-4\n  CURV 0.01\n", mm),
                "y^2",
            ),
            (
                frame("  TYPE ZERNFRIN\n  PARM 9 0.5\n  CURV 0.01\n", mm),
                "decentered Zernike",
            ),
            (
                frame(
                    "  TYPE ZERNFRIN\n  PARM 13 4\n  PARM 14 -2\n  CURV 0.01\n",
                    mm,
                ),
                "normalization",
            ),
            (
                frame("  TYPE EVENASPH\n  PARM 1 1e-4\n  CURV 0.01\n", mm),
                "r^2",
            ),
            (
                frame("  TYPE ODDASPHE\n  PARM 2 1e-4\n  CURV 0.01\n", mm),
                "r^2",
            ),
            (frame("  CURV 0.01\n  CLAP 1 0 5\n", mm), "circular"),
            (frame("  CURV 0.01\n  CLAP 0 1 5\n", mm), "obscuration"),
            (frame("  CURV 0.01\n  OBDC 1 0\n", mm), "decentered"),
            (
                frame("  CURV 0.01\n  GLAS NO_SUCH_GLASS\n", mm),
                "NO_SUCH_GLASS",
            ),
        ] {
            let error = import(&text).unwrap_err();
            assert!(
                error.contains(fragment),
                "expected {fragment}: {error}\n{text}"
            );
        }
        let mut stop_less = System::default();
        stop_less.update().unwrap();
        let text = export(&stop_less).unwrap().replace("  STOP\n", "");
        assert!(import(&text).unwrap_err().contains("STOP"));
        assert!(export(&stop_less).is_ok());
        stop_less.pupil.kind = PupilType::ImageNa;
        assert!(export(&stop_less).unwrap_err().contains("image-space NA"));
        stop_less.pupil.kind = PupilType::EntrancePupilDiameter;
        stop_less.fields[0].vignetting = [0.1, 0.2, 0., 0.];
        assert!(export(&stop_less).unwrap_err().contains("symmetric"));
        stop_less.fields[0].vignetting = [0.; 4];
        stop_less.surfaces[0].interaction = Interaction::Dummy;
        assert!(export(&stop_less).unwrap_err().contains("dummy"));
    }
    #[test]
    fn multi_configuration_operands_round_trip() {
        let mut s = System {
            configurations: vec![
                Configuration {
                    name: "Config 1".into(),
                    overrides: vec![],
                },
                Configuration {
                    name: "Config 2".into(),
                    overrides: vec![
                        Override::Thickness {
                            surface: 1,
                            value: 20.,
                        },
                        Override::Radius {
                            surface: 0,
                            value: -80.,
                        },
                        Override::Conic {
                            surface: 0,
                            value: -0.5,
                        },
                        Override::Material {
                            surface: 0,
                            material: "N-SF6_SCHOTT".into(),
                        },
                        Override::Aperture {
                            surface: 1,
                            value: 8.,
                        },
                    ],
                },
                Configuration {
                    name: "Config 3".into(),
                    overrides: vec![Override::ObjectDistance { value: Some(500.) }],
                },
            ],
            active_configuration: 1,
            ..Default::default()
        };
        s.update().unwrap();
        let text = export(&s).unwrap();
        assert!(text.contains("CONF 2 0 0 0 0 0 0 0 0 0"));
        assert!(text.contains("MNUM 3 2"));
        assert!(text.contains("THIC 2 2 20"));
        assert!(text.contains("CRVT 1 2 -0.0125"));
        assert!(text.contains("CONI 1 2 -0.5"));
        assert!(text.contains("GLSS 1 2 N-SF6"));
        assert!(text.contains("SDIA 2 2 8"));
        assert!(text.contains("THIC 0 3 500"));
        let back = import(&text).unwrap();
        assert_eq!(back.configurations.len(), 3);
        assert_eq!(back.active_configuration, 1);
        let two = &back.configurations[1].overrides;
        assert!(two.contains(&Override::Thickness {
            surface: 1,
            value: 20.
        }));
        assert!(two.contains(&Override::Radius {
            surface: 0,
            value: -80.
        }));
        assert!(two.contains(&Override::Conic {
            surface: 0,
            value: -0.5
        }));
        assert!(two.contains(&Override::Aperture {
            surface: 1,
            value: 8.
        }));
        assert!(matches!(
            two.iter().find(|o| matches!(o, Override::Material { .. })),
            Some(Override::Material { surface: 0, material }) if material == "N-SF6_SCHOTT"
        ));
        assert_eq!(
            back.configurations[2].overrides,
            vec![Override::ObjectDistance { value: Some(500.) }]
        );
        // Every configuration resolves and traces identically on reimport.
        for index in 0..3 {
            let (mut a, mut b) = (
                s.configured(index).unwrap(),
                back.configured(index).unwrap(),
            );
            a.update().unwrap();
            b.update().unwrap();
            let ray = |s: &System| s.trace_field(1., -2., &s.fields[0], s.reference()).unwrap();
            assert_eq!(ray(&a).image(), ray(&b).image(), "config {index}");
        }
        // Config 2's effective system carries the overrides.
        let resolved = back.configured(1).unwrap();
        assert_eq!(resolved.surfaces[1].thickness, 20.);
        assert_eq!(resolved.surfaces[0].radius, -80.);
        assert_eq!(resolved.surfaces[0].conic, -0.5);
        assert_eq!(resolved.surfaces[0].material, "N-SF6_SCHOTT");
        assert_eq!(resolved.surfaces[1].aperture, 8.);
        // Operand rows above MNUM and unsupported operand kinds fail clearly.
        assert!(
            import(&text.replace("MNUM 3 2", "MNUM 2 2"))
                .unwrap_err()
                .contains("above MNUM")
        );
        assert!(
            import(&text.replace("CONI 1 2 -0.5", "PAR3 1 2 -0.5"))
                .unwrap_err()
                .contains("PAR3")
        );
        assert!(
            import(&text.replace("THIC 0 3 500", "THIC 0 3 500\nTEMP 1 3 30 0 0"))
                .unwrap_err()
                .contains("TEMP")
        );
    }
    #[test]
    fn toroidal_and_zernike_profiles_roundtrip() {
        let mut s = System::default();
        s.surfaces[0].toroid = Some(Toroid {
            revolution: 30.,
            even_y: vec![1e-4, -2e-6],
        });
        s.surfaces[1].zernike = Some(SagZernike {
            normalize: 12.5,
            coefficients: vec![0.2, 0., -1e-3, 0., 0., 0., 0., 0., 0., 0., 5e-4],
        });
        s.update().unwrap();
        let text = export(&s).unwrap();
        assert!(text.contains("TYPE TOROIDAL"));
        assert!(text.contains("PARM 1 30"));
        assert!(text.contains("TYPE ZERNFRIN"));
        assert!(text.contains("PARM 13 11"));
        assert!(text.contains("PARM 14 12.5"));
        let back = import(&text).unwrap();
        let t = back.surfaces[0].toroid.as_ref().unwrap();
        assert_eq!(t.revolution, 30.);
        assert_eq!(t.even_y, vec![1e-4, -2e-6]);
        let z = back.surfaces[1].zernike.as_ref().unwrap();
        assert_eq!(z.normalize, 12.5);
        assert_eq!(
            z.coefficients,
            vec![0.2, 0., -1e-3, 0., 0., 0., 0., 0., 0., 0., 5e-4]
        );
        // A traced ray through the reimported system lands identically.
        let ray = |s: &System| s.trace_field(1., -2., &s.fields[0], s.reference()).unwrap();
        assert_eq!(ray(&s).image(), ray(&back).image());
        // Both profile kinds on one surface have no ZMX representation.
        s.surfaces[0].zernike = s.surfaces[1].zernike.clone();
        assert!(export(&s).unwrap_err().contains("native JSON"));
        s.surfaces[0].zernike = None;
        s.surfaces[0].toroid.as_mut().unwrap().even_y = vec![1e-4; 7];
        assert!(export(&s).unwrap_err().contains("TOROIDAL"));
        s.surfaces[0].toroid = None;
        s.surfaces[1].even = vec![1e-5];
        assert!(export(&s).unwrap_err().contains("native JSON"));
    }
}
