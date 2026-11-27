// SPDX-License-Identifier: GPL-3.0-or-later
// Saved prescription sections and their text share one set of report cells.
use super::*;
use std::fmt::Write;

#[derive(schemars::JsonSchema, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SectionId {
    General,
    Specification,
    FirstOrder,
    LensData,
    SurfaceData,
}
impl SectionId {
    fn title(self) -> &'static str {
        match self {
            Self::General => "General information",
            Self::Specification => "Optical specification",
            Self::FirstOrder => "First-order data",
            Self::LensData => "Lens data",
            Self::SurfaceData => "Surface profiles",
        }
    }
}
#[derive(schemars::JsonSchema, Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Cell {
    Number { value: f64, text: String },
    Text { text: String },
    Infinite { sign: i8, text: String },
    NotApplicable { reason: String, text: String },
    Unavailable { reason: String, text: String },
}
impl Cell {
    fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }
    fn number(value: f64, precision: usize) -> Self {
        if !value.is_finite() {
            return Self::unavailable("Computed value is not finite");
        }
        let text = if value != 0.
            && (value.abs() < 10_f64.powi(-(precision as i32)) || value.abs() >= 1e9)
        {
            format!("{value:.precision$e}")
        } else {
            format!("{value:.precision$}")
        };
        Self::Number { value, text }
    }
    fn result(value: Result<f64, String>, precision: usize) -> Self {
        match value {
            Ok(v) => Self::number(v, precision),
            Err(e) => Self::unavailable(e),
        }
    }
    fn unavailable(reason: impl Into<String>) -> Self {
        Self::Unavailable {
            reason: reason.into(),
            text: "Unavailable".into(),
        }
    }
    fn na(reason: impl Into<String>) -> Self {
        Self::NotApplicable {
            reason: reason.into(),
            text: "—".into(),
        }
    }
    fn infinite(sign: i8) -> Self {
        Self::Infinite {
            sign,
            text: if sign < 0 { "-Infinity" } else { "Infinity" }.into(),
        }
    }
    fn display(&self) -> &str {
        match self {
            Self::Number { text, .. }
            | Self::Text { text }
            | Self::Infinite { text, .. }
            | Self::NotApplicable { text, .. }
            | Self::Unavailable { text, .. } => text,
        }
    }
}
#[derive(schemars::JsonSchema, Serialize)]
pub struct Column {
    pub id: String,
    pub label: String,
    pub unit: Option<String>,
}
#[derive(schemars::JsonSchema, Serialize)]
pub struct Row {
    pub id: String,
    pub cells: Vec<Cell>,
}
#[derive(schemars::JsonSchema, Serialize)]
pub struct Table {
    pub id: String,
    pub title: String,
    pub columns: Vec<Column>,
    pub rows: Vec<Row>,
}
#[derive(schemars::JsonSchema, Serialize)]
pub struct Section {
    pub id: SectionId,
    pub title: String,
    pub paragraphs: Vec<String>,
    pub tables: Vec<Table>,
}
#[derive(schemars::JsonSchema, Serialize)]
pub struct Diagnostic {
    pub section: SectionId,
    pub table: String,
    pub row: String,
    pub column: String,
    pub message: String,
}
#[derive(schemars::JsonSchema, Serialize)]
pub struct PrescriptionReport {
    pub revision: u64,
    pub name: String,
    pub wavelength: f64,
    pub precision: usize,
    pub sections: Vec<Section>,
    pub diagnostics: Vec<Diagnostic>,
    pub text: String,
}
fn row(id: impl Into<String>, cells: Vec<Cell>) -> Row {
    Row {
        id: id.into(),
        cells,
    }
}
fn table(id: &str, title: &str, columns: &[(&str, &str, Option<&str>)], rows: Vec<Row>) -> Table {
    Table {
        id: id.into(),
        title: title.into(),
        columns: columns
            .iter()
            .map(|(id, label, unit)| Column {
                id: (*id).into(),
                label: (*label).into(),
                unit: unit.map(String::from),
            })
            .collect(),
        rows,
    }
}
fn metrics(id: &str, title: &str, rows: Vec<Row>) -> Table {
    table(
        id,
        title,
        &[
            ("quantity", "Quantity", None),
            ("value", "Value", None),
            ("unit", "Unit", None),
        ],
        rows,
    )
}
fn metric(id: &str, label: &str, value: Cell, unit: &str) -> Row {
    row(id, vec![Cell::text(label), value, Cell::text(unit)])
}
fn section(id: SectionId, paragraphs: Vec<String>, tables: Vec<Table>) -> Section {
    Section {
        id,
        title: id.title().into(),
        paragraphs,
        tables,
    }
}
fn num(value: f64, p: usize) -> Cell {
    Cell::number(value, p)
}
fn radius(value: f64, p: usize) -> Cell {
    if value == 0. {
        Cell::infinite(1)
    } else {
        num(value, p)
    }
}
fn enum_text<T: Serialize>(value: &T) -> Cell {
    Cell::text(serde_json::to_value(value).unwrap().as_str().unwrap())
}

