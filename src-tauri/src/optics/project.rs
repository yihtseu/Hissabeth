// Geopter project serialization translated from OpticalSystem::SaveToFile/LoadFromFile.
use super::*;
use serde_json::{Map, Value, json};
fn number(v: &Value, label: &str) -> Result<f64, String> {
    v.as_f64()
        .filter(|n| n.is_finite())
        .ok_or_else(|| format!("Missing or invalid Geopter {label}"))
}
fn array<'a>(v: &'a Value, label: &str) -> Result<&'a Vec<Value>, String> {
    v.as_array()
        .ok_or_else(|| format!("Missing Geopter {label} array"))
}
fn code(v: &Value, label: &str) -> Result<u64, String> {
    v.as_u64().ok_or_else(|| format!("Invalid Geopter {label}"))
}
fn color(v: &Value) -> Result<[f64; 4], String> {
    let values = array(v, "color")?;
    if values.len() != 4 {
        return Err("Geopter color requires four RGBA components".into());
    }
    let rgba = [
        number(&values[0], "color")?,
        number(&values[1], "color")?,
        number(&values[2], "color")?,
        number(&values[3], "color")?,
    ];
    if !valid_color(rgba) {
        return Err("Geopter RGBA components must be in 0–1".into());
    }
    Ok(rgba)
}
fn endpoint(value: &Value) -> Result<EndpointSurface, String> {
    let curvature = number(&value["Curvature"], "surface curvature")?;
    let mut shape = EndpointSurface::plane();
    shape.radius = if curvature == 0. { 0. } else { 1. / curvature };
    match value["Type"]
        .as_str()
        .ok_or("Missing Geopter surface type")?
    {
        "SPH" => {}
        "ASP" | "ODD" => {
            shape.conic = number(&value["Conic"], "conic")?;
            let coefficients = array(&value["Coefs"], "asphere coefficients")?
                .iter()
                .map(|v| number(v, "asphere coefficient"))
                .collect::<Result<Vec<_>, _>>()?;
            if value["Type"] == "ASP" {
                shape.even = coefficients;
            } else {
                shape.odd = coefficients;
            }
        }
        other => return Err(format!("Unknown Geopter surface profile {other}")),
    }
    if let Some(aperture) = value.get("Aperture") {
        if aperture["Type"] != "Circular" {
            return Err("Geopter aperture must be Circular".into());
        }
        shape.aperture = number(&aperture["Radius"], "aperture radius")?;
        shape.clear_aperture = true;
    }
    shape.validate()?;
    Ok(shape)
}
fn profile_json(
    profile: Profile<'_>,
    aperture: f64,
    clear_aperture: bool,
) -> Result<Value, String> {
    if !profile.even.is_empty() && !profile.odd.is_empty() {
        return Err(
            "Geopter project profiles cannot combine even and odd coefficient lists".into(),
        );
    }
    if profile.even.len() > 10 || profile.odd.len() > 10 {
        return Err("Geopter project profiles store at most ten polynomial coefficients".into());
    }
    let mut value = json!({"Type":if !profile.odd.is_empty(){"ODD"}else if !profile.even.is_empty()||profile.conic!=0.{"ASP"}else{"SPH"},"Curvature":if profile.radius==0.{0.}else{1./profile.radius}});
    if value["Type"] != "SPH" {
        value["Conic"] = json!(profile.conic);
        let terms = if !profile.odd.is_empty() {
            profile.odd
        } else {
            profile.even
        };
        // Geopter's even profile reader indexes all ten entries.
        let mut coefficients = vec![0.; 10];
        coefficients[..terms.len()].copy_from_slice(terms);
        value["Coefs"] = json!(coefficients);
    }
    if clear_aperture {
        value["Aperture"] = json!({"Type":"Circular","Radius":aperture});
    }
    Ok(value)
}
pub fn import(text: &str) -> Result<System, String> {
    let root: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let a = root["Assembly"]
        .as_object()
        .ok_or("Missing Geopter Assembly")?;
    let mut keys: Vec<usize> = a
        .keys()
        .filter_map(|key| key.parse::<usize>().ok())
        .collect();
    keys.sort_unstable();
    if keys.len() < 3 || keys.iter().enumerate().any(|(i, k)| i != *k) {
        return Err(
            "Geopter Assembly requires contiguous object, surfaces and image indices".into(),
        );
    }
    let object = &a["0"];
    let object_surface = endpoint(object)?;
    let last = &a[&(keys.len() - 1).to_string()];
    let image_surface = endpoint(last)?;
    if number(&last["Thickness"], "terminal thickness")? != 0. || last["Material"] != "AIR" {
        return Err(
            "Geopter terminal image entry requires zero following thickness and AIR".into(),
        );
    }
    let spec = &root["Spec"];
    let fs = &spec["Field"];
    let ws = &spec["Wvl"];
    let field_type = match code(&fs["Type"], "field type")? {
        0 => FieldType::Angle,
        1 => FieldType::ObjectHeight,
        2 => FieldType::ImageHeight,
        _ => return Err("Unknown Geopter field type".into()),
    };
    let xs = array(&fs["X"], "field X")?;
    let ys = array(&fs["Y"], "field Y")?;
    let weights = array(&fs["Weight"], "field weights")?;
    let field_colors = array(&fs["Color"], "field colors")?;
    if xs.len() != ys.len() || xs.len() != weights.len() || xs.len() != field_colors.len() {
        return Err("Unequal Geopter field arrays".into());
    }
    let mut fields = vec![];
    for i in 0..xs.len() {
        let mut v = [0.; 4];
        for (k, label) in ["VUY", "VLY", "VUX", "VLX"].iter().enumerate() {
            let data = array(&fs[label], label)?;
            if data.len() != xs.len() {
                return Err(format!("Unequal Geopter {label} array"));
            }
            v[k] = number(&data[i], label)?;
        }
        fields.push(Field {
            x: number(&xs[i], "field X")?,
            y: number(&ys[i], "field Y")?,
            weight: number(&weights[i], "field weight")?,
            vignetting: v,
            color: color(&field_colors[i])?,
        })
    }
    let waves = array(&ws["Value"], "wavelength values")?;
    let weights = array(&ws["Weight"], "wavelength weights")?;
    let wave_colors = array(&ws["Color"], "wavelength colors")?;
    if waves.len() != weights.len() || waves.len() != wave_colors.len() {
        return Err("Unequal Geopter wavelength arrays".into());
    }
    let wavelengths = waves
        .iter()
        .zip(weights)
        .zip(wave_colors)
        .map(|((w, t), c)| {
            Ok(Wavelength {
                value: number(w, "wavelength")?,
                weight: number(t, "wavelength weight")?,
                color: color(c)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let pupil = Pupil {
        kind: match code(&spec["Pupil"]["Type"], "pupil type")? {
            0 => PupilType::EntrancePupilDiameter,
            1 => PupilType::FNumber,
            2 => PupilType::ObjectNa,
            3 => PupilType::ImageNa,
            _ => return Err("Unknown Geopter pupil type".into()),
        },
        value: number(&spec["Pupil"]["Value"], "pupil value")?,
    };
    let mut surfaces = vec![];
    for i in 1..keys.len() - 1 {
        let value = &a[&i.to_string()];
        let shape = endpoint(value)?;
        let mut surface = Surface::sphere(
            shape.radius,
            number(&value["Thickness"], "thickness")?,
            value["Material"]
                .as_str()
                .ok_or("Missing Geopter material")?,
            shape.aperture,
        );
        surface.label = value["Label"]
            .as_str()
            .ok_or("Missing Geopter surface label")?
            .into();
        surface.conic = shape.conic;
        surface.even = shape.even;
        surface.odd = shape.odd;
        surface.clear_aperture = shape.clear_aperture;
        surfaces.push(surface);
    }
    let distance = number(&object["Thickness"], "object thickness")?;
    let stop = code(&a["Stop"], "stop index")? as usize;
    if stop == 0 || stop >= keys.len() - 1 {
        return Err("Geopter stop must be a physical surface".into());
    }
    let mut system = System {
        object_surface,
        image_surface,
        name: root["Title"]
            .as_str()
            .ok_or("Missing Geopter Title")?
            .into(),
        note: root["Note"].as_str().unwrap_or("").into(),
        surfaces,
        wavelengths,
        reference_wavelength: code(&ws["RefIndex"], "reference wavelength")? as usize,
        fields,
        field_type,
        pupil,
        pupil_mapping: PupilMapping::ReferenceEntrancePupil,
        object_distance: Some(distance),
        object_material: object["Material"]
            .as_str()
            .ok_or("Missing Geopter object material")?
            .into(),
        object_label: object["Label"]
            .as_str()
            .ok_or("Missing Geopter object label")?
            .into(),
        image_label: last["Label"]
            .as_str()
            .ok_or("Missing Geopter image label")?
            .into(),
        stop_index: stop - 1,
        temperature: 25.,
        pressure: 101325.,
        custom_glasses: vec![],
    };
    system.update()?;
    Ok(system)
}
pub fn export(s: &System) -> Result<Value, String> {
    if s.surfaces
        .iter()
        .any(|surface| !surface.gap_solve.is_fixed())
    {
        return Err("Geopter JSON does not store gap solve relationships; export native JSON to preserve them".into());
    }
    if s.pupil_mapping != PupilMapping::ReferenceEntrancePupil {
        return Err("Geopter projects use reference entrance-pupil mapping; select that mapping or export native JSON".into());
    }
    if !s.custom_glasses.is_empty() {
        return Err("Geopter projects cannot embed custom glass catalogs; export native JSON to preserve them".into());
    }
    if s.temperature != 25. || s.pressure != 101325. {
        return Err("Geopter projects do not store environmental conditions; set 25 C and 101325 Pa or export native JSON".into());
    }
    if s.object_material == "FUSED_SILICA" {
        return Err(
            "Geopter cannot store the built-in FUSED_SILICA object material; export native JSON"
                .into(),
        );
    }
    let mut assembly = Map::new();
    let mut object = profile_json(
        s.object_surface.profile(),
        s.object_surface.aperture,
        s.object_surface.clear_aperture,
    )?;
    object["Thickness"] = json!(
        s.object_distance
            .ok_or("Geopter JSON requires a finite object gap")?
    );
    object["Material"] = json!(s.object_material);
    object["Label"] = json!(s.object_label);
    assembly.insert("0".into(), object);
    for (i, surface) in s.surfaces.iter().enumerate() {
        if surface.material == "FUSED_SILICA" {
            return Err(format!(
                "Geopter cannot store the built-in FUSED_SILICA material on surface {}; export native JSON",
                i + 1
            ));
        }
        if surface.interaction != Interaction::Refract {
            return Err("Geopter project export requires refractive surfaces".into());
        }
        let mut value = profile_json(surface.profile(), surface.aperture, surface.clear_aperture)?;
        value["Thickness"] = json!(surface.thickness);
        value["Material"] = json!(surface.material);
        value["Label"] = json!(surface.label);
        assembly.insert((i + 1).to_string(), value);
    }
    let mut image = profile_json(
        s.image_surface.profile(),
        s.image_surface.aperture,
        s.image_surface.clear_aperture,
    )?;
    image["Thickness"] = json!(0.);
    image["Material"] = json!("AIR");
    image["Label"] = json!(s.image_label);
    assembly.insert((s.surfaces.len() + 1).to_string(), image);
    assembly.insert("Stop".into(), json!(s.stop_index + 1));
    let field = |axis: usize| {
        s.fields
            .iter()
            .map(|f| if axis == 0 { f.x } else { f.y })
            .collect::<Vec<_>>()
    };
    let vig = |i: usize| s.fields.iter().map(|f| f.vignetting[i]).collect::<Vec<_>>();
    Ok(
        json!({"Title":s.name,"Note":s.note,"Assembly":assembly,"Spec":{"Pupil":{"Type":match s.pupil.kind{PupilType::EntrancePupilDiameter=>0,PupilType::FNumber=>1,PupilType::ObjectNa=>2,PupilType::ImageNa=>3},"Value":s.pupil.value},"Field":{"Type":match s.field_type{FieldType::Angle=>0,FieldType::ObjectHeight=>1,FieldType::ImageHeight=>2},"X":field(0),"Y":field(1),"Weight":s.fields.iter().map(|f|f.weight).collect::<Vec<_>>(),"VUY":vig(0),"VLY":vig(1),"VUX":vig(2),"VLX":vig(3),"Color":s.fields.iter().map(|f|f.color).collect::<Vec<_>>()},"Wvl":{"RefIndex":s.reference_wavelength,"Value":s.wavelengths.iter().map(|w|w.value).collect::<Vec<_>>(),"Weight":s.wavelengths.iter().map(|w|w.weight).collect::<Vec<_>>(),"Color":s.wavelengths.iter().map(|w|w.color).collect::<Vec<_>>()}}}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn geopter_export_rejects_unstored_builtin_dispersion_for_each_medium() {
        let mut s = System::default();
        s.object_distance = Some(300.);
        s.temperature = 25.;
        s.pupil_mapping = PupilMapping::ReferenceEntrancePupil;
        for position in 0..=s.surfaces.len() {
            let mut trial = s.clone();
            if position == 0 {
                trial.object_material = "FUSED_SILICA".into();
            } else {
                trial.surfaces[position - 1].material = "FUSED_SILICA".into();
            }
            trial.update().unwrap();
            let before = serde_json::to_value(&trial).unwrap();
            assert!(export(&trial).unwrap_err().contains("FUSED_SILICA"));
            assert_eq!(serde_json::to_value(trial).unwrap(), before);
        }
    }
    #[test]
    fn geopter_object_medium_labels_and_rgba_round_trip_without_rewriting() {
        let mut source: Value = serde_json::from_str(include_str!(
            "../../../reference/upstream/example/book/sasian_triplet.json"
        ))
        .unwrap();
        source["Assembly"]["0"]["Material"] = json!("1.500000:60.000000");
        source["Assembly"]["0"]["Label"] = json!("Submerged object");
        source["Assembly"]["7"]["Label"] = json!("Detector");
        source["Spec"]["Field"]["Color"] = json!([
            [0.123456789, 0.2, 0.3, 0.],
            [0.4, 0.5, 0.6, 0.7],
            [0.8, 0.9, 1., 0.25]
        ]);
        source["Spec"]["Wvl"]["Color"] = json!([
            [0.7, 0.1, 0.2, 0.],
            [0.2, 0.8, 0.3, 0.5],
            [0.3, 0.4, 0.9, 1.]
        ]);
        let system = import(&source.to_string()).unwrap();
        let output = export(&system).unwrap();
        for (section, keys) in [("0", vec!["Material", "Label"]), ("7", vec!["Label"])] {
            for key in keys {
                assert_eq!(
                    source["Assembly"][section][key],
                    output["Assembly"][section][key]
                );
            }
        }
        for spec in ["Field", "Wvl"] {
            assert_eq!(source["Spec"][spec]["Color"], output["Spec"][spec]["Color"]);
        }
        let restored = import(&output.to_string()).unwrap();
        assert_eq!(
            serde_json::to_value(system).unwrap(),
            serde_json::to_value(restored).unwrap()
        );
        for invalid in [
            json!([0., 0., 0.]),
            json!([0., 1.1, 0., 1.]),
            json!([0., 0., 0., -0.1]),
        ] {
            let mut broken = source.clone();
            broken["Spec"]["Wvl"]["Color"][1] = invalid;
            assert!(import(&broken.to_string()).is_err());
        }
    }
    #[test]
    fn endpoint_profiles_and_apertures_survive_geopter_exchange() {
        for odd_object in [false, true] {
            let mut s = System::default();
            s.object_distance = Some(300.);
            s.temperature = 25.;
            s.pupil_mapping = PupilMapping::ReferenceEntrancePupil;
            s.object_surface.radius = 500.;
            s.object_surface.conic = -0.4;
            s.object_surface.clear_aperture = true;
            s.object_surface.aperture = 40.;
            s.image_surface.radius = -100.;
            s.image_surface.conic = -1.;
            s.image_surface.clear_aperture = true;
            s.image_surface.aperture = 10.;
            if odd_object {
                s.object_surface.odd = vec![0.000001];
                s.image_surface.even = vec![0.0001];
            } else {
                s.object_surface.even = vec![0.000001];
                s.image_surface.odd = vec![0.0001];
            }
            s.update().unwrap();
            let exported = export(&s).unwrap();
            assert_eq!(
                exported["Assembly"]["0"]["Type"],
                if odd_object { "ODD" } else { "ASP" }
            );
            assert_eq!(
                exported["Assembly"]["3"]["Type"],
                if odd_object { "ASP" } else { "ODD" }
            );
            let restored = import(&exported.to_string()).unwrap();
            if odd_object {
                s.object_surface.odd.resize(10, 0.);
                s.image_surface.even.resize(10, 0.);
            } else {
                s.object_surface.even.resize(10, 0.);
                s.image_surface.odd.resize(10, 0.);
            }
            assert_eq!(
                serde_json::to_value(&restored).unwrap(),
                serde_json::to_value(&s).unwrap()
            );
            let ray = restored
                .trace_field(0., 0., &restored.fields[1], restored.reference())
                .unwrap();
            let source = ray.object_point.unwrap();
            assert!(source[2] > -300.);
            assert!(
                ray.image_hit.unwrap().point[2]
                    < restored.surfaces.iter().map(|s| s.thickness).sum::<f64>()
            );
            let mut mixed = s.clone();
            mixed.object_surface.even = vec![0.];
            mixed.object_surface.odd = vec![0.];
            assert!(export(&mixed).is_err());
            mixed = s.clone();
            mixed.image_surface.even = vec![0.];
            mixed.image_surface.odd = vec![0.];
            assert!(export(&mixed).is_err());
            for endpoint in [true, false] {
                let mut excess = s.clone();
                let e = if endpoint {
                    &mut excess.object_surface
                } else {
                    &mut excess.image_surface
                };
                e.even = vec![0.; 11];
                e.odd.clear();
                assert!(export(&excess).unwrap_err().contains("ten polynomial"));
            }
            let mut excess = s.clone();
            excess.surfaces[0].even = vec![0.; 11];
            assert!(export(&excess).unwrap_err().contains("ten polynomial"));
            let mut broken = exported;
            broken["Assembly"]["3"]["Thickness"] = json!(2.);
            assert!(import(&broken.to_string()).is_err());
        }
    }
    #[test]
    fn upstream_triplet_import_and_trace() {
        let s = import(include_str!(
            "../../../reference/upstream/example/book/sasian_triplet.json"
        ))
        .unwrap();
        assert_eq!(s.surfaces.len(), 6);
        assert_eq!(s.stop_index, 2);
        assert_eq!(s.pupil_mapping, PupilMapping::ReferenceEntrancePupil);
        assert!((s.paraxial(s.reference()).unwrap()["efl"].as_f64().unwrap() - 50.).abs() < 1.);
        for f in &s.fields {
            let ray = s.trace_field(0., 0., f, s.reference()).unwrap();
            assert!(ray.image()[1].is_finite());
        }
    }
    #[test]
    fn project_round_trip_preserves_spec() {
        let s = import(include_str!(
            "../../../reference/upstream/example/book/sasian_triplet.json"
        ))
        .unwrap();
        let text = export(&s).unwrap().to_string();
        let again = import(&text).unwrap();
        assert_eq!(
            serde_json::to_value(s).unwrap(),
            serde_json::to_value(again).unwrap()
        );
    }
    #[test]
    fn all_upstream_examples_import_size_trace_and_round_trip() {
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../reference/upstream/example");
        let mut files = vec![];
        fn collect(path: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(path).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    collect(&path, files);
                } else if path.extension().is_some_and(|e| e == "json") {
                    files.push(path);
                }
            }
        }
        collect(&root, &mut files);
        assert_eq!(files.len(), 10);
        for path in files {
            let text = std::fs::read_to_string(&path).unwrap();
            let s = import(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let report = super::super::aperture::semi_diameters(&s).unwrap();
            assert!(
                report.surfaces.iter().all(|p| p.reached_rays > 0),
                "{}: {report:?}",
                path.display()
            );
            for field in &s.fields {
                let ray = s
                    .trace_field(0., 0., field, s.reference())
                    .unwrap_or_else(|e| panic!("{} chief: {e}", path.display()));
                assert!(ray.image().iter().all(|v| v.is_finite()));
            }
            let again = import(&export(&s).unwrap().to_string()).unwrap();
            assert_eq!(
                serde_json::to_value(s).unwrap(),
                serde_json::to_value(again).unwrap(),
                "{}",
                path.display()
            );
        }
    }
}
