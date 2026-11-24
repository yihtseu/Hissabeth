// SPDX-License-Identifier: GPL-3.0-or-later
// MCP tool schemas and strict JSON-schema argument validation.
use crate::optics::bench_lab::{FORMATS, TEMPLATES};
use serde_json::{Value, json};
use std::sync::OnceLock;

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
fn opt_num(min: f64, max: f64) -> Value {
    json!({"type":["number","null"],"minimum":min,"maximum":max})
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
fn transform_schema() -> Value {
    object(
        json!({
            "decenter": array(num(-1e6, 1e6), 2, 2),
            "tilt": array(num(-360., 360.), 3, 3),
            "tilt_first": {"type": "boolean"},
            "return_after": {"type": "boolean"}
        }),
        &[],
    )
}

pub fn system_schema() -> Value {
    let finite = num(-1e15, 1e15);
    let nonnegative = num(0., 1e15);
    let surface = object(
        json!({
            "label":string(),
            "radius":finite,
            "thickness":finite,
            "material":string(),
            "aperture":num(1e-12,1e9),
            "semi_diameter":nonnegative,
            "clear_aperture":{"type":"boolean"},
            "conic":finite,
            "even":array(finite.clone(),0,16),
            "odd":array(finite.clone(),0,20),
            "toroid":object(json!({"revolution":finite,"even_y":array(finite.clone(),0,8)}),&["revolution"]),
            "zernike":object(json!({"normalize":{"type":"number","exclusiveMinimum":0.},"coefficients":array(finite.clone(),0,37)}),&["normalize"]),
            "transform":{"oneOf":[transform_schema(),{"type":"null"}]},
            "interaction":enumeration(&["refract","reflect","dummy"]),
            "gap_solve":solve_schema()
        }),
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
    let override_operand = json!({"oneOf":[
        object(json!({"kind":enumeration(&["thickness"]),"surface":integer(0,127),"value":finite}), &["kind","surface","value"]),
        object(json!({"kind":enumeration(&["radius"]),"surface":integer(0,127),"value":finite}), &["kind","surface","value"]),
        object(json!({"kind":enumeration(&["conic"]),"surface":integer(0,127),"value":finite}), &["kind","surface","value"]),
        object(json!({"kind":enumeration(&["aperture"]),"surface":integer(0,127),"value":json!({"type":"number","exclusiveMinimum":0.,"maximum":1e9})}), &["kind","surface","value"]),
        object(json!({"kind":enumeration(&["material"]),"surface":integer(0,127),"material":string()}), &["kind","surface","material"]),
        object(json!({"kind":enumeration(&["object_distance"]),"value":{"type":["number","null"],"exclusiveMinimum":0.}}), &["kind","value"])
    ]});
    let configuration = object(
        json!({"name":string(),"overrides":array(override_operand,0,256)}),
        &["name", "overrides"],
    );
    let mut schema = object(
        json!({"name":string(),"note":string(),"surfaces":array(surface,1,128),"wavelengths":array(wavelength,1,32),"reference_wavelength":integer(0,31),"fields":array(field,1,64),"field_type":enumeration(&["angle","object_height","image_height"]),"pupil":object(json!({"kind":enumeration(&["entrance_pupil_diameter","f_number","object_na","image_na"]),"value":num(1e-12,1e9)}),&["kind","value"]),"pupil_mapping":enumeration(&["reference_entrance_pupil","real_stop"]),"object_distance":{"type":["number","null"],"exclusiveMinimum":0.},"object_material":string(),"object_label":string(),"image_label":string(),"object_surface":endpoint,"image_surface":endpoint,"stop_index":integer(0,127),"temperature":num(-200.,500.),"pressure":num(0.,1e8),"custom_glasses":array(glass,0,10000),"configurations":array(configuration,0,64),"active_configuration":integer(0,63)}),
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
    );
    schema["properties"]["bench"] = json!({"oneOf":[bench_schema(),{"type":"null"}]});
    schema
}

pub fn bench_schema() -> Value {
    let xyz = array(num(-1e6, 1e6), 3, 3);
    let mount = object(
        json!({"part":string(),"base_size":array(num(1e-9,1e5),2,2),"height_range":array(num(0.,1e6),2,2)}),
        &["part", "base_size", "height_range"],
    );
    let layer = object(
        json!({"index":num(1.,4.),"thickness":num(0.,1e5)}),
        &["index", "thickness"],
    );
    let coating = object(
        json!({"layers":array(layer,0,32),"substrate_index":num(1.,4.)}),
        &["layers", "substrate_index"],
    );
    let scatter = json!({"oneOf":[
        object(json!({"model":enumeration(&["lambertian"]),"fraction":num(0.,1.),"rays":integer(1,64)}), &["model","fraction","rays"]),
        object(json!({"model":enumeration(&["abg"]),"a":num(0.,1e3),"b":json!({"type":"number","exclusiveMinimum":0.,"maximum":1e3}),"g":json!({"type":"number","exclusiveMinimum":0.,"maximum":10.}),"rays":integer(1,64)}), &["model","a","b","g","rays"]),
        {"type":"null"}
    ]});
    let mut element_props = json!({"id":string(),"label":string(),"kind":enumeration(&["mirror","curved_mirror","lens","lens_group","splitter","pbs","waveplate","polarizer","detector","aperture","blocker"]),"position":xyz,"rotation":xyz,"aperture":num(1e-9,1e5),"body_radius":num(1e-9,1e5),"focal_length":num(-1e6,1e6),"reflectance":num(0.,1.),"transmittance":num(0.,1.),"mount":{"oneOf":[mount,{"type":"null"}]}});
    element_props["index"] = json!({"type":["number","null"],"minimum":1.,"maximum":4.});
    element_props["coating"] = json!({"oneOf":[coating,{"type":"null"}]});
    element_props["scatter"] = scatter;
    element_props["ghosts"] = integer(0, 2);
    element_props["pixels"] = json!({"oneOf":[array(integer(2,128),2,2),{"type":"null"}]});
    element_props["retardance_waves"] = opt_num(0., 10.);
    element_props["fast_axis_deg"] = opt_num(-360., 360.);
    element_props["extinction_ratio"] = opt_num(1., 1e9);
    element_props["screw_tpi"] = opt_num(10., 500.);
    element_props["lever_arm_mm"] = opt_num(2., 200.);
    let element = object(
        element_props,
        &[
            "id",
            "label",
            "kind",
            "position",
            "rotation",
            "aperture",
            "body_radius",
            "focal_length",
            "reflectance",
            "transmittance",
            "mount",
        ],
    );
    let polarization = json!({"oneOf":[
        object(json!({"kind":enumeration(&["unpolarized","horizontal","vertical","diagonal45","right_circular","left_circular"])}), &["kind"]),
        object(json!({"kind":enumeration(&["linear"]),"angle_deg":{"type":"number"}}), &["kind","angle_deg"]),
        {"type":"null"}
    ]});
    let source = object(
        json!({"id":string(),"position":xyz,"direction":xyz,"wavelength":num(100.,20000.),"power":num(1e-12,1e6),"beam_radius":num(0.,1e5),"rings":integer(0,8),"waist":{"type":["number","null"],"exclusiveMinimum":0.},"waist_offset":{"type":["number","null"]},"m2":{"type":["number","null"],"minimum":1.},"polarization":polarization}),
        &[
            "id",
            "position",
            "direction",
            "wavelength",
            "power",
            "beam_radius",
            "rings",
        ],
    );
    let connection = object(
        json!({"from":string(),"port":enumeration(&["out","reflected","transmitted"]),"to":string()}),
        &["from", "port", "to"],
    );
    object(
        json!({"name":string(),"table_size":array(num(1e-9,1e6),2,2),"hole_pitch":num(1e-9,1e6),"origin":xyz,"elements":array(element,0,128),"sources":array(source,1,16),"connections":array(connection,0,384),"max_interactions":integer(1,128),"min_power":num(0.,1.)}),
        &[
            "name",
            "table_size",
            "hole_pitch",
            "origin",
            "elements",
            "sources",
            "connections",
            "max_interactions",
            "min_power",
        ],
    )
}

fn optimize_variable_schema() -> Value {
    let bound = opt_num(-1e12, 1e12);
    json!({"oneOf":[
        object(json!({"kind":enumeration(&["radius"]),"surface":integer(0,127),"min":bound,"max":bound}), &["kind","surface"]),
        object(json!({"kind":enumeration(&["thickness"]),"surface":integer(0,127),"min":bound,"max":bound}), &["kind","surface"]),
        object(json!({"kind":enumeration(&["conic"]),"surface":integer(0,127),"min":bound,"max":bound}), &["kind","surface"]),
        object(json!({"kind":enumeration(&["even_asphere"]),"surface":integer(0,127),"index":integer(0,15),"min":bound,"max":bound}), &["kind","surface","index"]),
        object(json!({"kind":enumeration(&["odd_asphere"]),"surface":integer(0,127),"index":integer(0,19),"min":bound,"max":bound}), &["kind","surface","index"]),
        object(json!({"kind":enumeration(&["model_glass_nd"]),"surface":integer(0,127),"min":bound,"max":bound}), &["kind","surface"]),
        object(json!({"kind":enumeration(&["model_glass_vd"]),"surface":integer(0,127),"min":bound,"max":bound}), &["kind","surface"]),
        object(json!({"kind":enumeration(&["decenter"]),"surface":integer(0,127),"axis":enumeration(&["x","y"]),"min":bound,"max":bound}), &["kind","surface","axis"]),
        object(json!({"kind":enumeration(&["tilt"]),"surface":integer(0,127),"axis":enumeration(&["x","y","z"]),"min":bound,"max":bound}), &["kind","surface","axis"]),
        object(json!({"kind":enumeration(&["config_thickness"]),"configuration":integer(0,63),"surface":integer(0,127),"min":bound,"max":bound}), &["kind","configuration","surface"]),
        object(json!({"kind":enumeration(&["config_radius"]),"configuration":integer(0,63),"surface":integer(0,127),"min":bound,"max":bound}), &["kind","configuration","surface"])
    ]})
}

fn optimize_operand_schema() -> Value {
    let finite = num(-1e12, 1e12);
    let weight = num(0., 1e9);
    let mode = enumeration(&["eq", "lt", "gt"]);
    let wave = opt_num(100., 20000.);
    let cfg = json!({"type":["integer","null"],"minimum":0,"maximum":63});
    let field_opt = json!({"type":["integer","null"],"minimum":0,"maximum":63});
    let samp_opt = json!({"oneOf":[sampling_schema(),{"type":"null"}]});
    json!({"oneOf":[
        object(json!({"kind":enumeration(&["rms_spot"]),"field_index":field_opt,"wavelength":wave,"sampling":samp_opt,"target":finite,"weight":weight,"mode":mode,"configuration":cfg}), &["kind"]),
        object(json!({"kind":enumeration(&["rms_wavefront"]),"field_index":integer(0,63),"wavelength":wave,"grid":json!({"type":["integer","null"],"minimum":5,"maximum":65}),"target":finite,"weight":weight,"mode":mode,"configuration":cfg}), &["kind"]),
        object(json!({"kind":enumeration(&["efl"]),"wavelength":wave,"target":finite,"weight":weight,"mode":mode,"configuration":cfg}), &["kind","target"]),
        object(json!({"kind":enumeration(&["bfl"]),"wavelength":wave,"target":finite,"weight":weight,"mode":mode,"configuration":cfg}), &["kind","target"]),
        object(json!({"kind":enumeration(&["working_f_number"]),"wavelength":wave,"target":finite,"weight":weight,"mode":mode,"configuration":cfg}), &["kind","target"]),
        object(json!({"kind":enumeration(&["total_track"]),"first":json!({"type":["integer","null"],"minimum":0,"maximum":127}),"last":json!({"type":["integer","null"],"minimum":1,"maximum":128}),"target":finite,"weight":weight,"mode":mode,"configuration":cfg}), &["kind","target"]),
        object(json!({"kind":enumeration(&["center_thickness"]),"surface":integer(0,127),"target":finite,"weight":weight,"mode":mode,"configuration":cfg}), &["kind","surface","target"]),
        object(json!({"kind":enumeration(&["edge_thickness"]),"surface":integer(0,127),"height":opt_num(0.,1e6),"target":finite,"weight":weight,"mode":mode,"configuration":cfg}), &["kind","surface","target"]),
        object(json!({"kind":enumeration(&["distortion"]),"field_index":integer(0,63),"wavelength":wave,"target":finite,"weight":weight,"mode":mode,"configuration":cfg}), &["kind","field_index"]),
        object(json!({"kind":enumeration(&["axial_color"]),"wave1":num(100.,20000.),"wave2":num(100.,20000.),"target":finite,"weight":weight,"mode":mode,"configuration":cfg}), &["kind","wave1","wave2"]),
        object(json!({"kind":enumeration(&["lateral_color"]),"field_index":integer(0,63),"wave1":num(100.,20000.),"wave2":num(100.,20000.),"target":finite,"weight":weight,"mode":mode,"configuration":cfg}), &["kind","field_index","wave1","wave2"]),
        object(json!({"kind":enumeration(&["chief_ray_angle"]),"field_index":integer(0,63),"wavelength":wave,"target":finite,"weight":weight,"mode":mode,"configuration":cfg}), &["kind","field_index"]),
        object(json!({"kind":enumeration(&["ray_intercept"]),"field_index":integer(0,63),"px":num(-2.,2.),"py":num(-2.,2.),"wavelength":wave,"axis":enumeration(&["x","y"]),"target":finite,"weight":weight,"mode":mode,"configuration":cfg}), &["kind","field_index","px","py","axis"])
    ]})
}

fn tolerance_operand_schema() -> Value {
    json!({"oneOf":[
        object(json!({"kind":enumeration(&["radius"]),"surface":integer(0,127),"delta_percent":opt_num(1e-9,90.),"delta_mm":opt_num(1e-9,1e6)}), &["kind","surface"]),
        object(json!({"kind":enumeration(&["thickness"]),"surface":integer(0,127),"delta":num(1e-9,1e4)}), &["kind","surface","delta"]),
        object(json!({"kind":enumeration(&["decenter"]),"surface":integer(0,127),"delta":num(1e-9,1e4)}), &["kind","surface","delta"]),
        object(json!({"kind":enumeration(&["tilt"]),"surface":integer(0,127),"delta_deg":num(1e-9,45.)}), &["kind","surface","delta_deg"]),
        object(json!({"kind":enumeration(&["index"]),"surface":integer(0,127),"delta":num(1e-9,0.5)}), &["kind","surface","delta"]),
        object(json!({"kind":enumeration(&["abbe"]),"surface":integer(0,127),"delta_percent":num(1e-9,50.)}), &["kind","surface","delta_percent"]),
        object(json!({"kind":enumeration(&["element_decenter"]),"first":integer(0,127),"last":integer(0,127),"delta":num(1e-9,1e4)}), &["kind","first","last","delta"]),
        object(json!({"kind":enumeration(&["element_tilt"]),"first":integer(0,127),"last":integer(0,127),"delta_deg":num(1e-9,45.)}), &["kind","first","last","delta_deg"])
    ]})
}

fn compensator_schema() -> Value {
    json!({"oneOf":[
        object(json!({"kind":enumeration(&["none"])}), &["kind"]),
        object(json!({"kind":enumeration(&["autofocus"]),"wavelength":opt_num(100.,20000.),"max_delta":opt_num(0.,1e5)}), &["kind"])
    ]})
}

fn criterion_schema() -> Value {
    let wave = opt_num(100., 20000.);
    let samp_opt = json!({"oneOf":[sampling_schema(),{"type":"null"}]});
    let field_opt = json!({"type":["integer","null"],"minimum":0,"maximum":63});
    json!({"oneOf":[
        object(json!({"kind":enumeration(&["rms_spot"]),"field_index":field_opt,"wavelength":wave,"sampling":samp_opt}), &["kind"]),
        object(json!({"kind":enumeration(&["rms_wavefront"]),"field_index":integer(0,63),"wavelength":wave,"grid":json!({"type":["integer","null"],"minimum":5,"maximum":65})}), &["kind"]),
        object(json!({"kind":enumeration(&["efl"]),"wavelength":wave}), &["kind"]),
        object(json!({"kind":enumeration(&["geometric_mtf"]),"field_index":integer(0,63),"frequency":num(1e-6,10000.),"axis":json!({"type":["string","null"],"enum":["x","y",null]}),"wavelength":wave,"sampling":samp_opt}), &["kind","frequency"])
    ]})
}

pub fn tools() -> Value {
    tool_list().clone()
}

fn tool_list() -> &'static Value {
    static TOOLS: OnceLock<Value> = OnceLock::new();
    TOOLS.get_or_init(build_tools)
}

fn build_tools() -> Value {
    let mut list = vec![];
    let mut add = |name: &str, description: &str, properties: Value, required: &[&str]| {
        list.push(json!({"name":name,"description":description,"inputSchema":object(properties,required)}))
    };
    let wave = num(100., 20000.);
    let revision = integer(0, 9_007_199_254_740_991);

    add(
        "bench_get",
        "Read the saved free-space laser bench and shared project revision. Table XY; Z height; mm; rotations [roll,pitch,yaw] in degrees.",
        json!({}),
        &[],
    );
    add(
        "bench_example",
        "Get a read-only two-arm laser bench example. Nothing is saved until bench_set or system_set.",
        json!({}),
        &[],
    );
    add(
        "bench_set",
        "Atomically save a validated free-space bench in the current project. Requires the current shared project revision. Retains the sequential prescription.",
        json!({"bench":bench_schema(),"revision":revision}),
        &["bench", "revision"],
    );
    add(
        "bench_trace",
        "Trace the saved bench or supplied draft without writing. Nearest surface/mount hits, optional intended port connections, ideal thin lenses and lens_group instances of the current prescription, bounded ray trees and complete power accounting. A lens_group position is its first vertex; local +X is prescription +Z. Groups require forward refractive positive-gap prescriptions with air at both boundaries; their scalar transmittance is applied once. Splitters may set index for polarization-averaged Fresnel splitting; mirrors and splitters may carry a thin-film coating stack. Mirrors, splitters and thin lenses may scatter (lambertian or ABg) into deterministic hemisphere samples; scattered power leaves the specular child. Group ghosts enumerate internal Fresnel bounces (0-2) per surface; shares beyond the bounce budget are truncated to conserve power, and ghost segments skip internal obstruction checks. Detectors with pixels record irradiance maps. Terminations and segments carry the full branch power tree. Jones polarization drives waveplates, polarizers and PBS outputs. Curved mirrors use a paraxial focusing reflection. The geometric ledger adds branch powers incoherently.",
        json!({"bench":bench_schema()}),
        &[],
    );
    add(
        "bench_assembly",
        "Read assembly coordinates, surface normals, holes/offsets, mounts, BOM and clearance diagnostics from the saved bench or supplied draft. Missing hardware is unresolved. No writes.",
        json!({"bench":bench_schema()}),
        &[],
    );
    add(
        "bench_catalog",
        "Search representative lab optics and mounting assemblies. Includes model parameters, clamping metadata and approximate planning prices.",
        json!({"query":string(),"kind":string()}),
        &[],
    );
    add(
        "bench_template",
        "Preview a mounted lab bench template. Set apply=true with the current revision to save it as the project bench.",
        json!({"name":enumeration(TEMPLATES),"apply":{"type":"boolean"},"revision":revision}),
        &["name"],
    );
    add(
        "bench_auto_aim",
        "Aim a draft or saved bench from its nominal connections. Optionally snap elements to the hole grid and set a common world Z. Set apply=true with the current revision to save.",
        json!({"bench":bench_schema(),"snap_holes":{"type":"boolean"},"uniform_z":{"type":"number","exclusiveMinimum":0.,"maximum":1e6},"apply":{"type":"boolean"},"revision":revision}),
        &[],
    );
    add(
        "bench_lab_check",
        "Review optical terminations, Gaussian aperture clearance, mount collisions and hole access. Recommends dump positions for escaped beams.",
        json!({"bench":bench_schema()}),
        &[],
    );
    add(
        "bench_alignment_guide",
        "Read ordered installation steps, expected beam radii, and quarter-turn screw and spot-shift estimates.",
        json!({"bench":bench_schema()}),
        &[],
    );
    add(
        "bench_export_blueprint",
        "Return a lab blueprint or alignment SOP with its filename, MIME type and text content.",
        json!({"bench":bench_schema(),"format":enumeration(FORMATS)}),
        &["format"],
    );
    add(
        "workspace_status",
        "Read the workspace state before requesting optical data. A recovery state is connected but has no active optical system.",
        json!({}),
        &[],
    );
    add(
        "workspace_retry",
        "While recovery is active, reread a user-repaired saved file under the current workspace lease. A missing or invalid file stays in recovery. Requires its current recovery revision.",
        json!({"revision":revision}),
        &["revision"],
    );
    add(
        "workspace_recover",
        "While recovery is active, validate a chosen native/Geopter/ZMX project or create a new system. Preserve the original file before installing the chosen system; reject changed disk contents and stale recovery revisions. No optical tool can write through recovery.",
        json!({"revision":revision,"source":{"oneOf":[object(json!({"kind":enumeration(&["default"])}),&["kind"]),object(json!({"kind":enumeration(&["project"]),"format":enumeration(&["hissabeth","geopter","zmx"]),"text":string()}),&["kind","format","text"])]}}),
        &["revision", "source"],
    );
    add(
        "system_get",
        "Read the entire current optical prescription and revision",
        json!({}),
        &[],
    );
    add(
        "system_set",
        "Atomically save the complete prescription; revision must match system_get",
        json!({"system":system_schema(),"revision":revision}),
        &["system", "revision"],
    );
    add(
        "system_validate",
        "Validate a complete prescription without changing the workspace",
        json!({"system":system_schema()}),
        &["system"],
    );
    add(
        "system_resolved",
        "Read the prescription with the active configuration's overrides applied: the exact system every analysis tool evaluates. Read-only; authoring fields stay on system_get",
        json!({}),
        &[],
    );
    add(
        "configuration_activate",
        "Select the active multi-configuration by zero-based index and save atomically. All analysis tools then evaluate that configuration. Requires the current revision",
        json!({"index":integer(0,63),"revision":revision}),
        &["index", "revision"],
    );
    add(
        "surface_edit",
        "Stage inserted or removed internal surface rows in a supplied draft without saving, solving or tracing. Zero-based before may equal the row count (before Image). Remaps stop and retained gap references; rejects deletion of gaps referenced by retained solves. Returns the transformed system, old_to_new indices, inserted indices and stop_reassigned.",
        json!({"system":system_schema(),"operation":{"oneOf":[
            object(json!({"kind":enumeration(&["insert"]),"before":integer(0,128),"count":integer(1,128)}),&["kind","before","count"]),
            object(json!({"kind":enumeration(&["remove"]),"first":integer(0,127),"count":integer(1,128)}),&["kind","first","count"])
        ]}}),
        &["system", "operation"],
    );
    add(
        "design_summary",
        "High-density optical summary returning first-order metrics, polychromatic per-field RMS spot radii, distortion, axial color, and minimum center/edge thicknesses in one call.",
        json!({}),
        &[],
    );
    add(
        "optimize",
        "Run the native Damped Least Squares (Levenberg-Marquardt) sequential optical optimizer over chosen variables (radius, thickness, conic, asphere, model glass, tilt/decenter, configuration overrides) and merit function operands (RMS spot, wavefront, EFL, BFL, F/#, track, center/edge thickness, distortion, color, chief ray angle, ray intercept). Set apply=true with the current revision to commit the optimized system atomically; otherwise returns a dry-run preview.",
        json!({
            "variables": array(optimize_variable_schema(), 1, 64),
            "operands": array(optimize_operand_schema(), 1, 256),
            "max_iterations": integer(1, 200),
            "initial_damping": json!({"type":"number","exclusiveMinimum":0.,"maximum":1e6}),
            "tolerance": json!({"type":"number","exclusiveMinimum":0.,"maximum":1.}),
            "apply": {"type": "boolean"},
            "revision": revision
        }),
        &["variables", "operands"],
    );
    add(
        "tolerance_sensitivity",
        "Evaluate per-operand plus/minus tolerance sensitivities (radius, thickness, decenter, tilt, index, Abbe, element decenter/tilt) with optional autofocus compensation, sorted by worst offenders plus RSS estimate.",
        json!({
            "tolerances": array(tolerance_operand_schema(), 1, 256),
            "compensator": compensator_schema(),
            "criterion": criterion_schema()
        }),
        &["tolerances", "criterion"],
    );
    add(
        "tolerance_monte_carlo",
        "Run parallel seeded Monte Carlo tolerance trials with optional autofocus compensator, returning distribution statistics (mean, std_dev, min/max, p50/p80/p90/p95/p98), pass yield rate, compensator stroke range, and histogram.",
        json!({
            "tolerances": array(tolerance_operand_schema(), 1, 256),
            "compensator": compensator_schema(),
            "criterion": criterion_schema(),
            "trials": integer(10, 2000),
            "distribution": enumeration(&["normal", "uniform"]),
            "seed": integer(0, 9_007_199_254_740_991),
            "pass_threshold": opt_num(-1e12, 1e12),
            "lower_is_better": {"type": ["boolean", "null"]}
        }),
        &["tolerances", "criterion"],
    );
    add(
        "history_list",
        "List the in-memory revision history snapshots of the current workspace, including undo/redo availability.",
        json!({}),
        &[],
    );
    add(
        "history_undo",
        "Undo the latest workspace mutation by restoring the previous prescription and project link. Requires the current revision.",
        json!({"revision": revision}),
        &["revision"],
    );
    add(
        "history_redo",
        "Redo a previously undone workspace mutation atomically. Requires the current revision.",
        json!({"revision": revision}),
        &["revision"],
    );
    add(
        "history_restore",
        "Restore a historical prescription and project link by its zero-based index from history_list. The restore is undoable and requires the current revision.",
        json!({"index": integer(0, 63), "revision": revision}),
        &["index", "revision"],
    );
    add(
        "history_compare",
        "Compare two revision history snapshots (default: previous vs current), returning optical metric deltas (EFL, BFL, F/#, RMS spot) and per-surface prescription diffs.",
        json!({"from_index": integer(0, 63), "to_index": integer(0, 63)}),
        &[],
    );
    add(
        "project_list",
        "List the active linked project file, saved projects in the workspace projects directory, and recent project paths.",
        json!({}),
        &[],
    );
    add(
        "project_open",
        "Open a native Hissabeth JSON or ZMX project file from disk, validate it, link native JSON files for subsequent saves, and update recent projects. Requires the current revision.",
        json!({"path": string(), "revision": revision}),
        &["path", "revision"],
    );
    add(
        "project_save_as",
        "Atomically save the current prescription to a file path on disk (relative paths resolve inside the workspace projects directory) and link future saves to it. Requires the current revision.",
        json!({"path": string(), "revision": revision}),
        &["path", "revision"],
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
    let pair = json!({"type":"array","items":{"type":"number"},"minItems":2,"maxItems":2});
    add(
        "trace_inspect",
        "Inspect a normalized pupil ray or a finite source XY / (dx/dz,dy/dz) launch with positive initial z direction; return partial paths and physical failures. Pupil rays use saved mapping and vignetting; object rays use the actual curved source without pupil aiming.",
        json!({"wavelength":wave,"launch":{"oneOf":[
            object(json!({"kind":enumeration(&["pupil"]),"pupil":pair,"field_index":integer(0,63)}), &["kind","pupil","field_index"]),
            object(json!({"kind":enumeration(&["object"]),"point":pair,"tangent":pair}), &["kind","point","tangent"])
        ]}}),
        &["launch", "wavelength"],
    );
    add(
        "paraxial_trace",
        "Per-vertex linearized axial and stored-field chief ray tables in global x/y; signed reduced angles include centered reflections. No aperture clipping. The image row uses its vertex plane.",
        json!({"wavelength":wave,"field_index":integer(0,63)}),
        &["wavelength", "field_index"],
    );
    add(
        "field_curvature",
        "Continuous radial field sweep of analytic sagittal/tangential chief-ray foci for every stored wavelength, including zero weights and duplicates. Default endpoint is +Y at the maximum stored field radius; optional field_end selects a 2D slice. Shifts reference the stored image vertex; retain per-branch failures.",
        json!({"field_end":pair,"samples":integer(2,257)}),
        &["samples"],
    );
    add(
        "prescription_report",
        "Assemble selected saved-prescription sections as structured tables and the same canonical text. Includes endpoints, first-order/cardinal quantities, actual media, signed gaps, profiles and gap relations. Per-cell errors preserve other data; no sections returns header only. Does not mutate the system.",
        json!({"sections":array(enumeration(&["general","specification","first_order","lens_data","surface_data"]),0,5),"wavelength":wave,"precision":integer(0,15)}),
        &["sections", "wavelength", "precision"],
    );
    let common = json!({"wavelength":wave,"field_index":integer(0,63),"field":num(-1e9,1e9),"field_x":num(-1e9,1e9)});
    for (name, description) in [
        (
            "paraxial",
            "Compute first-order matrix, focal lengths, image conjugate, magnification and pupils. Afocal systems retain defined quantities; absent finite focal lengths, image conjugate and working F-number are null.",
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
        (
            "wavefront_zernike",
            "Least-squares Noll Zernike decomposition of the traced pupil wavefront; unit-RMS disk normalization, coefficients in waves",
        ),
        (
            "huygens_psf",
            "Unit-energy PSF from a direct Huygens-Fresnel sum over traced pupil samples at arbitrary image sampling; paraxial exit pupil mapping",
        ),
        (
            "huygens_mtf",
            "Field-local diffraction MTF from the direct Huygens PSF with the same complex OTF composition as the FFT path",
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
        if ["geometric_mtf", "diffraction_mtf", "huygens_mtf"].contains(&name) {
            props.as_object_mut().unwrap().remove("wavelength");
            props["spectrum"] = spectrum_schema();
            required.push("spectrum");
        }
        if name == "polychromatic_spot" {
            props.as_object_mut().unwrap().remove("wavelength");
        }
        if [
            "wavefront",
            "diffraction_psf",
            "diffraction_mtf",
            "wavefront_zernike",
            "huygens_psf",
            "huygens_mtf",
        ]
        .contains(&name)
        {
            props["grid"] = match name {
                "diffraction_psf" | "diffraction_mtf" => {
                    json!({"type":"integer","enum":[8,16,32,64,128]})
                }
                "wavefront" | "wavefront_zernike" | "huygens_psf" | "huygens_mtf" => {
                    integer(5, 129)
                }
                _ => integer(3, 129),
            };
        }
        if name == "wavefront_zernike" {
            props["terms"] = integer(1, 45);
        }
        if name == "huygens_psf" {
            props["size"] = integer(3, 129);
            props["pitch"] = json!({"type":"number","exclusiveMinimum":0.});
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
        if name == "diffraction_mtf" || name == "huygens_mtf" {
            props["max_frequency"] = json!({"type":"number","exclusiveMinimum":0.});
        }
        add(name, description, props, &required);
    }
    add(
        "gaussian_beam",
        "Paraxial Gaussian beam q-parameter propagation through the reduced system matrix: per-surface 1/e^2 radii plus image-plane waist, curvature, Rayleigh range and divergence. Input waist_z is the global z of the input waist (first vertex at 0). Unclipped, unaberrated, on-axis",
        json!({"wavelength":wave,"waist":num(1e-6,1e3),"waist_z":num(-1e9,1e9)}),
        &["waist"],
    );
    add(
        "fiber_coupling",
        "Gaussian overlap coupling efficiency at the stored image plane from the propagated beam into a flat-phase fiber mode of given mode field diameter; also reports the optimum MFD and its efficiency. No offsets, tilt or aberration",
        json!({"wavelength":wave,"waist":num(1e-6,1e3),"waist_z":num(-1e9,1e9),"mode_field_diameter":num(1e-6,1e3)}),
        &["waist", "mode_field_diameter"],
    );
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
        "project_new",
        "Replace the saved project with a minimum blank prescription: one plane air stop, unclipped plane endpoints, infinity source, one on-axis field, d-line and a 10 mm entrance pupil. Clears prior project data. Requires the current revision; creation is atomic and resolves the new model before saving.",
        json!({"revision":revision}),
        &["revision"],
    );
    add(
        "project_import",
        "Import native or Geopter project text and atomically replace current prescription",
        json!({"format":enumeration(&["geopter","hissabeth","zmx"]),"text":string(),"revision":revision}),
        &["format", "text", "revision"],
    );
    add(
        "project_export",
        "Export the current prescription as native or Geopter JSON text",
        json!({"format":enumeration(&["geopter","hissabeth","zmx"])}),
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

pub fn validate_schema(value: &Value, schema: &Value, path: &str) -> Result<(), String> {
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

pub fn validate_args(name: &str, args: &Value) -> Result<(), String> {
    let tool = tool_list()
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
    validate_schema(args, &tool["inputSchema"], "arguments")?;
    if matches!(name, "optimize" | "bench_template" | "bench_auto_aim")
        && args["apply"] == true
        && args.get("revision").is_none()
    {
        return Err("Missing arguments.revision".into());
    }
    Ok(())
}