fn general(s: &System, p: usize) -> Section {
    let mut paragraphs = vec![format!("Note:\n{}", s.note)];
    let mut tables = vec![metrics(
        "general",
        "System information",
        vec![
            metric("name", "Title", Cell::text(&s.name), ""),
            metric(
                "object_label",
                "Object label",
                Cell::text(&s.object_label),
                "",
            ),
            metric("image_label", "Image label", Cell::text(&s.image_label), ""),
            metric("temperature", "Temperature", num(s.temperature, p), "C"),
            metric("pressure", "Pressure", num(s.pressure, p), "Pa"),
            metric(
                "custom_glasses",
                "Project glass entries",
                num(s.custom_glasses.len() as f64, 0),
                "",
            ),
        ],
    )];
    if !s.configurations.is_empty() {
        paragraphs.push("Configuration surfaces use the same one-based numbering as the aperture stop; override values replace the shared base row in the resolved prescription. Every other section of this report evaluates the active configuration.".into());
        let configurations = s
            .configurations
            .iter()
            .enumerate()
            .map(|(i, c)| {
                row(
                    format!("configuration_{i}"),
                    vec![
                        num((i + 1) as f64, 0),
                        Cell::text(&c.name),
                        Cell::text(if i == s.active_configuration {
                            "Yes"
                        } else {
                            "No"
                        }),
                        num(c.overrides.len() as f64, 0),
                    ],
                )
            })
            .collect();
        tables.push(table(
            "configurations",
            "Configurations",
            &[
                ("index", "Entry", None),
                ("name", "Name", None),
                ("active", "Active", None),
                ("overrides", "Override count", None),
            ],
            configurations,
        ));
        let mut overrides = vec![];
        for (i, c) in s.configurations.iter().enumerate() {
            for (j, o) in c.overrides.iter().enumerate() {
                let surface = |k: usize| num((k + 1) as f64, 0);
                let (kind, target, value) = match o {
                    Override::Thickness { surface: k, value } => {
                        ("thickness", surface(*k), num(*value, p))
                    }
                    Override::Radius { surface: k, value } => {
                        ("radius", surface(*k), radius(*value, p))
                    }
                    Override::Conic { surface: k, value } => ("conic", surface(*k), num(*value, p)),
                    Override::Aperture { surface: k, value } => {
                        ("aperture", surface(*k), num(*value, p))
                    }
                    Override::Material {
                        surface: k,
                        material,
                    } => ("material", surface(*k), Cell::text(material)),
                    Override::ObjectDistance { value } => (
                        "object_distance",
                        Cell::na("Shared object vertex"),
                        value.map_or(Cell::infinite(1), |v| num(v, p)),
                    ),
                };
                overrides.push(row(
                    format!("override_{i}_{j}"),
                    vec![num((i + 1) as f64, 0), Cell::text(kind), target, value],
                ));
            }
        }
        tables.push(table(
            "configuration_overrides",
            "Configuration overrides",
            &[
                ("configuration", "Configuration", None),
                ("kind", "Operand", None),
                ("surface", "Surface", None),
                ("value", "Value", None),
            ],
            overrides,
        ));
    }
    section(SectionId::General, paragraphs, tables)
}
fn specification(s: &System, p: usize) -> Section {
    let rows = vec![
        metric(
            "pupil_kind",
            "Pupil specification",
            enum_text(&s.pupil.kind),
            "",
        ),
        metric(
            "pupil_value",
            "Pupil value",
            num(s.pupil.value, p),
            match s.pupil.kind {
                PupilType::EntrancePupilDiameter => "mm",
                _ => "",
            },
        ),
        metric(
            "pupil_mapping",
            "Pupil ray mapping",
            enum_text(&s.pupil_mapping),
            "",
        ),
        metric(
            "field_type",
            "Field specification",
            enum_text(&s.field_type),
            "",
        ),
        metric(
            "object_distance",
            "Object vertex distance",
            s.object_distance.map_or(Cell::infinite(1), |v| num(v, p)),
            "mm",
        ),
        metric(
            "object_material",
            "Object medium",
            Cell::text(&s.object_material),
            "",
        ),
        metric(
            "stop",
            "Aperture stop (surface)",
            num((s.stop_index + 1) as f64, 0),
            "",
        ),
        metric(
            "reference",
            "Saved reference wavelength",
            num(s.reference(), p),
            "nm",
        ),
    ];
    let wavelengths = s
        .wavelengths
        .iter()
        .enumerate()
        .map(|(i, w)| {
            row(
                format!("wavelength_{i}"),
                vec![
                    num((i + 1) as f64, 0),
                    num(w.value, p),
                    num(w.weight, p),
                    Cell::text(if i == s.reference_wavelength {
                        "Yes"
                    } else {
                        "No"
                    }),
                    Cell::text(format!(
                        "({}, {}, {}, {})",
                        w.color[0], w.color[1], w.color[2], w.color[3]
                    )),
                ],
            )
        })
        .collect();
    let fields = s
        .fields
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let mut cells = vec![
                num((i + 1) as f64, 0),
                num(f.x, p),
                num(f.y, p),
                num(f.weight, p),
            ];
            cells.extend(f.vignetting.iter().map(|v| num(*v, p)));
            cells.push(Cell::text(format!(
                "({}, {}, {}, {})",
                f.color[0], f.color[1], f.color[2], f.color[3]
            )));
            row(format!("field_{i}"), cells)
        })
        .collect();
    let field_unit = if s.field_type == FieldType::Angle {
        "degrees"
    } else {
        "mm"
    };
    section(SectionId::Specification,vec!["Field and wavelength rows retain saved order, colors, duplicates and zero weights. Vignetting columns are upper/lower Y, then upper/lower X.".into()],vec![
        metrics("specification","Specification",rows),
        table("wavelengths","Wavelengths",&[("index","Entry",None),("wavelength","Wavelength",Some("nm")),("weight","Weight",None),("reference","Reference",None),("color","RGBA",None)],wavelengths),
        table("fields","Fields",&[("index","Entry",None),("x","X",Some(field_unit)),("y","Y",Some(field_unit)),("weight","Weight",None),
            ("vuy","VUY",None),("vly","VLY",None),("vux","VUX",None),("vlx","VLX",None),("color","RGBA",None)],fields)])
}

