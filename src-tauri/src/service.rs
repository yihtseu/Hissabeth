use crate::optics::{
    self, Field, PupilSampling, Spectrum, System, analysis, aperture, material, project, solve,
};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
pub struct Engine {
    pub system: System,
    pub revision: u64,
    pub path: PathBuf,
    _lease: std::fs::File,
}
fn persist_system(path: &std::path::Path, system: &System) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(system).map_err(|e| e.to_string())?;
    let pending = path.with_extension("pending");
    let mut file = std::fs::File::create(&pending).map_err(|e| e.to_string())?;
    file.write_all(&bytes).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    std::fs::rename(&pending, path).map_err(|e| e.to_string())
}
pub fn field_argument(s: &System, args: &Value) -> Result<Field, String> {
    if let Some(i) = args["field_index"].as_u64() {
        return s
            .fields
            .get(i as usize)
            .cloned()
            .ok_or_else(|| "Field index outside prescription".into());
    }
    if args.get("field").is_some() || args.get("field_x").is_some() {
        return Ok(Field {
            x: args["field_x"].as_f64().unwrap_or(0.),
            y: args["field"].as_f64().unwrap_or(0.),
            weight: 1.,
            vignetting: [0.; 4],
            color: Field::meridional(0.).color,
        });
    }
    Ok(s.fields[0].clone())
}
impl Engine {
    pub fn open(path: PathBuf) -> Result<Self, String> {
        let lease = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path.with_extension("lock"))
            .map_err(|e| e.to_string())?;
        lease
            .try_lock()
            .map_err(|_| "This workspace is already open in another Hissabeth process")?;
        let loaded = path.exists();
        let mut system: System = if loaded {
            serde_json::from_slice(&std::fs::read(&path).map_err(|e| e.to_string())?)
                .map_err(|e| format!("Invalid optical project: {e}"))?
        } else {
            System::default()
        };
        let stored = serde_json::to_value(&system).map_err(|e| e.to_string())?;
        system.update()?;
        if loaded && stored != serde_json::to_value(&system).map_err(|e| e.to_string())? {
            persist_system(&path, &system)?;
        }
        let mut random = [0u8; 8];
        getrandom::fill(&mut random).map_err(|e| e.to_string())?;
        let revision = u64::from_le_bytes(random) & ((1u64 << 48) - 1);
        let e = Self {
            system,
            revision,
            path,
            _lease: lease,
        };
        e.system.validate()?;
        Ok(e)
    }
    fn replace(&mut self, mut next: System, expected: u64) -> Result<Value, String> {
        if expected != self.revision {
            return Err("System changed; read it again before editing".into());
        }
        next.update()?;
        self.commit(next, expected)
    }
    fn commit(&mut self, next: System, expected: u64) -> Result<Value, String> {
        if expected != self.revision {
            return Err("System changed; read it again before editing".into());
        }
        persist_system(&self.path, &next)?;
        self.system = next;
        self.revision += 1;
        Ok(json!({"revision":self.revision}))
    }
    pub fn call(&mut self, name: &str, args: Value) -> Result<Value, String> {
        validate_args(name, &args)?;
        let w = args["wavelength"]
            .as_f64()
            .unwrap_or(self.system.reference());
        let field = field_argument(&self.system, &args)?;
        let grid = args["grid"].as_u64().unwrap_or(17) as usize;
        let samples = args["samples"].as_u64().unwrap_or(33) as usize;
        let expected = || {
            args["revision"]
                .as_u64()
                .ok_or_else(|| "revision is required".to_string())
        };
        let mut result = match name {
            "system_get" => Ok(json!({"system":self.system,"revision":self.revision})),
            "system_set" => {
                let s: System =
                    serde_json::from_value(args["system"].clone()).map_err(|e| e.to_string())?;
                self.replace(s, expected()?)
            }
            "system_validate" => {
                let mut s: System =
                    serde_json::from_value(args["system"].clone()).map_err(|e| e.to_string())?;
                s.update()?;
                Ok(json!({"valid":true,"system":s}))
            }
            "materials_list" => {
                let mut names: Vec<String> = material::catalogs()
                    .iter()
                    .chain(&self.system.custom_glasses)
                    .map(|g| g.key())
                    .collect();
                names.extend(["AIR".into(), "FUSED_SILICA".into()]);
                names.sort();
                names.dedup();
                Ok(json!(names))
            }
            "material_search" => {
                let query = args["query"].as_str().unwrap_or("").to_uppercase();
                let catalog = args["catalog"].as_str().unwrap_or("").to_uppercase();
                let offset = args["offset"].as_u64().unwrap_or(0) as usize;
                let limit = args["limit"].as_u64().unwrap_or(50) as usize;
                let matches: Vec<_> = material::catalogs()
                    .iter()
                    .chain(&self.system.custom_glasses)
                    .filter(|g| {
                        (catalog.is_empty() || g.catalog == catalog)
                            && (g.name.contains(&query) || g.catalog.contains(&query))
                    })
                    .collect();
                let data:Vec<_>=matches.iter().skip(offset).take(limit).map(|g|json!({"key":g.key(),"name":g.name,"catalog":g.catalog,"nd":g.nd,"vd":g.vd,"formula":g.formula,"wavelength_range":g.wavelength_range})).collect();
                Ok(
                    json!({"total":matches.len(),"glasses":data,"warnings":material::catalog_warnings().iter().filter(|w|catalog.is_empty()||w.catalog==catalog).collect::<Vec<_>>()}),
                )
            }
            "material_index" => {
                let name = args["material"].as_str().unwrap();
                let temperature = args["temperature"]
                    .as_f64()
                    .unwrap_or(self.system.temperature);
                let pressure = args["pressure"].as_f64().unwrap_or(self.system.pressure);
                let mut s = self.system.clone();
                s.temperature = temperature;
                s.pressure = pressure;
                let n = s.material_index(name, w)?;
                Ok(json!({"index":n,"wavelength":w,"temperature":temperature,"pressure":pressure}))
            }
            "air_index" => Ok(
                json!({"index":material::air_index(w/1000.,args["temperature"].as_f64().unwrap_or(self.system.temperature),args["pressure"].as_f64().unwrap_or(self.system.pressure))?}),
            ),
            "catalog_import" => {
                let mut next = self.system.clone();
                let catalog = args["catalog"].as_str().unwrap();
                let glasses = material::parse_agf(catalog, args["text"].as_str().unwrap())?;
                let count = glasses.glasses.len();
                next.custom_glasses
                    .retain(|g| g.catalog != catalog.to_uppercase());
                next.custom_glasses.extend(glasses.glasses);
                let mut r = self.replace(next, expected()?)?;
                r["glasses_imported"] = json!(count);
                r["warnings"] = json!(glasses.warnings);
                Ok(r)
            }
            "paraxial" => self.system.paraxial(w),
            "trace_ray" => serde_json::to_value(self.system.trace_field(
                args["x"].as_f64().unwrap_or(0.),
                args["y"].as_f64().unwrap_or(0.),
                &field,
                w,
            )?)
            .map_err(|e| e.to_string()),
            "spot_diagram" => self.system.spot_field(
                &field,
                w,
                serde_json::from_value::<PupilSampling>(args["sampling"].clone())
                    .map_err(|e| e.to_string())?,
            ),
            "polychromatic_spot" => analysis::polychromatic_spot(
                &self.system,
                &field,
                serde_json::from_value::<PupilSampling>(args["sampling"].clone())
                    .map_err(|e| e.to_string())?,
            ),
            "layout" => Ok(self.system.layout()),
            "geometric_mtf" => crate::compute::geometric_mtf(&self.system, &args),
            "ray_fan" => analysis::ray_fan(&self.system, &field, w, samples),
            "opd_fan" => analysis::opd_fan(&self.system, &field, w, samples),
            "chromatic_focus" => analysis::chromatic_focus(
                &self.system,
                args["lower"].as_f64().unwrap_or_else(|| {
                    self.system
                        .wavelengths
                        .iter()
                        .map(|w| w.value)
                        .fold(f64::INFINITY, f64::min)
                }),
                args["upper"].as_f64().unwrap_or_else(|| {
                    self.system
                        .wavelengths
                        .iter()
                        .map(|w| w.value)
                        .fold(f64::NEG_INFINITY, f64::max)
                }),
                samples,
            ),
            "longitudinal_aberration" => analysis::longitudinal(&self.system, w, samples),
            "spherochromatism" => {
                let reference_wavelength = self.system.reference();
                let reference_focus = self.system.paraxial(reference_wavelength)?["image_distance"]
                    .as_f64()
                    .unwrap();
                let curves: Vec<_> = self
                    .system
                    .wavelengths
                    .iter()
                    .enumerate()
                    .map(|(index, wv)| -> Result<Value, String> {
                        let mut curve = analysis::longitudinal(&self.system, wv.value, samples)?;
                        curve["color"] = json!(wv.color);
                        curve["weight"] = json!(wv.weight);
                        curve["wavelength_index"] = json!(index);
                        Ok(curve)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(
                    json!({"curves":curves,"reference_focus":reference_focus,"reference_wavelength":reference_wavelength,"units":"mm"}),
                )
            }
            "astigmatism" => analysis::astigmatism(&self.system, &field, w),
            "distortion" => analysis::distortion(&self.system, &field, w),
            "wavefront" => Ok(analysis::wavefront(&self.system, &field, w, grid)?.json()),
            "diffraction_psf" => analysis::diffraction_psf(
                &self.system,
                &field,
                w,
                args["grid"].as_u64().unwrap_or(32) as usize,
            ),
            "diffraction_mtf" => analysis::diffraction_mtf(
                &self.system,
                &field,
                serde_json::from_value::<Spectrum>(args["spectrum"].clone())
                    .map_err(|e| e.to_string())?,
                args["grid"].as_u64().unwrap_or(32) as usize,
                args["max_frequency"].as_f64(),
            ),
            "encircled_energy" => analysis::encircled_energy(
                &self.system,
                &field,
                w,
                serde_json::from_value::<PupilSampling>(args["sampling"].clone())
                    .map_err(|e| e.to_string())?,
                samples,
            ),
            "surface_sag" => {
                let i = args["surface"].as_u64().unwrap() as usize;
                let s = self
                    .system
                    .surfaces
                    .get(i)
                    .ok_or("Surface index outside prescription")?;
                let h = args["height"].as_f64().unwrap();
                let (z, dz) = optics::sag(s, h)?;
                Ok(json!({"sag":z,"derivative":dz,"height":h,"units":"mm"}))
            }
            "autofocus" => {
                let s = solve::autofocus(&self.system, w)?;
                self.commit(s, expected()?)
            }
            "solve" => {
                let definition = serde_json::from_value(args["definition"].clone())
                    .map_err(|e| e.to_string())?;
                let (next, result) = solve::apply(
                    &self.system,
                    args["gap"].as_u64().unwrap() as usize,
                    definition,
                )?;
                let mut r = self.commit(next, expected()?)?;
                r["solve"] = result;
                Ok(r)
            }
            "solve_recalculate" => {
                let mut r = self.replace(self.system.clone(), expected()?)?;
                r["solves"] = json!(solve::states(&self.system));
                Ok(r)
            }
            "aperture_report" => {
                let mut report = serde_json::to_value(aperture::semi_diameters(&self.system)?)
                    .map_err(|e| e.to_string())?;
                report["revision"] = json!(self.revision);
                Ok(report)
            }
            "vignetting_update" => {
                let revision = expected()?;
                if revision != self.revision {
                    return Err("System changed; read it again before editing".into());
                }
                let (next, report) = aperture::update_vignetting(&self.system)?;
                let committed = self.commit(next, revision)?;
                let mut result = serde_json::to_value(report).map_err(|e| e.to_string())?;
                result["revision"] = committed["revision"].clone();
                Ok(result)
            }
            "project_import" => {
                let text = args["text"].as_str().unwrap();
                let next = match args["format"].as_str().unwrap() {
                    "geopter" => project::import(text)?,
                    "hissabeth" => serde_json::from_str(text).map_err(|e| e.to_string())?,
                    _ => unreachable!(),
                };
                self.replace(next, expected()?)
            }
            "project_export" => {
                let v = if args["format"] == "geopter" {
                    project::export(&self.system)?
                } else {
                    serde_json::to_value(&self.system).map_err(|e| e.to_string())?
                };
                Ok(json!({"text":serde_json::to_string_pretty(&v).map_err(|e|e.to_string())?}))
            }
            "capabilities" => Ok(
                json!({"tools":tools(),"units":{"length":"mm","wavelength":"nm","field_angle":"degrees","pressure":"Pa","temperature":"C"},"backend":"cpu","scope":{"sequential":true,"finite_conjugates":true,"curved_object_surface":true,"curved_image_surface":true,"endpoint_apertures":true,"object_medium":true,"prescribed_plot_colors":true,"internal_stop_aiming":true,"centered_surfaces":true,"centered_reflection":true,"automatic_semidiameters":true,"directional_vignetting":true,"field_local_analyses":true,"pupil_sampling":["grid","hexapolar"],"pupil_mappings":["reference_entrance_pupil","real_stop"],"weighted_polychromatic_mtf":true,"analysis_revisions":true,"folded_coordinates":false,"polarization":false,"nonsequential":false}}),
            ),
            _ => Err(format!("Unknown tool {name}")),
        }?;
        if [
            "paraxial",
            "trace_ray",
            "layout",
            "spot_diagram",
            "polychromatic_spot",
            "geometric_mtf",
            "ray_fan",
            "opd_fan",
            "chromatic_focus",
            "longitudinal_aberration",
            "spherochromatism",
            "astigmatism",
            "distortion",
            "wavefront",
            "diffraction_psf",
            "diffraction_mtf",
            "encircled_energy",
            "surface_sag",
        ]
        .contains(&name)
        {
            result["revision"] = json!(self.revision);
        }
        Ok(result)
    }
}
fn num(min: f64, max: f64) -> Value {
    json!({"type":"number","minimum":min,"maximum":max})
}
fn integer(min: usize, max: usize) -> Value {
    json!({"type":"integer","minimum":min,"maximum":max})
}
fn string() -> Value {
    json!({"type":"string"})
}
fn array(items: Value, min: usize, max: usize) -> Value {
    json!({"type":"array","items":items,"minItems":min,"maxItems":max})
}
fn enumeration(values: &[&str]) -> Value {
    json!({"type":"string","enum":values})
}
fn object(props: Value, required: &[&str]) -> Value {
    json!({"type":"object","properties":props,"required":required,"additionalProperties":false})
}
fn sampling_schema() -> Value {
    json!({"oneOf":[
        object(json!({"kind":enumeration(&["grid"]),"count":integer(3,129)}), &["kind","count"]),
        object(json!({"kind":enumeration(&["hexapolar"]),"rings":integer(1,64)}), &["kind","rings"])
    ]})
}
fn spectrum_schema() -> Value {
    json!({"oneOf":[
        object(json!({"kind":enumeration(&["monochromatic"]),"wavelength":num(100.,20000.)}), &["kind","wavelength"]),
        object(json!({"kind":enumeration(&["polychromatic"])}), &["kind"])
    ]})
}
fn solve_schema() -> Value {
    let number = num(-1e15, 1e15);
    json!({"oneOf":[
        object(json!({"kind":enumeration(&["fixed"])}), &["kind"]),
        object(json!({"kind":enumeration(&["pickup"]),"source":integer(0,127),"scale":number,"offset":number}), &["kind","source","scale","offset"]),
        object(json!({"kind":enumeration(&["edge_thickness"]),"height":num(0.,1e6),"value":number}), &["kind","height","value"]),
        object(json!({"kind":enumeration(&["overall_length"]),"first":integer(0,127),"last":integer(1,128),"value":number}), &["kind","first","last","value"]),
        object(json!({"kind":enumeration(&["marginal_height"]),"value":number,"zone":{"type":"number"}}), &["kind","value","zone"])
    ]})
}
pub fn system_schema() -> Value {
    let finite = num(-1e15, 1e15);
    let nonnegative = num(0., 1e15);
    let surface = object(
        json!({"label":string(),"radius":finite,"thickness":finite,"material":string(),"aperture":num(1e-12,1e9),"semi_diameter":nonnegative,"clear_aperture":{"type":"boolean"},"conic":finite,"even":array(finite.clone(),0,16),"odd":array(finite.clone(),0,20),"interaction":enumeration(&["refract","reflect","dummy"]),"gap_solve":solve_schema()}),
        &[
            "label",
            "radius",
            "thickness",
            "material",
            "aperture",
            "semi_diameter",
            "clear_aperture",
            "conic",
            "even",
            "odd",
            "interaction",
            "gap_solve",
        ],
    );
    let endpoint = object(
        json!({"radius":finite,"conic":finite,"even":array(finite.clone(),0,16),"odd":array(finite.clone(),0,20),"aperture":{"type":"number","exclusiveMinimum":0.},"clear_aperture":{"type":"boolean"},"semi_diameter":nonnegative}),
        &[
            "radius",
            "conic",
            "even",
            "odd",
            "aperture",
            "clear_aperture",
            "semi_diameter",
        ],
    );
    let field = object(
        json!({"x":finite,"y":finite,"weight":nonnegative,"vignetting":array(json!({"type":"number","exclusiveMaximum":1.}),4,4),"color":array(num(0.,1.),4,4)}),
        &["x", "y", "weight", "vignetting", "color"],
    );
    let wavelength = object(
        json!({"value":num(100.,20000.),"weight":nonnegative,"color":array(num(0.,1.),4,4)}),
        &["value", "weight", "color"],
    );
    let glass = object(
        json!({"name":string(),"catalog":string(),"formula":integer(1,13),"coefficients":array(finite.clone(),0,20),"nd":finite,"vd":finite,"wavelength_range":{"type":["array","null"],"items":num(0.,100.),"minItems":2,"maxItems":2},"thermal":{"type":["array","null"],"items":finite,"minItems":7,"maxItems":7},"transmission":array(array(finite.clone(),3,3),0,10000),"comment":string()}),
        &[
            "name",
            "catalog",
            "formula",
            "coefficients",
            "nd",
            "vd",
            "wavelength_range",
            "thermal",
            "transmission",
            "comment",
        ],
    );
    object(
        json!({"name":string(),"note":string(),"surfaces":array(surface,1,128),"wavelengths":array(wavelength,1,32),"reference_wavelength":integer(0,31),"fields":array(field,1,64),"field_type":enumeration(&["angle","object_height","image_height"]),"pupil":object(json!({"kind":enumeration(&["entrance_pupil_diameter","f_number","object_na","image_na"]),"value":num(1e-12,1e9)}),&["kind","value"]),"pupil_mapping":enumeration(&["reference_entrance_pupil","real_stop"]),"object_distance":{"type":["number","null"],"exclusiveMinimum":0.},"object_material":string(),"object_label":string(),"image_label":string(),"object_surface":endpoint,"image_surface":endpoint,"stop_index":integer(0,127),"temperature":num(-200.,500.),"pressure":num(0.,1e8),"custom_glasses":array(glass,0,10000)}),
        &[
            "name",
            "note",
            "surfaces",
            "wavelengths",
            "reference_wavelength",
            "fields",
            "field_type",
            "pupil",
            "pupil_mapping",
            "object_distance",
            "object_material",
            "object_label",
            "image_label",
            "object_surface",
            "image_surface",
            "stop_index",
            "temperature",
            "pressure",
            "custom_glasses",
        ],
    )
}
pub fn tools() -> Value {
    let mut list = vec![];
    let mut add = |name: &str, description: &str, properties: Value, required: &[&str]| {
        list.push(json!({"name":name,"description":description,"inputSchema":object(properties,required)}))
    };
    add(
        "system_get",
        "Read the entire current optical prescription and revision",
        json!({}),
        &[],
    );
    add(
        "system_set",
        "Atomically save the complete prescription; revision must match system_get",
        json!({"system":system_schema(),"revision":integer(0,9_007_199_254_740_991)}),
        &["system", "revision"],
    );
    add(
        "system_validate",
        "Validate a complete prescription without changing the workspace",
        json!({"system":system_schema()}),
        &["system"],
    );
    add(
        "materials_list",
        "List qualified product_catalog keys for built-in and project glass catalogs",
        json!({}),
        &[],
    );
    add(
        "material_search",
        "Search catalog metadata; wavelengths in metadata are micrometers",
        json!({"query":string(),"catalog":string(),"offset":integer(0,100000),"limit":integer(1,500)}),
        &[],
    );
    let wave = num(100., 20000.);
    let revision = integer(0, 9_007_199_254_740_991);
    add(
        "material_index",
        "Compute refractive index at wavelength nm with environmental corrections",
        json!({"material":string(),"wavelength":wave,"temperature":num(-200.,500.),"pressure":num(0.,1e8)}),
        &["material", "wavelength"],
    );
    add(
        "air_index",
        "Compute absolute air index from wavelength nm, temperature C and pressure Pa",
        json!({"wavelength":wave,"temperature":num(-200.,500.),"pressure":num(0.,1e8)}),
        &["wavelength"],
    );
    add(
        "catalog_import",
        "Parse an AGF catalog and replace that named project catalog, preserving all other catalogs",
        json!({"catalog":string(),"text":string(),"revision":revision}),
        &["catalog", "text", "revision"],
    );
    add(
        "layout",
        "Get global 3D ray paths for stored fields, including explicit trace rejection reasons",
        json!({}),
        &[],
    );
    let common = json!({"wavelength":wave,"field_index":integer(0,63),"field":num(-1e9,1e9),"field_x":num(-1e9,1e9)});
    for (name, description) in [
        (
            "paraxial",
            "Compute first-order matrix, focal lengths, finite image conjugate, magnification and pupils",
        ),
        (
            "trace_ray",
            "Trace x/y in pupil mm, normalized by the wavelength's paraxial entrance-pupil radius using the saved pupil mapping; stored field_index or arbitrary field/field_x",
        ),
        (
            "spot_diagram",
            "Trace a grid or hexapolar pupil quadrature; report area-weighted centroid/RMS, geometric radius and rejected-ray counts",
        ),
        (
            "polychromatic_spot",
            "Area- and spectrum-weighted spot diagram for positive-weight wavelengths, retaining lateral color and sampled transmission",
        ),
        (
            "geometric_mtf",
            "Geometric MTF from a complex OTF sum over weighted real intercepts on one physical frequency grid; selected or weighted wavelengths",
        ),
        (
            "ray_fan",
            "Field-local tangential/sagittal transverse ray aberrations relative to chief ray in mm",
        ),
        (
            "opd_fan",
            "Field-local tangential/sagittal optical path difference relative to chief reference sphere, in waves",
        ),
        (
            "longitudinal_aberration",
            "Longitudinal spherical aberration as axis intercept versus pupil radius",
        ),
        (
            "spherochromatism",
            "Longitudinal spherical aberration for every stored wavelength",
        ),
        (
            "astigmatism",
            "Differential sagittal/tangential focal positions about the chief ray",
        ),
        (
            "distortion",
            "Actual chief image intercept relative to the paraxial chief intercept at the stored image plane",
        ),
        (
            "wavefront",
            "Masked pupil wavefront map, RMS and peak-to-valley in waves",
        ),
        (
            "diffraction_psf",
            "Unit-energy Fraunhofer diffraction intensity and sampled Strehl ratio; paraxial exit pupil mapping",
        ),
        (
            "diffraction_mtf",
            "Field-local diffraction MTF from a complex OTF sum at common physical frequencies, with spectral/transmission weights and real-chief lateral color",
        ),
        (
            "encircled_energy",
            "Geometric encircled energy about the transmitted-ray centroid",
        ),
    ] {
        let mut props = common.clone();
        if name == "trace_ray" {
            props["x"] = num(-1e6, 1e6);
            props["y"] = num(-1e6, 1e6);
        }
        let mut required = vec![];
        if [
            "spot_diagram",
            "polychromatic_spot",
            "geometric_mtf",
            "encircled_energy",
        ]
        .contains(&name)
        {
            props["sampling"] = sampling_schema();
            required.push("sampling");
        }
        if ["geometric_mtf", "diffraction_mtf"].contains(&name) {
            props.as_object_mut().unwrap().remove("wavelength");
            props["spectrum"] = spectrum_schema();
            required.push("spectrum");
        }
        if name == "polychromatic_spot" {
            props.as_object_mut().unwrap().remove("wavelength");
        }
        if ["wavefront", "diffraction_psf", "diffraction_mtf"].contains(&name) {
            props["grid"] = match name {
                "diffraction_psf" | "diffraction_mtf" => {
                    json!({"type":"integer","enum":[8,16,32,64,128]})
                }
                "wavefront" => integer(5, 129),
                _ => integer(3, 129),
            };
        }
        if [
            "ray_fan",
            "opd_fan",
            "longitudinal_aberration",
            "spherochromatism",
            "encircled_energy",
        ]
        .contains(&name)
        {
            props["samples"] = integer(if name == "encircled_energy" { 2 } else { 3 }, 257);
        }
        if name == "geometric_mtf" {
            props["max_frequency"] =
                json!({"type":"number","exclusiveMinimum":0.,"maximum":10000.});
            props["axis"] = enumeration(&["x", "y"]);
        }
        if name == "diffraction_mtf" {
            props["max_frequency"] = json!({"type":"number","exclusiveMinimum":0.});
        }
        add(name, description, props, &required);
    }
    add(
        "chromatic_focus",
        "Paraxial image focus shift across a wavelength interval",
        json!({"lower":wave,"upper":wave,"samples":integer(2,257)}),
        &[],
    );
    add(
        "surface_sag",
        "Evaluate radial sag and its first derivative, including even and odd terms",
        json!({"surface":integer(0,127),"height":num(0.,1e6)}),
        &["surface", "height"],
    );
    add(
        "autofocus",
        "Set final image distance to the finite or infinite conjugate paraxial focus",
        json!({"wavelength":wave,"revision":revision}),
        &["revision"],
    );
    add(
        "solve",
        "Store a persistent gap solve and recalculate all constraints atomically. Fixed removes the relationship and retains thickness. Indices are zero-based; overall length sums the half-open gap interval [first,last)",
        json!({"gap":integer(0,127),"definition":solve_schema(),"revision":revision}),
        &["gap", "definition", "revision"],
    );
    add(
        "solve_recalculate",
        "Recalculate all saved gap constraints atomically and return their definitions and resolved thicknesses",
        json!({"revision":revision}),
        &["revision"],
    );
    add(
        "aperture_report",
        "Reference-wavelength drawing envelopes from chief and four field-local pupil rim rays, ignoring physical clipping; includes reached prefixes and geometric failures",
        json!({}),
        &[],
    );
    add(
        "vignetting_update",
        "Compute four global-axis aperture boundaries per field, store directional vignetting factors, and resolve gap constraints atomically at the reference wavelength",
        json!({"revision":revision}),
        &["revision"],
    );
    add(
        "project_import",
        "Import native or Geopter project text and atomically replace current prescription",
        json!({"format":enumeration(&["geopter","hissabeth"]),"text":string(),"revision":revision}),
        &["format", "text", "revision"],
    );
    add(
        "project_export",
        "Export the current prescription as native or Geopter JSON text",
        json!({"format":enumeration(&["geopter","hissabeth"])}),
        &["format"],
    );
    add(
        "capabilities",
        "Describe all implemented tools and physical scope",
        json!({}),
        &[],
    );
    Value::Array(list)
}
fn validate_schema(value: &Value, schema: &Value, path: &str) -> Result<(), String> {
    if let Some(choices) = schema["oneOf"].as_array() {
        let results: Vec<_> = choices
            .iter()
            .map(|s| validate_schema(value, s, path))
            .collect();
        let matching = results.iter().filter(|r| r.is_ok()).count();
        return if matching == 1 {
            Ok(())
        } else if matching == 0 {
            Err(format!(
                "Invalid definition at {path}: {}",
                results
                    .into_iter()
                    .filter_map(Result::err)
                    .collect::<Vec<_>>()
                    .join("; ")
            ))
        } else {
            Err(format!("Ambiguous definition at {path}"))
        };
    }
    let matches = |kind: &str| match kind {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "number" => value.as_f64().is_some_and(|n| n.is_finite()),
        "integer" => value.as_u64().is_some(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        _ => false,
    };
    let valid = if let Some(k) = schema["type"].as_str() {
        matches(k)
    } else if let Some(kinds) = schema["type"].as_array() {
        kinds.iter().any(|k| matches(k.as_str().unwrap_or("")))
    } else {
        true
    };
    if !valid {
        return Err(format!("Invalid type at {path}"));
    }
    if value.is_null() {
        return Ok(());
    }
    if let Some(n) = value.as_f64() {
        for (bound, inclusive, lower) in [
            ("minimum", true, true),
            ("maximum", true, false),
            ("exclusiveMinimum", false, true),
            ("exclusiveMaximum", false, false),
        ] {
            if let Some(b) = schema[bound].as_f64() {
                if if lower {
                    n < b || (!inclusive && n == b)
                } else {
                    n > b || (!inclusive && n == b)
                } {
                    return Err(format!("Value outside range at {path}"));
                }
            }
        }
    }
    if let Some(allowed) = schema["enum"].as_array() {
        if !allowed.contains(value) {
            return Err(format!("Unknown value at {path}"));
        }
    }
    if let Some(text) = value.as_str() {
        if text.len() > 8_000_000 {
            return Err(format!("Text too large at {path}"));
        }
    }
    if let Some(items) = value.as_array() {
        let min = schema["minItems"].as_u64().unwrap_or(0) as usize;
        let max = schema["maxItems"].as_u64().unwrap_or(100_000) as usize;
        if items.len() < min || items.len() > max {
            return Err(format!("Invalid array length at {path}"));
        }
        for (i, v) in items.iter().enumerate() {
            validate_schema(v, &schema["items"], &format!("{path}[{i}]"))?;
        }
    }
    if let Some(object) = value.as_object() {
        if let Some(required) = schema["required"].as_array() {
            for key in required {
                let key = key.as_str().unwrap();
                if !object.contains_key(key) {
                    return Err(format!("Missing {path}.{key}"));
                }
            }
        }
        for (key, v) in object {
            let child = &schema["properties"][key];
            if child.is_null() {
                return Err(format!("Unknown argument {path}.{key}"));
            }
            validate_schema(v, child, &format!("{path}.{key}"))?;
        }
    }
    Ok(())
}
fn validate_args(name: &str, args: &Value) -> Result<(), String> {
    let list = tools();
    let tool = list
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == name)
        .ok_or_else(|| format!("Unknown tool {name}"))?;
    if args.get("field_index").is_some()
        && (args.get("field").is_some() || args.get("field_x").is_some())
    {
        return Err("Choose a stored field_index or explicit field coordinates".into());
    }
    validate_schema(args, &tool["inputSchema"], "arguments")
}
pub fn rpc(request: Value, engine: &mut Engine) -> Option<Value> {
    let id = request.get("id")?.clone();
    let result = match request["method"].as_str().unwrap_or("") {
        "initialize" => Ok(
            json!({"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"hissabeth","version":env!("CARGO_PKG_VERSION")},"instructions":"Read system_get before editing. All dimensions are mm and wavelengths nm. GUI reflects changes. Use field_index for stored fields. Every mutating operation requires the current revision."}),
        ),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools":tools()})),
        "tools/call" => {
            let p = &request["params"];
            let r = engine.call(
                p["name"].as_str().unwrap_or(""),
                p.get("arguments").cloned().unwrap_or(json!({})),
            );
            Ok(match r {
                Ok(v) => {
                    json!({"content":[{"type":"text","text":v.to_string()}],"isError":false})
                }
                Err(e) => json!({"content":[{"type":"text","text":e}],"isError":true}),
            })
        }
        _ => Err(json!({"code":-32601,"message":"Method not found"})),
    };
    Some(match result {
        Ok(v) => json!({"jsonrpc":"2.0","id":id,"result":v}),
        Err(e) => json!({"jsonrpc":"2.0","id":id,"error":e}),
    })
}
pub fn data_dir() -> PathBuf {
    std::env::var_os("HISSABETH_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            #[cfg(target_os = "windows")]
            let root = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
            #[cfg(not(target_os = "windows"))]
            let root = std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .or_else(|| {
                    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share"))
                });
            root.unwrap_or_else(|| PathBuf::from(".")).join("hissabeth")
        })
}
pub fn serve(engine: Arc<Mutex<Engine>>) -> Result<(), String> {
    let dir = data_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let mut random = [0u8; 32];
    getrandom::fill(&mut random).map_err(|e| e.to_string())?;
    let token = random
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let endpoint = json!({"address":listener.local_addr().unwrap().to_string(),"token":token});
    let path = dir.join("endpoint.json");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|e| e.to_string())?;
    file.write_all(endpoint.to_string().as_bytes())
        .map_err(|e| e.to_string())?;
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
            let mut line = String::new();
            if BufReader::new((&mut stream).take(16_000_000))
                .read_line(&mut line)
                .is_err()
            {
                continue;
            }
            let Ok(req) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if req["token"] != token {
                continue;
            }
            let response = match engine.lock() {
                Ok(mut e) => rpc(req["request"].clone(), &mut e),
                Err(_) => None,
            };
            if let Some(r) = response {
                let _ = writeln!(stream, "{r}");
            }
        }
    });
    Ok(())
}
pub fn relay() -> Result<(), String> {
    for line in std::io::stdin().lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        let request: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => {
                println!(
                    "{}",
                    json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Parse error"}})
                );
                continue;
            }
        };
        if request.get("id").is_none() {
            continue;
        }
        let response = (|| -> Result<Value, String> {
            let endpoint: Value = serde_json::from_slice(
                &std::fs::read(data_dir().join("endpoint.json"))
                    .map_err(|_| "Open Hissabeth before connecting MCP")?,
            )
            .map_err(|e| e.to_string())?;
            let address = endpoint["address"].as_str().ok_or("Invalid endpoint")?;
            let addr: std::net::SocketAddr = address.parse().map_err(|_| "Invalid endpoint")?;
            if !addr.ip().is_loopback() {
                return Err("Endpoint must be loopback".into());
            }
            let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(3))
                .map_err(|e| format!("Open Hissabeth: {e}"))?;
            stream
                .set_read_timeout(Some(Duration::from_secs(60)))
                .map_err(|e| e.to_string())?;
            writeln!(
                stream,
                "{}",
                json!({"token":endpoint["token"],"request":request})
            )
            .map_err(|e| e.to_string())?;
            let mut result = String::new();
            BufReader::new(stream)
                .read_line(&mut result)
                .map_err(|e| e.to_string())?;
            serde_json::from_str(&result).map_err(|e| e.to_string())
        })();
        println!(
            "{}",
            response.unwrap_or_else(
                |e| json!({"jsonrpc":"2.0","id":request["id"],"error":{"code":-32603,"message":e}})
            )
        );
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn engine(label: &str) -> Engine {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!("{label}-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        Engine::open(path).unwrap()
    }
    #[test]
    fn mcp_tool_payload_is_one_json_text_document() {
        let mut e = engine("mcp-json-payload");
        for (index, name) in ["system_get", "materials_list"].iter().enumerate() {
            let response = rpc(json!({"jsonrpc":"2.0","id":index,"method":"tools/call","params":{"name":name,"arguments":{}}}), &mut e).unwrap();
            let result = &response["result"];
            assert_eq!(result.as_object().unwrap().len(), 2);
            assert_eq!(result["isError"], false);
            assert_eq!(result["content"].as_array().unwrap().len(), 1);
            assert_eq!(result["content"][0]["type"], "text");
            let value: Value =
                serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
            if *name == "system_get" {
                assert_eq!(value["system"]["object_material"], "AIR");
                assert_eq!(
                    value["system"]["wavelengths"][0]["color"]
                        .as_array()
                        .unwrap()
                        .len(),
                    4
                );
            } else {
                assert!(value.as_array().unwrap().contains(&json!("AIR")));
            }
        }
    }
    #[test]
    fn project_metadata_and_spherochromatism_persist_with_strict_atomic_edits() {
        use crate::optics::Wavelength;
        let mut e = engine("project-metadata");
        let mut system = e.system.clone();
        system.object_material = "1.500000:60.000000".into();
        system.object_label = "Submerged emitter".into();
        system.image_label = "Measured detector".into();
        system.wavelengths = vec![
            Wavelength {
                color: [0.9, 0.1, 0.2, 0.],
                ..Wavelength::new(656.273, 1.)
            },
            Wavelength {
                color: [0.1, 0.8, 0.2, 0.25],
                ..Wavelength::new(587.562, 1.)
            },
            Wavelength {
                color: [0.1, 0.2, 0.9, 1.],
                ..Wavelength::new(486.133, 0.)
            },
            Wavelength {
                color: [0.9, 0.7, 0.1, 0.5],
                ..Wavelength::new(587.562, 2.)
            },
        ];
        system.reference_wavelength = 1;
        let revision = e.revision;
        e.call("system_set", json!({"system":system,"revision":revision}))
            .unwrap();
        let response = e.call("spherochromatism", json!({"samples":5})).unwrap();
        let curves = response["curves"].as_array().unwrap();
        assert_eq!(curves.len(), 4);
        assert_eq!(response["reference_wavelength"], 587.562);
        assert_eq!(response["revision"], e.revision);
        for (index, curve) in curves.iter().enumerate() {
            assert_eq!(curve["wavelength_index"], index);
            assert_eq!(curve["color"], json!(system.wavelengths[index].color));
            assert_eq!(curve["weight"], system.wavelengths[index].weight);
            assert_eq!(curve["points"][0]["shift"], 0.);
        }
        let reference = response["reference_focus"].as_f64().unwrap();
        assert!((curves[1]["points"][0]["focus"].as_f64().unwrap() - reference).abs() < 1e-12);
        assert!((curves[0]["points"][0]["focus"].as_f64().unwrap() - reference).abs() > 1e-5);
        let saved = std::fs::read(&e.path).unwrap();
        let current = serde_json::to_value(&e.system).unwrap();
        for mutation in 0..5 {
            let mut broken = current.clone();
            match mutation {
                0 => {
                    broken.as_object_mut().unwrap().remove("object_material");
                }
                1 => {
                    broken["fields"][0].as_object_mut().unwrap().remove("color");
                }
                2 => broken["wavelengths"][0]["color"] = json!([0., 0., 0.]),
                3 => broken["wavelengths"][0]["color"] = json!([0., 0., 0., 1.1]),
                _ => broken["object_material"] = json!("UNKNOWN_OBJECT_MATERIAL"),
            }
            assert!(
                e.call("system_set", json!({"system":broken,"revision":e.revision}))
                    .is_err()
            );
            assert_eq!(std::fs::read(&e.path).unwrap(), saved);
            assert_eq!(serde_json::to_value(&e.system).unwrap(), current);
        }
        let path = e.path.clone();
        drop(e);
        let reopened = Engine::open(path).unwrap();
        assert_eq!(serde_json::to_value(reopened.system).unwrap(), current);
    }
    #[test]
    fn endpoint_shapes_solved_image_gap_and_derived_envelopes_persist_atomically() {
        let mut e = engine("endpoint-shapes");
        let mut system = e.system.clone();
        system.object_distance = Some(300.);
        system.object_surface.radius = 500.;
        system.image_surface.radius = 100.;
        system.image_surface.conic = -1.;
        system.image_surface.even = vec![0.0001];
        e.call("system_set", json!({"system":system,"revision":e.revision}))
            .unwrap();
        e.call("solve",json!({"gap":1,"definition":{"kind":"edge_thickness","height":3.,"value":5.},"revision":e.revision})).unwrap();
        let current = serde_json::to_value(&e.system).unwrap();
        let saved = std::fs::read(&e.path).unwrap();
        for mutation in 0..5 {
            let mut broken = current.clone();
            match mutation {
                0 => {
                    broken.as_object_mut().unwrap().remove("object_surface");
                }
                1 => broken["image_surface"]["thickness"] = json!(2.),
                2 => broken["image_surface"]["semi_diameter"] = json!(-1.),
                3 => broken["object_surface"]["aperture"] = json!(0.),
                _ => broken["object_distance"] = Value::Null,
            }
            assert!(
                e.call("system_set", json!({"system":broken,"revision":e.revision}))
                    .is_err()
            );
            assert_eq!(std::fs::read(&e.path).unwrap(), saved);
            assert_eq!(serde_json::to_value(&e.system).unwrap(), current);
        }
        let mut stale = current.clone();
        stale["object_surface"]["semi_diameter"] = json!(999.);
        stale["image_surface"]["semi_diameter"] = json!(999.);
        stale["surfaces"][1]["thickness"] = json!(999.);
        let path = e.path.clone();
        drop(e);
        std::fs::write(&path, serde_json::to_vec(&stale).unwrap()).unwrap();
        let recovered = Engine::open(path.clone()).unwrap();
        assert_eq!(serde_json::to_value(&recovered.system).unwrap(), current);
        assert_eq!(
            serde_json::from_slice::<Value>(&std::fs::read(path).unwrap()).unwrap(),
            current
        );
    }
    #[test]
    fn discovery_matches_dispatch() {
        let mut e = engine("discovery");
        let r = rpc(
            json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
            &mut e,
        )
        .unwrap();
        let names: Vec<_> = r["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        for tool in [
            "system_get",
            "system_set",
            "system_validate",
            "material_search",
            "ray_fan",
            "wavefront",
            "diffraction_psf",
            "solve",
            "project_import",
        ] {
            assert!(names.contains(&tool));
        }
        assert!(rpc(json!({"method":"notifications/initialized"}), &mut e).is_none());
    }
    #[test]
    fn revision_is_required_and_detects_stale_edits() {
        let mut e = engine("revision");
        let first = e.revision;
        let system = serde_json::to_value(&e.system).unwrap();
        assert!(
            e.call("system_set", json!({"system":system,"revision":first+1}))
                .is_err()
        );
        e.call("system_set", json!({"system":system,"revision":first}))
            .unwrap();
        assert_eq!(e.revision, first + 1);
        let path = e.path.clone();
        let data = serde_json::to_value(&e.system).unwrap();
        drop(e);
        let recovered = Engine::open(path.clone()).unwrap();
        assert_eq!(serde_json::to_value(&recovered.system).unwrap(), data);
        assert_ne!(recovered.revision, first);
        drop(recovered);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn schema_rejects_nested_unknowns_and_invalid_samples() {
        let mut e = engine("schema");
        let mut s = serde_json::to_value(&e.system).unwrap();
        s["surfaces"][0]["secret"] = json!(1);
        let revision = e.revision;
        assert!(
            e.call("system_set", json!({"system":s,"revision":revision}))
                .is_err()
        );
        assert!(
            e.call(
                "spot_diagram",
                json!({"sampling":{"kind":"grid","count":4.5}})
            )
            .is_err()
        );
        assert!(
            e.call("trace_ray", json!({"field_index":0,"field":2.}))
                .is_err()
        );
    }
    #[test]
    fn autofocus_and_gap_solve_persist() {
        let mut e = engine("autofocus");
        let revision = e.revision;
        let result = e.call("autofocus", json!({"revision":revision})).unwrap();
        assert_eq!(result["revision"], revision + 1);
        let revision = e.revision;
        let result=e.call("solve",json!({"gap":1,"definition":{"kind":"pickup","source":0,"scale":2.,"offset":1.},"revision":revision})).unwrap();
        assert_eq!(result["solve"]["thickness"], 11.);
        std::fs::remove_file(&e.path).unwrap();
    }
    #[test]
    fn solve_state_updates_validates_imports_and_recovers_atomically() {
        let mut e = engine("persistent-solves");
        let revision = e.revision;
        e.call("solve", json!({"gap":1,"definition":{"kind":"pickup","source":0,"scale":2.,"offset":1.},"revision":revision})).unwrap();
        let mut draft = serde_json::to_value(&e.system).unwrap();
        draft["surfaces"][0]["thickness"] = json!(7.);
        draft["surfaces"][1]["thickness"] = json!(999.);
        let preview = e.call("system_validate", json!({"system":draft})).unwrap();
        assert_eq!(preview["system"]["surfaces"][1]["thickness"], 15.);
        assert_eq!(e.system.surfaces[1].thickness, 11.);
        let revision = e.revision;
        e.call("system_set", json!({"system":draft,"revision":revision}))
            .unwrap();
        assert_eq!(e.system.surfaces[1].thickness, 15.);
        let revision = e.revision;
        let saved = std::fs::read(&e.path).unwrap();
        assert!(
            e.call("autofocus", json!({"revision":revision}))
                .unwrap_err()
                .contains("controlled")
        );
        assert!(e.call("solve", json!({"gap":0,"definition":{"kind":"pickup","source":1,"scale":0.5,"offset":0.},"revision":revision})).unwrap_err().contains("unique"));
        assert_eq!(e.revision, revision);
        assert_eq!(std::fs::read(&e.path).unwrap(), saved);
        assert_eq!(e.system.surfaces[0].thickness, 7.);
        let native = e
            .call("project_export", json!({"format":"hissabeth"}))
            .unwrap();
        let mut imported: Value = serde_json::from_str(native["text"].as_str().unwrap()).unwrap();
        imported["surfaces"][0]["thickness"] = json!(8.);
        imported["surfaces"][1]["thickness"] = json!(-200.);
        e.call(
            "project_import",
            json!({"format":"hissabeth","text":imported.to_string(),"revision":revision}),
        )
        .unwrap();
        assert_eq!(e.system.surfaces[1].thickness, 17.);
        assert!(
            e.call("project_export", json!({"format":"geopter"}))
                .unwrap_err()
                .contains("solve relationships")
        );
        let revision = e.revision;
        let recalculated = e
            .call("solve_recalculate", json!({"revision":revision}))
            .unwrap();
        assert_eq!(recalculated["solves"][0]["definition"]["kind"], "pickup");
        assert_eq!(recalculated["solves"][0]["thickness"], 17.);
        let path = e.path.clone();
        drop(e);
        imported["surfaces"][0]["thickness"] = json!(9.);
        imported["surfaces"][1]["thickness"] = json!(77.);
        std::fs::write(&path, imported.to_string()).unwrap();
        let mut e = Engine::open(path.clone()).unwrap();
        assert_eq!(e.system.surfaces[1].thickness, 19.);
        let stored: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(stored["surfaces"][1]["thickness"], 19.);
        let revision = e.revision;
        e.call(
            "solve",
            json!({"gap":1,"definition":{"kind":"fixed"},"revision":revision}),
        )
        .unwrap();
        let mut draft = serde_json::to_value(&e.system).unwrap();
        draft["surfaces"][0]["thickness"] = json!(12.);
        let revision = e.revision;
        e.call("system_set", json!({"system":draft,"revision":revision}))
            .unwrap();
        assert_eq!(e.system.surfaces[1].thickness, 19.);
        drop(e);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn solve_definitions_and_sampling_schemas_match_runtime_constraints() {
        let mut e = engine("solve-schema");
        let revision = e.revision;
        for definition in [
            json!({"kind":"pickup","source":0,"scale":2.}),
            json!({"kind":"fixed","value":1.}),
            json!({"kind":"overall_length","first":0,"last":2,"value":70.,"scale":1.}),
            json!({"kind":"marginal_height","value":0.,"zone":"invalid"}),
        ] {
            assert!(
                e.call(
                    "solve",
                    json!({"gap":1,"definition":definition,"revision":revision})
                )
                .is_err()
            );
        }
        let mut native = serde_json::to_value(&e.system).unwrap();
        native["surfaces"][0]
            .as_object_mut()
            .unwrap()
            .remove("gap_solve");
        assert!(
            e.call(
                "project_import",
                json!({"format":"hissabeth","text":native.to_string(),"revision":revision})
            )
            .is_err()
        );
        assert!(
            serde_json::from_value::<solve::GapSolve>(json!({"kind":"fixed","extra":0})).is_err()
        );
        assert!(validate_args("diffraction_psf", &json!({"grid":17})).is_err());
        assert!(
            validate_args(
                "diffraction_mtf",
                &json!({"grid":129,"spectrum":{"kind":"polychromatic"}})
            )
            .is_err()
        );
        assert!(validate_args("wavefront", &json!({"grid":3})).is_err());
        assert!(validate_args("opd_fan", &json!({"grid":4})).is_err());
        for grid in [8, 16, 32, 64, 128] {
            validate_args("diffraction_psf", &json!({"grid":grid})).unwrap();
            validate_args(
                "diffraction_mtf",
                &json!({"grid":grid,"spectrum":{"kind":"polychromatic"}}),
            )
            .unwrap();
        }
        validate_args(
            "encircled_energy",
            &json!({"samples":2,"sampling":{"kind":"grid","count":17}}),
        )
        .unwrap();
        validate_args("solve", &json!({"gap":1,"definition":{"kind":"marginal_height","value":0.,"zone":1.5},"revision":revision})).unwrap();
        assert_eq!(e.revision, revision);
    }
    #[test]
    fn aperture_schema_revision_and_atomic_failure() {
        let mut e = engine("apertures");
        let mut system = serde_json::to_value(&e.system).unwrap();
        let revision = e.revision;
        system["surfaces"][0]
            .as_object_mut()
            .unwrap()
            .remove("semi_diameter");
        assert!(
            e.call("system_set", json!({"system":system,"revision":revision}))
                .is_err()
        );
        assert!(validate_args("opd_fan", &json!({"samples":2})).is_err());
        validate_args("opd_fan", &json!({"samples":3})).unwrap();
        assert!(validate_args("opd_fan", &json!({"grid":17})).is_err());
        let mut system = serde_json::to_value(&e.system).unwrap();
        system["fields"][0]["vignetting"] = json!([-3., 0., 0., 0.]);
        e.call("system_set", json!({"system":system,"revision":revision}))
            .unwrap();
        let revision = e.revision;
        let report = e.call("aperture_report", json!({})).unwrap();
        assert_eq!(report["revision"], revision);
        let mut system = serde_json::to_value(&e.system).unwrap();
        system["surfaces"][1]["aperture"] = json!(0.001);
        e.call("system_set", json!({"system":system,"revision":revision}))
            .unwrap();
        let revision = e.revision;
        let saved = std::fs::read(&e.path).unwrap();
        assert!(
            e.call("vignetting_update", json!({"revision":revision-1}))
                .is_err()
        );
        assert!(
            e.call("vignetting_update", json!({"revision":revision}))
                .is_err()
        );
        assert_eq!(e.revision, revision);
        assert_eq!(std::fs::read(&e.path).unwrap(), saved);
    }

    #[test]
    fn spectrum_sampling_contracts_and_read_revisions_match_engine_state() {
        let mut e = engine("spectrum-sampling");
        let initial = e.revision;
        let system = serde_json::to_value(&e.system).unwrap();
        e.call("system_set", json!({"system":system,"revision":initial}))
            .unwrap();
        let current = e.revision;
        let saved = std::fs::read(&e.path).unwrap();
        let mut missing = serde_json::to_value(&e.system).unwrap();
        missing.as_object_mut().unwrap().remove("pupil_mapping");
        assert!(
            e.call("system_set", json!({"system":missing,"revision":current}))
                .is_err()
        );
        assert!(
            e.call("project_export", json!({"format":"geopter"}))
                .is_err()
        );
        for (name, args) in [
            (
                "spot_diagram",
                json!({"sampling":{"kind":"hexapolar","rings":2}}),
            ),
            (
                "polychromatic_spot",
                json!({"sampling":{"kind":"grid","count":9}}),
            ),
            (
                "geometric_mtf",
                json!({"spectrum":{"kind":"polychromatic"},"sampling":{"kind":"hexapolar","rings":2}}),
            ),
            (
                "diffraction_mtf",
                json!({"spectrum":{"kind":"polychromatic"},"grid":8}),
            ),
            (
                "encircled_energy",
                json!({"sampling":{"kind":"hexapolar","rings":2},"samples":3}),
            ),
            ("paraxial", json!({})),
            ("trace_ray", json!({"x":0.,"y":0.})),
            ("layout", json!({})),
            ("ray_fan", json!({"samples":3})),
            ("opd_fan", json!({"samples":3})),
            (
                "chromatic_focus",
                json!({"lower":500.,"upper":600.,"samples":3}),
            ),
            ("longitudinal_aberration", json!({"samples":3})),
            ("spherochromatism", json!({"samples":3})),
            ("astigmatism", json!({})),
            ("distortion", json!({})),
            ("wavefront", json!({"grid":5})),
            ("diffraction_psf", json!({"grid":8})),
            ("surface_sag", json!({"surface":0,"height":1.})),
        ] {
            let result = e
                .call(name, args)
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            assert_eq!(result["revision"], current, "{name}");
        }
        for (name, args) in [
            ("spot_diagram", json!({})),
            (
                "spot_diagram",
                json!({"grid":9,"sampling":{"kind":"grid","count":9}}),
            ),
            (
                "spot_diagram",
                json!({"sampling":{"kind":"hexapolar","rings":0}}),
            ),
            (
                "spot_diagram",
                json!({"sampling":{"kind":"grid","count":5,"rings":2}}),
            ),
            (
                "polychromatic_spot",
                json!({"sampling":{"kind":"grid","count":9},"wavelength":500.}),
            ),
            (
                "geometric_mtf",
                json!({"sampling":{"kind":"grid","count":9}}),
            ),
            (
                "geometric_mtf",
                json!({"sampling":{"kind":"grid","count":9},"spectrum":{"kind":"monochromatic"}}),
            ),
            (
                "geometric_mtf",
                json!({"sampling":{"kind":"grid","count":9},"spectrum":{"kind":"polychromatic","wavelength":500.}}),
            ),
            (
                "diffraction_mtf",
                json!({"spectrum":{"kind":"polychromatic"},"wavelength":500.}),
            ),
            (
                "diffraction_mtf",
                json!({"spectrum":{"kind":"polychromatic"},"grid":8,"max_frequency":0.}),
            ),
            (
                "diffraction_mtf",
                json!({"spectrum":{"kind":"polychromatic"},"grid":8,"max_frequency":1e8}),
            ),
        ] {
            assert!(e.call(name, args).is_err(), "{name}");
        }
        assert_eq!(e.revision, current);
        assert_eq!(std::fs::read(&e.path).unwrap(), saved);
    }
    #[test]
    fn catalog_import_is_transactional() {
        let mut e = engine("catalog");
        let revision = e.revision;
        assert!(
            e.call(
                "catalog_import",
                json!({"catalog":"USER","text":"CD invalid","revision":revision})
            )
            .is_err()
        );
        assert_eq!(e.revision, revision);
        let text = "NM TEST 5 0 1.5 60\nCD 1.5 0 0\nLD 0.3 2.5\nTD 0 0 0 0 0 0 20";
        let r = e
            .call(
                "catalog_import",
                json!({"catalog":"USER","text":text,"revision":revision}),
            )
            .unwrap();
        assert_eq!(r["glasses_imported"], 1);
        let index = e
            .call(
                "material_index",
                json!({"material":"TEST_USER","wavelength":587.5618}),
            )
            .unwrap();
        assert!((index["index"].as_f64().unwrap() - 1.5).abs() < 1e-12);
        std::fs::remove_file(&e.path).unwrap();
    }
}