fn finite(value: f64) -> Result<f64, String> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err("Computed value is not finite".into())
    }
}
fn first_order(s: &System, w: f64, p: usize) -> Section {
    let mut rows = vec![];
    let mut add = |id: &str, label: &str, result: Result<f64, String>, unit: &str| {
        let cell = if s.object_distance.is_none() && matches!(id, "magnification" | "reduction") {
            Cell::na("Requires a finite object")
        } else {
            Cell::result(result, p)
        };
        rows.push(metric(id, label, cell, unit))
    };
    let matrix = s.matrix(w, 0, s.surfaces.len());
    let basic_value = |key: &str| -> Result<f64, String> {
        match key {
            "object_index" => return s.object_index(w),
            "image_index" => return s.paraxial_index_before(s.surfaces.len(), w).map(f64::abs),
            "entrance_pupil_distance" => return s.entrance_pupil_distance(w),
            "entrance_pupil_radius" => return s.pupil_radius(w),
            "object_na" => {
                return match s.object_distance {
                    None => Ok(0.),
                    Some(d) => finite(
                        (s.object_index(w)?
                            * (s.pupil_radius(w)? / (d + s.entrance_pupil_distance(w)?))
                                .atan()
                                .sin())
                        .abs(),
                    ),
                };
            }
            "exit_pupil_distance" | "exit_pupil_radius" => {
                let (post, index) = s.matrix(w, s.stop_index, s.surfaces.len())?;
                if post.d.abs() < 1e-15 {
                    return Err("Exit pupil lies at infinity".into());
                }
                return if key == "exit_pupil_distance" {
                    finite(-index * post.b / post.d)
                } else {
                    finite(s.pupil_matrix(w)?.a.abs() * s.pupil_radius(w)? / post.d.abs())
                };
            }
            _ => {}
        }
        let (m, n) = *matrix.as_ref().map_err(Clone::clone)?;
        let n0 = s.object_index(w)?;
        match key {
            "efl" | "bfl" | "front_focal_distance" => {
                if m.c.abs() < 1e-14 {
                    return Err("Afocal system has no finite focal position".into());
                }
                finite(match key {
                    "efl" => -n.abs() / m.c,
                    "bfl" => -n * m.a / m.c,
                    _ => n0 * m.d / m.c,
                })
            }
            "magnification" | "image_distance" => {
                if let Some(d) = s.object_distance {
                    let denominator = m.c * d / n0 + m.d;
                    if denominator.abs() < 1e-15 {
                        return Err("Gaussian image conjugate lies at infinity".into());
                    }
                    finite(if key == "magnification" {
                        1. / denominator
                    } else {
                        -n * (m.a * d / n0 + m.b) / denominator
                    })
                } else if key == "magnification" {
                    Err("Magnification requires a finite object".into())
                } else if m.c.abs() < 1e-14 {
                    Err("Infinite object has no finite image conjugate in an afocal system".into())
                } else {
                    finite(-n * m.a / m.c)
                }
            }
            "image_na" | "f_number" => {
                let r = s.pupil_radius(w)?;
                let shift = s.entrance_pupil_distance(w)?;
                let gain = if let Some(d) = s.object_distance {
                    (m.c * d + m.d * n0) / (d + shift)
                } else {
                    m.c
                };
                if key == "image_na" {
                    finite((n * (gain * r / n).atan().sin()).abs())
                } else if gain == 0. {
                    Err("Working F-number is infinite for a zero marginal image cone".into())
                } else {
                    finite(1. / (2. * (gain * r).abs()))
                }
            }
            _ => Err("Unknown first-order quantity".into()),
        }
    };
    for (key, label, unit) in [
        (
            "efl",
            "Effective focal length along image propagation",
            "mm",
        ),
        (
            "front_focal_distance",
            "Front focal position from first vertex",
            "mm",
        ),
        ("bfl", "Back focal position from last vertex", "mm"),
        ("f_number", "Working F-number", ""),
        ("magnification", "Gaussian magnification", ""),
        (
            "image_distance",
            "Gaussian image conjugate from last vertex",
            "mm",
        ),
        (
            "entrance_pupil_distance",
            "Entrance pupil position from first vertex",
            "mm",
        ),
        ("entrance_pupil_radius", "Entrance pupil radius", "mm"),
        (
            "exit_pupil_distance",
            "Exit pupil position from last vertex",
            "mm",
        ),
        ("exit_pupil_radius", "Exit pupil radius", "mm"),
        ("object_index", "Object physical index", ""),
        ("image_index", "Image physical index", ""),
        ("object_na", "Object numerical aperture", ""),
        ("image_na", "Image numerical aperture", ""),
    ] {
        add(key, label, basic_value(key), unit);
    }
    for (key, label, which) in [
        (
            "front_principal_plane",
            "Front principal plane from first vertex",
            0,
        ),
        (
            "back_principal_plane",
            "Back principal plane from last vertex",
            1,
        ),
    ] {
        let result = matrix.as_ref().map_err(Clone::clone).and_then(|(m, n)| {
            if m.c.abs() < 1e-14 {
                Err("Principal plane is undefined for an afocal system".into())
            } else if which == 0 {
                finite(s.object_index(w)? * (m.d - 1.) / m.c)
            } else {
                finite(n * (1. - m.a) / m.c)
            }
        });
        add(key, label, result, "mm");
    }
    let reduction = matrix.as_ref().map_err(Clone::clone).and_then(|(m, _)| {
        let distance = s
            .object_distance
            .ok_or("Reduction requires a finite object")?;
        finite(m.c * distance / s.object_index(w)? + m.d)
    });
    add(
        "reduction",
        "Reduction (inverse Gaussian magnification)",
        reduction,
        "",
    );
    let maximum = s.fields.iter().enumerate().fold(0, |best, (i, f)| {
        if f.x.hypot(f.y) > s.fields[best].x.hypot(s.fields[best].y) {
            i
        } else {
            best
        }
    });
    let f = &s.fields[maximum];
    let rays = inspection::paraxial_trace(s, f, w);
    let chief_data = |component: usize, key: &str| {
        rays.as_ref().map_err(Clone::clone).and_then(|v| {
            let r = v["chief"].as_array().unwrap();
            let at = if s.object_distance.is_some() { 1 } else { 0 };
            r[at][key][component]
                .as_f64()
                .ok_or_else(|| "Chief ray value is undefined".into())
        })
    };
    let frame = s.field_frame(f);
    let invariant = || -> Result<[f64; 2], String> {
        let trace = rays.as_ref().map_err(Clone::clone)?;
        let axis = trace["axis"].as_array().unwrap();
        let at = if s.object_distance.is_some() { 1 } else { 0 };
        let y = axis[at]["point"][1]
            .as_f64()
            .ok_or("Axial height is undefined")?;
        let q = axis[at]["reduced_in"][1]
            .as_f64()
            .ok_or("Axial reduced angle is undefined")?;
        let mut result = [0.; 2];
        for (k, element) in result.iter_mut().enumerate() {
            *element = finite(y * chief_data(k, "reduced_in")? - q * chief_data(k, "point")?)?;
        }
        Ok(result)
    }();
    for k in 0..2 {
        let key = if k == 0 {
            "optical_invariant_x"
        } else {
            "optical_invariant_y"
        };
        let label = if k == 0 {
            "Optical invariant, global X"
        } else {
            "Optical invariant, global Y"
        };
        add(
            key,
            label,
            invariant.as_ref().map_err(Clone::clone).map(|a| a[k]),
            "mm",
        );
    }
    add(
        "optical_invariant",
        "Optical invariant, field-tangential projection",
        invariant
            .as_ref()
            .map_err(Clone::clone)
            .map(|a| FieldFrame::project(frame.tangential, *a)),
        "mm",
    );
    for k in 0..2 {
        add(
            if k == 0 {
                "object_angle_x"
            } else {
                "object_angle_y"
            },
            if k == 0 {
                "Chief object angle X"
            } else {
                "Chief object angle Y"
            },
            chief_data(k, "slope_in").map(|v| v.atan().to_degrees()),
            "degrees",
        );
        add(
            if k == 0 {
                "stored_image_x"
            } else {
                "stored_image_y"
            },
            if k == 0 {
                "Chief at stored image vertex plane X"
            } else {
                "Chief at stored image vertex plane Y"
            },
            s.paraxial_chief_image(f, w).map(|v| v[k]),
            "mm",
        );
        let gaussian = matrix.as_ref().map_err(Clone::clone).and_then(|(m, n)| {
            let h = chief_data(k, "point")?;
            let q = s.object_index(w)? * chief_data(k, "slope_in")?;
            finite(m.a * h + m.b * q + basic_value("image_distance")? / n * (m.c * h + m.d * q))
        });
        add(
            if k == 0 {
                "conjugate_image_x"
            } else {
                "conjugate_image_y"
            },
            if k == 0 {
                "Gaussian conjugate image height X"
            } else {
                "Gaussian conjugate image height Y"
            },
            gaussian,
            "mm",
        );
    }
    rows.push(metric(
        "chief_field",
        "Chief-field entry",
        num((maximum + 1) as f64, 0),
        "",
    ));
    rows.push(metric(
        "chief_field_x",
        "Chief-field X",
        num(f.x, p),
        if s.field_type == FieldType::Angle {
            "degrees"
        } else {
            "mm"
        },
    ));
    rows.push(metric(
        "chief_field_y",
        "Chief-field Y",
        num(f.y, p),
        if s.field_type == FieldType::Angle {
            "degrees"
        } else {
            "mm"
        },
    ));
    rows.push(metric(
        "stored_image_gap",
        "Stored image-space gap",
        num(s.surfaces.last().unwrap().thickness, p),
        "mm",
    ));
    rows.push(metric(
        "image_vertex",
        "Global image vertex",
        num(s.surfaces.iter().map(|v| v.thickness).sum(), p),
        "mm",
    ));
    section(SectionId::FirstOrder,vec!["The first largest-radius stored field defines the chief. Optical invariants use the vertex-source axial ray and the actual-source chief ray; their global X/Y components and field-tangential projection retain their signs. Gaussian conjugates and stored image-plane intercepts are separate quantities. Physical indices are positive; paraxial q includes axial direction. Principal-plane and focal positions use global z offsets from the named vertex.".into()],vec![metrics("first_order","First-order quantities",rows)])
}

fn material_cells(s: &System, medium: &str, w: f64, p: usize) -> [Cell; 3] {
    let current = Cell::result(s.material_index(medium, w), p);
    let d = s.material_index(medium, 587.5618);
    let vd = || -> Result<Cell, String> {
        let nd = *d.as_ref().map_err(Clone::clone)?;
        let f = s.material_index(medium, 486.1327)?;
        let c = s.material_index(medium, 656.2725)?;
        if f == c {
            return Ok(if nd == 1. {
                Cell::na("Relative air has no F-C material dispersion")
            } else {
                Cell::infinite(if nd > 1. { 1 } else { -1 })
            });
        }
        Ok(num((nd - 1.) / (f - c), p))
    }();
    [
        current,
        Cell::result(d, p),
        vd.unwrap_or_else(Cell::unavailable),
    ]
}
fn solve_text(definition: solve::GapSolve, p: usize, reference: f64) -> String {
    let f = |v| Cell::number(v, p).display().to_string();
    match definition {
        solve::GapSolve::Fixed {} => "Fixed".into(),
        solve::GapSolve::Pickup {
            source,
            scale,
            offset,
        } => format!(
            "Pickup: gap {}; scale {}; offset {} mm",
            source + 1,
            f(scale),
            f(offset)
        ),
        solve::GapSolve::EdgeThickness { height, value } => {
            format!("Edge separation {} mm at height {} mm", f(value), f(height))
        }
        solve::GapSolve::OverallLength { first, last, value } => {
            format!("Sum gaps {} through {} = {} mm", first + 1, last, f(value))
        }
        solve::GapSolve::MarginalHeight { value, zone } => format!(
            "Marginal height {} mm; zone {}; saved reference {} nm; first saved field",
            f(value),
            f(zone),
            reference
        ),
    }
}
fn lens_data(s: &System, w: f64, p: usize) -> Section {
    let mut rows = vec![];
    let mut medium = s.object_material.clone();
    let mut vertex = 0.;
    let mut direction = 1.;
    let mut push = |id: String,
                    name: String,
                    label: &str,
                    interaction: Cell,
                    r: f64,
                    gap: Cell,
                    z: Cell,
                    stored: &str,
                    actual: &str,
                    before: &str,
                    aperture: f64,
                    clear: bool,
                    semi: f64,
                    stop: bool,
                    direction: f64| {
        let [index, nd, vd] = material_cells(s, actual, w, p);
        rows.push(row(
            id,
            vec![
                Cell::text(name),
                Cell::text(label),
                interaction,
                radius(r, p),
                gap,
                z,
                Cell::text(stored),
                Cell::text(actual),
                Cell::result(s.material_index(before, w), p),
                index,
                nd,
                vd,
                if clear {
                    num(aperture, p)
                } else {
                    Cell::na("Physical clipping is disabled")
                },
                num(semi, p),
                Cell::text(if stop { "Stop" } else { "" }),
                num(direction, 0),
            ],
        ));
    };
    push(
        "object".into(),
        "Object".into(),
        &s.object_label,
        Cell::text("Source"),
        s.object_surface.radius,
        s.object_distance.map_or(Cell::infinite(1), |v| num(v, p)),
        s.object_distance.map_or(Cell::infinite(-1), |v| num(-v, p)),
        &s.object_material,
        &medium,
        &medium,
        s.object_surface.aperture,
        s.object_surface.clear_aperture,
        s.object_surface.semi_diameter,
        false,
        1.,
    );
    for (i, surface) in s.surfaces.iter().enumerate() {
        let before = medium.clone();
        match surface.interaction {
            Interaction::Refract => medium = surface.material.clone(),
            Interaction::Reflect => direction = -direction,
            Interaction::Dummy => {}
        }
        push(
            format!("surface_{i}"),
            format!("Surface {}", i + 1),
            &surface.label,
            enum_text(&surface.interaction),
            surface.radius,
            num(surface.thickness, p),
            num(vertex, p),
            &surface.material,
            &medium,
            &before,
            surface.aperture,
            surface.clear_aperture,
            surface.semi_diameter,
            i == s.stop_index,
            direction,
        );
        vertex += surface.thickness;
    }
    push(
        "image".into(),
        "Image".into(),
        &s.image_label,
        Cell::text("Detector"),
        s.image_surface.radius,
        Cell::na("No gap follows the image endpoint"),
        num(vertex, p),
        "",
        &medium,
        &medium,
        s.image_surface.aperture,
        s.image_surface.clear_aperture,
        s.image_surface.semi_diameter,
        false,
        direction,
    );
    let constraints = s
        .surfaces
        .iter()
        .enumerate()
        .map(|(i, v)| {
            row(
                format!("gap_{i}"),
                vec![
                    num((i + 1) as f64, 0),
                    num(v.thickness, p),
                    Cell::text(solve_text(v.gap_solve, p, s.reference())),
                ],
            )
        })
        .collect();
    section(SectionId::LensData,vec!["Internal gaps are signed axial translations; the image vertex is their sum. Centered frames have x=y=0. A plane has infinite radius. Stored material and the actual following medium are separate for mirrors and dummy surfaces. Indices are positive physical values at the report environment; n_d and V_d use 587.5618/486.1327/656.2725 nm. V_d=(n_d-1)/(n_F-n_C). Spectral or thermal failures affect only their cells. Direction is the paraxial axial sign, not a real off-axis ray direction. Semi-diameters are the saved derived drawing sizes.".into()],vec![
        table("lens_data","Optical surfaces",&[("surface","Surface",None),("label","Label",None),("interaction","Interaction",None),("radius","Radius",Some("mm")),
            ("gap","Following gap",Some("mm")),("vertex","Global vertex z",Some("mm")),("stored_material","Stored material",None),("medium","Following medium",None),
            ("n_before","Index before",None),("index","Index at report wavelength",None),("nd","n_d",None),("vd","Derived V_d",None),
            ("aperture","Physical clear radius",Some("mm")),("semi_diameter","Drawing semi-diameter",Some("mm")),("stop","Stop",None),("direction","Paraxial z direction",None)],rows),
        table("gap_constraints","Gap relations",&[("gap","Gap",None),("thickness","Resolved thickness",Some("mm")),("definition","Relation",None)],constraints)])
}
fn surface_data(s: &System, p: usize) -> Section {
    let profiles = std::iter::once((
        "object".to_string(),
        "Object".to_string(),
        s.object_surface.profile(),
    ))
    .chain(s.surfaces.iter().enumerate().map(|(i, s)| {
        (
            format!("surface_{i}"),
            format!("Surface {}", i + 1),
            s.profile(),
        )
    }))
    .chain(std::iter::once((
        "image".into(),
        "Image".into(),
        s.image_surface.profile(),
    )));
    let mut tables = vec![];
    for (id, name, profile) in profiles {
        let mut rows = vec![
            metric("radius", "Base radius", radius(profile.radius, p), "mm"),
            metric("conic", "Conic constant", num(profile.conic, p), ""),
        ];
        for (family, terms, start, step) in
            [("even", profile.even, 4, 2), ("odd", profile.odd, 3, 1)]
        {
            for (index, a) in terms.iter().enumerate() {
                let power = start + step * index;
                rows.push(metric(
                    &format!("{family}_{power}"),
                    &format!("{family} r^{power}"),
                    num(*a, p),
                    &format!("mm^{}", 1 - power as i32),
                ));
            }
        }
        if let Some(t) = profile.toroid {
            rows.push(metric(
                "toroid_revolution",
                "Toroidal revolution radius",
                num(t.revolution, p),
                "mm",
            ));
            for (index, a) in t.even_y.iter().enumerate() {
                let power = 4 + 2 * index;
                rows.push(metric(
                    &format!("toroid_y{power}"),
                    &format!("toroidal y^{power}"),
                    num(*a, p),
                    &format!("mm^{}", 1 - power as i32),
                ));
            }
        }
        if let Some(z) = profile.zernike {
            rows.push(metric(
                "zernike_normalize",
                "Zernike normalization radius",
                num(z.normalize, p),
                "mm",
            ));
            for (index, a) in z.coefficients.iter().enumerate() {
                rows.push(metric(
                    &format!("zernike_z{}", index + 1),
                    &format!("Zernike Fringe Z{}", index + 1),
                    num(*a, p),
                    "mm",
                ));
            }
        }
        tables.push(metrics(&id, &format!("{name} profile"), rows));
    }
    section(SectionId::SurfaceData,vec!["Sag is the base conic plus even powers r^4,r^6,... and odd-list powers r^3,r^4,... . Toroidal profiles add a revolution radius with even y^4,y^6,... coefficients on the YZ section. Zernike Fringe sag terms Z1..Z37 (mm, Fringe ordering) add on their normalization radius. Every saved coefficient is listed, including zero entries and simultaneous even/odd terms. Object and image profiles are included. Display formatting uses scientific notation for small nonzero values; structured numeric cells retain full precision.".into()],tables)
}
fn one_line(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\r', "\\r")
        .replace('\n', "\\n")
}
fn cell_text(cell: &Cell) -> String {
    match cell {
        Cell::NotApplicable { reason, text } | Cell::Unavailable { reason, text } => {
            format!("{} ({})", text, one_line(reason))
        }
        _ => one_line(cell.display()),
    }
}

pub fn generate(
    s: &System,
    revision: u64,
    requested: Vec<SectionId>,
    w: f64,
    precision: usize,
) -> Result<PrescriptionReport, String> {
    if precision > 15 {
        return Err("Display decimals must be 0–15".into());
    }
    if !w.is_finite() || !(100. ..=20000.).contains(&w) {
        return Err("Wavelength must be 100–20000 nm".into());
    }
    for (i, id) in requested.iter().enumerate() {
        if requested[..i].contains(id) {
            return Err("Report sections must be unique".into());
        }
    }
    let mut sections = vec![];
    for id in [
        SectionId::General,
        SectionId::Specification,
        SectionId::FirstOrder,
        SectionId::LensData,
        SectionId::SurfaceData,
    ] {
        if requested.contains(&id) {
            sections.push(match id {
                SectionId::General => general(s, precision),
                SectionId::Specification => specification(s, precision),
                SectionId::FirstOrder => first_order(s, w, precision),
                SectionId::LensData => lens_data(s, w, precision),
                SectionId::SurfaceData => surface_data(s, precision),
            });
        }
    }
    let mut diagnostics = vec![];
    for section in &sections {
        for table in &section.tables {
            for row in &table.rows {
                for (column, cell) in table.columns.iter().zip(&row.cells) {
                    if let Cell::Unavailable { reason, .. } = cell {
                        diagnostics.push(Diagnostic {
                            section: section.id,
                            table: table.id.clone(),
                            row: row.id.clone(),
                            column: column.id.clone(),
                            message: reason.clone(),
                        });
                    }
                }
            }
        }
    }
    let mut text = format!(
        "PRESCRIPTION\nTitle: {}\nSaved revision: {revision}\nWavelength: {w} nm\nDisplay decimals: {precision}\nLength: mm; field angles: degrees; temperature: C; pressure: Pa\n",
        one_line(&s.name)
    );
    writeln!(
        text,
        "Sections: {}",
        if sections.is_empty() {
            "none".into()
        } else {
            sections
                .iter()
                .map(|s| s.title.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        }
    )
    .unwrap();
    for section in &sections {
        writeln!(
            text,
            "\n{}\n{}",
            section.title.to_uppercase(),
            "=".repeat(section.title.len())
        )
        .unwrap();
        for paragraph in &section.paragraphs {
            writeln!(text, "{paragraph}\n").unwrap();
        }
        for table in &section.tables {
            writeln!(text, "{}", table.title).unwrap();
            writeln!(
                text,
                "{}",
                table
                    .columns
                    .iter()
                    .map(|c| match &c.unit {
                        Some(u) => format!("{} [{u}]", c.label),
                        None => c.label.clone(),
                    })
                    .collect::<Vec<_>>()
                    .join("\t")
            )
            .unwrap();
            for row in &table.rows {
                writeln!(
                    text,
                    "{}",
                    row.cells
                        .iter()
                        .map(cell_text)
                        .collect::<Vec<_>>()
                        .join("\t")
                )
                .unwrap();
            }
            text.push('\n');
        }
    }
    Ok(PrescriptionReport {
        revision,
        name: s.name.clone(),
        wavelength: w,
        precision,
        sections,
        diagnostics,
        text,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn value(
        report: &PrescriptionReport,
        section: SectionId,
        table: &str,
        row: &str,
        column: usize,
    ) -> Value {
        serde_json::to_value(
            &report
                .sections
                .iter()
                .find(|s| s.id == section)
                .unwrap()
                .tables
                .iter()
                .find(|t| t.id == table)
                .unwrap()
                .rows
                .iter()
                .find(|r| r.id == row)
                .unwrap()
                .cells[column],
        )
        .unwrap()
    }
    fn first(s: &System) -> PrescriptionReport {
        generate(s, 42, vec![SectionId::FirstOrder], s.reference(), 6).unwrap()
    }
    fn close(r: &PrescriptionReport, key: &str, expected: f64) {
        let v = value(r, SectionId::FirstOrder, "first_order", key, 1);
        assert_eq!(v["kind"], "number", "{key}: {v}");
        let actual = v["value"].as_f64().unwrap();
        assert!(
            (actual - expected).abs() < 1e-10,
            "{key}: {actual} != {expected}"
        );
    }
    #[test]
    fn canonical_selection_text_and_precision_retain_saved_values() {
        let mut s = System::default();
        s.name = "Title\tline\nend".into();
        s.note = "First line\nSecond line".into();
        s.image_surface.even = vec![1e-20, 0.];
        s.wavelengths.push(Wavelength::new(s.reference(), 0.));
        let before = serde_json::to_value(&s).unwrap();
        let r = generate(
            &s,
            17,
            vec![
                SectionId::SurfaceData,
                SectionId::General,
                SectionId::Specification,
            ],
            s.reference(),
            3,
        )
        .unwrap();
        assert_eq!(
            r.sections.iter().map(|s| s.id).collect::<Vec<_>>(),
            vec![
                SectionId::General,
                SectionId::Specification,
                SectionId::SurfaceData
            ]
        );
        assert!(r.text.contains("Title: Title\\tline\\nend"));
        assert!(r.text.contains("First line\nSecond line"));
        let tiny = value(&r, SectionId::SurfaceData, "image", "even_4", 1);
        assert_eq!(tiny["value"], 1e-20);
        assert_eq!(tiny["text"], "1.000e-20");
        assert_eq!(
            value(&r, SectionId::SurfaceData, "image", "even_6", 1)["value"],
            0.
        );
        for section in &r.sections {
            for table in &section.tables {
                for row in &table.rows {
                    assert_eq!(row.cells.len(), table.columns.len());
                    let text = row
                        .cells
                        .iter()
                        .map(cell_text)
                        .collect::<Vec<_>>()
                        .join("\t");
                    assert!(r.text.lines().any(|line| line == text));
                }
            }
        }
        assert_eq!(serde_json::to_value(&s).unwrap(), before);
        let empty = generate(&s, 17, vec![], 500., 0).unwrap();
        assert!(empty.sections.is_empty() && empty.diagnostics.is_empty());
        assert!(empty.text.contains("Sections: none"));
        assert!(generate(&s, 0, vec![SectionId::General, SectionId::General], 500., 3).is_err());
        for (w, p) in [(f64::NAN, 3), (99., 3), (20001., 3), (500., 16)] {
            assert!(generate(&s, 0, vec![], w, p).is_err());
        }
    }
    #[test]
    fn hand_derived_thick_lens_cardinals_conjugates_and_skew_invariants() {
        let mut s = System::default();
        s.wavelengths = vec![Wavelength::new(587.562, 1.)];
        s.reference_wavelength = 0;
        s.object_distance = Some(200.);
        s.field_type = FieldType::ObjectHeight;
        s.object_material = "1.500000:60.000000".into();
        s.surfaces = vec![Surface::sphere(-50., 200., "AIR", 10.)];
        s.stop_index = 0;
        s.fields = vec![Field {
            x: 3.,
            ..Field::meridional(4.)
        }];
        s.pupil.value = 6.;
        let r = first(&s);
        // Single immersed interface: M=[1,0; -0.01,1], n0=1.5, ni=1.
        for (key, expected) in [
            ("efl", 100.),
            ("front_focal_distance", -150.),
            ("bfl", 100.),
            ("front_principal_plane", 0.),
            ("back_principal_plane", 0.),
            ("reduction", -1. / 3.),
            ("magnification", -3.),
            ("image_distance", 400.),
            ("object_index", 1.5),
            ("image_index", 1.),
            ("conjugate_image_x", -9.),
            ("conjugate_image_y", -12.),
            ("stored_image_x", -4.5),
            ("stored_image_y", -6.),
            ("optical_invariant_x", -0.0675),
            ("optical_invariant_y", -0.09),
            ("optical_invariant", -0.1125),
        ] {
            close(&r, key, expected);
        }
        // A two-interface lens: hand product R2*T(5/1.5)*R1.
        s.object_material = "AIR".into();
        s.object_distance = None;
        s.surfaces = vec![
            Surface::sphere(50., 5., "1.500000:60.000000", 10.),
            Surface::sphere(-50., 30., "AIR", 10.),
        ];
        s.field_type = FieldType::Angle;
        s.fields = vec![Field::meridional(5.)];
        let r = first(&s);
        let c = -0.01 * (29. / 30.) - 0.01;
        close(&r, "front_principal_plane", (29. / 30. - 1.) / c);
        close(&r, "back_principal_plane", (1. - 29. / 30.) / c);
        close(&r, "efl", -1. / c);
        close(&r, "optical_invariant", 3. * 5_f64.to_radians().tan());
    }
    #[test]
    fn mirror_and_afocal_cells_preserve_valid_quantities() {
        let mut s = System::default();
        s.surfaces = vec![Surface::sphere(-100., -50., "N-BK7_SCHOTT", 10.)];
        s.stop_index = 0;
        s.surfaces[0].interaction = Interaction::Reflect;
        let r = first(&s);
        close(&r, "efl", 50.);
        close(&r, "bfl", -50.);
        close(&r, "image_distance", -50.);
        close(&r, "front_focal_distance", -50.);
        close(&r, "object_index", 1.);
        close(&r, "image_index", 1.);
        for key in ["magnification", "reduction"] {
            assert_eq!(
                value(&r, SectionId::FirstOrder, "first_order", key, 1)["kind"],
                "not_applicable"
            );
            assert!(!r.diagnostics.iter().any(|d| d.row == key));
        }
        s.surfaces[0].radius = 0.;
        s.object_distance = Some(100.);
        let r = first(&s);
        close(&r, "image_distance", 100.);
        close(&r, "magnification", 1.);
        for key in [
            "efl",
            "bfl",
            "front_principal_plane",
            "back_principal_plane",
        ] {
            assert_eq!(
                value(&r, SectionId::FirstOrder, "first_order", key, 1)["kind"],
                "unavailable"
            );
            assert!(r.diagnostics.iter().any(|d| d.row == key));
        }
        s.object_distance = None;
        let r = first(&s);
        close(&r, "object_index", 1.);
        close(&r, "image_index", 1.);
        assert_eq!(
            value(
                &r,
                SectionId::FirstOrder,
                "first_order",
                "image_distance",
                1
            )["kind"],
            "unavailable"
        );
    }
    #[test]
    fn endpoints_mixed_profiles_constraints_and_actual_media_are_complete() {
        let mut s = System::default();
        s.object_distance = Some(100.);
        s.object_surface.radius = -40.;
        s.image_surface.radius = 30.;
        s.object_surface.even = vec![0., 1e-18];
        s.object_surface.odd = vec![1e-8, 0.];
        s.surfaces[0].interaction = Interaction::Reflect;
        s.surfaces[0].thickness = -5.;
        s.surfaces[1].interaction = Interaction::Dummy;
        s.surfaces[1].material = "N-SF11_SCHOTT".into();
        s.surfaces[1].gap_solve = solve::GapSolve::Pickup {
            source: 0,
            scale: 2.,
            offset: 1.,
        };
        let r = generate(
            &s,
            9,
            vec![SectionId::LensData, SectionId::SurfaceData],
            s.reference(),
            6,
        )
        .unwrap();
        for id in ["object", "surface_0", "surface_1", "image"] {
            assert_eq!(
                value(&r, SectionId::LensData, "lens_data", id, 7)["text"],
                "AIR"
            );
        }
        assert_eq!(
            value(&r, SectionId::LensData, "lens_data", "surface_0", 15)["value"],
            -1.
        );
        assert_eq!(
            value(&r, SectionId::LensData, "lens_data", "surface_1", 5)["value"],
            -5.
        );
        assert_eq!(
            value(&r, SectionId::LensData, "lens_data", "image", 4)["kind"],
            "not_applicable"
        );
        assert_eq!(
            value(&r, SectionId::LensData, "lens_data", "object", 11)["kind"],
            "not_applicable"
        );
        assert!(
            value(&r, SectionId::LensData, "gap_constraints", "gap_1", 2)["text"]
                .as_str()
                .unwrap()
                .contains("Pickup: gap 1")
        );
        assert_eq!(
            value(&r, SectionId::SurfaceData, "object", "even_6", 1)["value"],
            1e-18
        );
        assert_eq!(
            value(&r, SectionId::SurfaceData, "object", "odd_4", 1)["value"],
            0.
        );
        assert!(r.diagnostics.is_empty());
    }
    #[test]
    fn spectral_failure_is_local_to_cells_and_retains_reference_dispersion() {
        let s = System::default();
        let r = generate(
            &s,
            0,
            vec![
                SectionId::General,
                SectionId::LensData,
                SectionId::FirstOrder,
            ],
            100.,
            6,
        )
        .unwrap();
        assert_eq!(
            value(&r, SectionId::LensData, "lens_data", "surface_0", 9)["kind"],
            "unavailable"
        );
        assert_eq!(
            value(&r, SectionId::LensData, "lens_data", "surface_0", 10)["kind"],
            "number"
        );
        assert_eq!(
            value(&r, SectionId::LensData, "lens_data", "surface_0", 11)["kind"],
            "number"
        );
        close(&r, "object_index", 1.);
        close(&r, "object_na", 0.);
        assert_eq!(
            value(&r, SectionId::General, "general", "name", 1)["text"],
            s.name
        );
        assert!(
            r.diagnostics
                .iter()
                .any(|d| d.table == "lens_data" && d.row == "surface_0" && d.column == "index")
        );
    }
}
