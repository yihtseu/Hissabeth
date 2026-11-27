//! Shared command contract for desktop, broker, and direct engine calls.
use super::*;
use schemars::JsonSchema;
use serde::{Serialize, de::DeserializeOwned};
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Read,
    Mutate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// File operation exposed to the caller. Managed workspace persistence is
/// implicit in mutations, including writes to the already linked project.
pub enum FileAccess {
    None,
    Read,
    Write,
    /// The step commands determine the requested file operations.
    Steps,
}

type Handler = dyn Fn(Target<'_>, Value) -> Result<Value, String> + Send + Sync;

pub struct CommandSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub access: Access,
    pub meta: bool,
    pub files: FileAccess,
    pub params_schema: Value,
    pub result_schema: fn() -> Value,
    handler: Box<Handler>,
}

impl CommandSpec {
    pub(super) fn call(&self, target: Target<'_>, args: Value) -> Result<Value, String> {
        self.validate(&args)?;
        (self.handler)(target, args)
    }

    fn validate(&self, args: &Value) -> Result<(), String> {
        if args.get("field_index").is_some()
            && (args.get("field").is_some() || args.get("field_x").is_some())
        {
            return Err("Choose a stored field_index or explicit field coordinates".into());
        }
        schema::validate_schema(args, &self.params_schema, "arguments")?;
        if self.access == Access::Mutate && args["apply"] == true && args.get("revision").is_none()
        {
            return Err("Missing arguments.revision".into());
        }
        Ok(())
    }
}

fn command<P: DeserializeOwned + JsonSchema + 'static, R: Serialize + JsonSchema + 'static>(
    name: &'static str,
    access: Access,
    files: FileAccess,
    description: &'static str,
    run: impl Fn(Target<'_>, P) -> Result<R, String> + Send + Sync + 'static,
) -> CommandSpec {
    CommandSpec {
        name,
        description,
        access,
        meta: false,
        files,
        params_schema: schema::derived::<P>(),
        result_schema: schema::result::<R>,
        handler: Box::new(move |target, args| {
            let params = serde_json::from_value(args).map_err(|e| e.to_string())?;
            serde_json::to_value(run(target, params)?).map_err(|e| e.to_string())
        }),
    }
}

pub fn lookup(name: &str) -> Result<&'static CommandSpec, String> {
    registry()
        .iter()
        .find(|spec| spec.name == name)
        .ok_or_else(|| format!("Unknown tool {name}"))
}

#[derive(Serialize, JsonSchema)]
pub(super) struct ToolDescription {
    name: &'static str,
    description: &'static str,
    #[serde(rename = "inputSchema")]
    input_schema: &'static Value,
}

pub(super) fn descriptions() -> Vec<ToolDescription> {
    registry()
        .iter()
        .map(|spec| ToolDescription {
            name: spec.name,
            description: spec.description,
            input_schema: &spec.params_schema,
        })
        .collect()
}

pub fn tools() -> Value {
    json!(descriptions())
}

pub fn validate_args(name: &str, args: &Value) -> Result<(), String> {
    lookup(name)?.validate(args)
}

pub(super) fn call(target: Target<'_>, name: &str, args: Value) -> Result<Value, String> {
    lookup(name)?.call(target, args)
}

pub(super) enum Target<'a> {
    Shared(&'a SharedWorkspace, Option<&'a ChangeNotifier>),
    Workspace(&'a mut Workspace),
    Engine(&'a mut Engine),
}

pub(super) enum Store<'a> {
    Workspace(&'a mut Workspace),
    Engine(&'a mut Engine),
}

impl<'a> Store<'a> {
    pub(super) fn engine(self) -> Result<&'a mut Engine, String> {
        match self {
            Self::Workspace(w) => w.engine_mut(),
            Self::Engine(e) => Ok(e),
        }
    }
    pub fn workspace(self, name: &str) -> Result<&'a mut Workspace, String> {
        match self {
            Self::Workspace(w) => Ok(w),
            Self::Engine(_) => Err(format!("Unknown tool {name}")),
        }
    }
}

impl Target<'_> {
    // Shared dispatch permits standalone bench previews even during recovery.
    // Direct Workspace calls have always required a ready engine for optical tools.
    pub fn standalone(&self) -> Result<(), String> {
        if let Self::Workspace(w) = self {
            w.engine()?;
        }
        Ok(())
    }

    pub fn read<R>(&self, f: impl FnOnce(&Engine) -> Result<R, String>) -> Result<R, String> {
        match self {
            Self::Shared(shared, _) => f(shared.read().map_err(|e| e.to_string())?.engine()?),
            Self::Workspace(w) => f(w.engine()?),
            Self::Engine(e) => f(e),
        }
    }

    // Clone under the shared lock, then release it before optical computation.
    pub fn system<R>(
        &self,
        f: impl FnOnce(&System, u64) -> Result<R, String>,
    ) -> Result<R, String> {
        match self {
            Self::Shared(_, _) => {
                let (system, revision) = self.read(|e| Ok((e.system.clone(), e.revision)))?;
                f(&system, revision)
            }
            _ => self.read(|e| f(&e.system, e.revision)),
        }
    }

    pub fn write<R>(self, f: impl FnOnce(Store<'_>) -> Result<R, String>) -> Result<R, String> {
        match self {
            Self::Engine(e) => f(Store::Engine(e)),
            Self::Workspace(w) => f(Store::Workspace(w)),
            Self::Shared(shared, notify) => {
                let mut guard = shared.write().map_err(|e| e.to_string())?;
                let before = guard.revision();
                let result = f(Store::Workspace(&mut guard))?;
                let after = guard.revision();
                drop(guard);
                if let Some(notify) = notify.filter(|_| before != after) {
                    notify(after);
                }
                Ok(result)
            }
        }
    }

    pub fn edit<R>(
        self,
        revision: u64,
        f: impl FnOnce(&mut Engine) -> Result<R, String>,
    ) -> Result<R, String> {
        self.write(|store| {
            let engine = store.engine()?;
            engine.check_revision(revision)?;
            f(engine)
        })
    }
}

macro_rules! tool {
    (Meta, $handler:ident, $access:ident, $files:ident, $description:literal) => {
        CommandSpec {
            meta: true,
            ..tool!($handler, $access, $files, $description)
        }
    };
    ($handler:ident, Read, $files:ident, $description:literal) => {
        command(
            stringify!($handler),
            Access::Read,
            FileAccess::$files,
            $description,
            |target, params| commands::$handler(&target, params),
        )
    };
    ($handler:ident, Mutate, $files:ident, $description:literal) => {
        command(
            stringify!($handler),
            Access::Mutate,
            FileAccess::$files,
            $description,
            commands::$handler,
        )
    };
}

pub fn registry() -> &'static [CommandSpec] {
    static TOOLS: LazyLock<Vec<CommandSpec>> = LazyLock::new(|| {
        vec![
            tool!(Meta, batch, Mutate, Steps, "Run ordered optical commands under one workspace lock. Supply an optional starting revision. Mutations inherit the current step revision. Stop at the first error; rollback defaults to true. History, workspace control, capabilities and nested batch commands are excluded."),
            tool!(
                bench_get,
                Read,
                None,
                "Read the saved free-space laser bench and shared project revision. Table XY; Z height; mm; rotations [roll,pitch,yaw] in degrees."
            ),
            tool!(
                bench_example,
                Read,
                None,
                "Get a read-only two-arm laser bench example. Nothing is saved until bench_set or system_set."
            ),
            tool!(
                bench_set,
                Mutate,
                None,
                "Atomically save a free-space bench at the current shared project revision. Stage members use fixed reference poses and translate along a unit table-frame axis by position_mm + pzt.value_um/1000. Set travel in mm and the optional PZT range in um. Staged benches return a fresh trace. Other elements with omitted or null rotation require bench_auto_aim before saving. Branches use output from, input to, and optional intermediate output selections via."
            ),
            tool!(
                bench_trace,
                Read,
                None,
                "Trace the saved bench or supplied draft without writing. Nearest surface/mount hits, optional intended port connections, ideal thin lenses and lens_group instances of the current prescription, bounded ray trees and complete power accounting. A lens_group position is its first vertex; local +X is prescription +Z. Groups require forward refractive positive-gap prescriptions with air at both boundaries; their scalar transmittance is applied once. Splitters may set index for polarization-averaged Fresnel splitting; mirrors and splitters may carry a thin-film coating stack. Mirrors, splitters and thin lenses may scatter (lambertian or ABg) into deterministic hemisphere samples; scattered power leaves the specular child. Group ghosts enumerate internal Fresnel bounces (0-2) per surface; shares beyond the bounce budget are truncated to conserve power, and ghost segments skip internal obstruction checks. Detectors with pixels record irradiance maps. Physical segments record ports, media, wavelength and OPL and link to their preceding segment by index. Named branches aggregate on-axis specular paths between their declared ports. Jones polarization drives waveplates, polarizers and PBS outputs. Curved mirrors use a paraxial focusing reflection. The geometric ledger adds branch powers incoherently."
            ),
            tool!(
                bench_assembly,
                Read,
                None,
                "Read assembly coordinates, surface normals, holes/offsets, mounts, BOM and clearance diagnostics from the saved bench or supplied draft. Missing hardware is unresolved. No writes."
            ),
            tool!(
                bench_catalog,
                Read,
                None,
                "Search representative lab optics and mounting assemblies. Includes model parameters, clamping metadata and approximate planning prices."
            ),
            tool!(
                bench_template,
                Mutate,
                None,
                "Preview a mounted lab bench template. Set apply=true with the current revision to save it as the project bench."
            ),
            tool!(
                bench_auto_aim,
                Mutate,
                None,
                "Solve omitted or null rotations and rotations with aim_rotation=true. Stage rotations are fixed; source directions are adjustable with aim_direction=true. Align two incident branches at each combiner using a shared reflected/transmitted output. Both separation measures must be below 1e-6 in mm and mrad. Residuals are relative to the first incoming branch. On conflict, return ok=false, the input ports, and the original bench. Optional hole snapping and uniform Z move elements outside stages. Set apply=true with the current revision to save a successful result."
            ),
            tool!(
                bench_lab_check,
                Read,
                None,
                "Read the delay stages (position_mm, optional pzt_um), named branch paths (opl_mm, stage_ids, SVG highlight color), assertions and lab diagnostics for one project revision. Checks optical terminations, Gaussian aperture clearance, mount collisions and hole access. Evaluate equal_opl as the first named branch minus the second in mm or um. Evaluate colinear at a common splitter, returning spot separation in mm and output angular separation in mrad. The assertion unit selects which measure is compared with target and tolerance. Recommends dump positions for escaped beams. Set require_pass=true to fail the command when the check reports errors."
            ),
            tool!(
                bench_field,
                Read,
                None,
                "Read the transverse complex field at the end of one named branch, or the superposed field of every named branch ending at one input port. Sources need field sampling (grid, pitch_um, profile, waist_um) and a polarization. Each arm launches that field, propagates it by the band-limited angular spectrum through its traced flights, applies clear apertures, region outlines, thin lens and curved mirror quadratic phases and element actions (mask, SLM, tip_tilt, rotating ground glass), and scales by the traced power and Jones vector, so ND filters, splitters and coatings act through the trace. Arms of one source, or of sources sharing a coherence group, interfere with a Gaussian coherence-length factor. Returns power, near-field centroid, far-field centroid in mrad, mean phase, speckle contrast averaged over ground-glass realizations, per-arm readings, pair coherence and correlation, sampling warnings (window edge power and band limit), and optional block-averaged intensity and phase arrays. Lens groups are not supported; boundaries are periodic."
            ),
            tool!(
                bench_alignment_guide,
                Read,
                None,
                "Read ordered installation steps, expected beam radii, and quarter-turn screw and spot-shift estimates."
            ),
            tool!(
                bench_export_blueprint,
                Read,
                None,
                "Return a lab blueprint or alignment SOP with its filename, MIME type and text content. SVG shows the project revision, stage readings, branch OPL, assertion results and highlighted stage-controlled arms."
            ),
            tool!(
                Meta, workspace_status,
                Read,
                None,
                "Read the workspace state before requesting optical data. A recovery state is connected but has no active optical system."
            ),
            tool!(
                Meta, workspace_retry,
                Mutate,
                Read,
                "While recovery is active, reread a user-repaired saved file under the current workspace lease. A missing or invalid file stays in recovery. Requires its current recovery revision."
            ),
            tool!(
                Meta, workspace_recover,
                Mutate,
                Write,
                "While recovery is active, validate a chosen native/Geopter/ZMX project or create a new system. Preserve the original file before installing the chosen system; reject changed disk contents and stale recovery revisions. No optical tool can write through recovery."
            ),
            tool!(
                system_get,
                Read,
                None,
                "Read the entire current optical prescription and revision"
            ),
            tool!(
                system_set,
                Mutate,
                None,
                "Atomically save the complete prescription; revision must match system_get"
            ),
            tool!(
                system_validate,
                Read,
                None,
                "Validate a complete prescription without changing the workspace"
            ),
            tool!(
                system_resolved,
                Read,
                None,
                "Read the prescription with the active configuration's overrides applied: the exact system every analysis tool evaluates. Read-only; authoring fields stay on system_get"
            ),
            tool!(
                configuration_activate,
                Mutate,
                None,
                "Select the active multi-configuration by zero-based index and save atomically. All analysis tools then evaluate that configuration. Requires the current revision"
            ),
            tool!(
                surface_edit,
                Read,
                None,
                "Stage inserted or removed internal surface rows in a supplied draft without saving, solving or tracing. Zero-based before may equal the row count (before Image). Remaps stop and retained gap references; rejects deletion of gaps referenced by retained solves. Returns the transformed system, old_to_new indices, inserted indices and stop_reassigned."
            ),
            tool!(
                design_summary,
                Read,
                None,
                "High-density optical summary returning first-order metrics, polychromatic per-field RMS spot radii, distortion, axial color, and minimum center/edge thicknesses in one call."
            ),
            tool!(
                optimize,
                Mutate,
                None,
                "Run the native Damped Least Squares (Levenberg-Marquardt) sequential optical optimizer over chosen variables (radius, thickness, conic, asphere, model glass, tilt/decenter, configuration overrides) and merit function operands (RMS spot, wavefront, EFL, BFL, F/#, track, center/edge thickness, distortion, color, chief ray angle, ray intercept). Set apply=true with the current revision to commit the optimized system atomically; otherwise returns a dry-run preview."
            ),
            tool!(
                tolerance_sensitivity,
                Read,
                None,
                "Evaluate per-operand plus/minus tolerance sensitivities (radius, thickness, decenter, tilt, index, Abbe, element decenter/tilt) with optional autofocus compensation, sorted by worst offenders plus RSS estimate."
            ),
            tool!(
                tolerance_monte_carlo,
                Read,
                None,
                "Run parallel seeded Monte Carlo tolerance trials with optional autofocus compensator, returning distribution statistics (mean, std_dev, min/max, p50/p80/p90/p95/p98), pass yield rate, compensator stroke range, and histogram."
            ),
            tool!(
                Meta, history_list,
                Read,
                None,
                "List the in-memory revision history snapshots of the current workspace, including undo/redo availability."
            ),
            tool!(
                Meta, history_undo,
                Mutate,
                None,
                "Undo the latest workspace mutation by restoring the previous prescription and project link. Requires the current revision."
            ),
            tool!(
                Meta, history_redo,
                Mutate,
                None,
                "Redo a previously undone workspace mutation atomically. Requires the current revision."
            ),
            tool!(
                Meta, history_restore,
                Mutate,
                None,
                "Restore a historical prescription and project link by its zero-based index from history_list. The restore is undoable and requires the current revision."
            ),
            tool!(
                Meta, history_compare,
                Read,
                None,
                "Compare two revision history snapshots (default: previous vs current), returning optical metric deltas (EFL, BFL, F/#, RMS spot) and per-surface prescription diffs."
            ),
            tool!(
                project_list,
                Read,
                Read,
                "List the active linked project file, saved projects in the workspace projects directory, and recent project paths."
            ),
            tool!(
                project_open,
                Mutate,
                Read,
                "Open a native Hissabeth JSON or ZMX project file from disk, validate it, link native JSON files for subsequent saves, and update recent projects. Requires the current revision."
            ),
            tool!(
                project_save_as,
                Mutate,
                Write,
                "Atomically save the current prescription as Hissabeth JSON and link future saves to it. Relative paths resolve inside the workspace projects directory. Use project_export for ZMX. Requires the current revision."
            ),
            tool!(
                materials_list,
                Read,
                None,
                "List qualified product_catalog keys for built-in and project glass catalogs"
            ),
            tool!(
                material_search,
                Read,
                None,
                "Search catalog metadata; wavelengths in metadata are micrometers"
            ),
            tool!(
                material_index,
                Read,
                None,
                "Compute refractive index at wavelength nm with environmental corrections"
            ),
            tool!(
                air_index,
                Read,
                None,
                "Compute absolute air index from wavelength nm, temperature C and pressure Pa"
            ),
            tool!(
                catalog_import,
                Mutate,
                None,
                "Parse an AGF catalog and replace that named project catalog, preserving all other catalogs"
            ),
            tool!(
                layout,
                Read,
                None,
                "Get global 3D ray paths for stored fields, including explicit trace rejection reasons"
            ),
            tool!(
                trace_inspect,
                Read,
                None,
                "Inspect a normalized pupil ray or a finite source XY / (dx/dz,dy/dz) launch with positive initial z direction; return partial paths and physical failures. Pupil rays use saved mapping and vignetting; object rays use the actual curved source without pupil aiming."
            ),
            tool!(
                paraxial_trace,
                Read,
                None,
                "Per-vertex linearized axial and stored-field chief ray tables in global x/y; signed reduced angles include centered reflections. No aperture clipping. The image row uses its vertex plane."
            ),
            tool!(
                field_curvature,
                Read,
                None,
                "Continuous radial field sweep of analytic sagittal/tangential chief-ray foci for every stored wavelength, including zero weights and duplicates. Default endpoint is +Y at the maximum stored field radius; optional field_end selects a 2D slice. Shifts reference the stored image vertex; retain per-branch failures."
            ),
            tool!(
                prescription_report,
                Read,
                None,
                "Assemble selected saved-prescription sections as structured tables and the same canonical text. Includes endpoints, first-order/cardinal quantities, actual media, signed gaps, profiles and gap relations. Per-cell errors preserve other data; no sections returns header only. Does not mutate the system."
            ),
            tool!(
                paraxial,
                Read,
                None,
                "Compute first-order matrix, focal lengths, image conjugate, magnification and pupils. Afocal systems retain defined quantities; absent finite focal lengths, image conjugate and working F-number are null."
            ),
            tool!(
                trace_ray,
                Read,
                None,
                "Trace x/y in pupil mm, normalized by the wavelength's paraxial entrance-pupil radius using the saved pupil mapping; stored field_index or arbitrary field/field_x"
            ),
            tool!(
                spot_diagram,
                Read,
                None,
                "Trace a grid or hexapolar pupil quadrature; report area-weighted centroid/RMS, geometric radius and rejected-ray counts"
            ),
            tool!(
                polychromatic_spot,
                Read,
                None,
                "Area- and spectrum-weighted spot diagram for positive-weight wavelengths, retaining lateral color and sampled transmission"
            ),
            tool!(
                geometric_mtf,
                Read,
                None,
                "Geometric MTF from a complex OTF sum over weighted real intercepts on one physical frequency grid; selected or weighted wavelengths"
            ),
            tool!(
                ray_fan,
                Read,
                None,
                "Field-local tangential/sagittal transverse ray aberrations relative to chief ray in mm"
            ),
            tool!(
                opd_fan,
                Read,
                None,
                "Field-local tangential/sagittal optical path difference relative to chief reference sphere, in waves"
            ),
            tool!(
                longitudinal_aberration,
                Read,
                None,
                "Longitudinal spherical aberration as axis intercept versus pupil radius"
            ),
            tool!(
                spherochromatism,
                Read,
                None,
                "Longitudinal spherical aberration for every stored wavelength"
            ),
            tool!(
                astigmatism,
                Read,
                None,
                "Differential sagittal/tangential focal positions about the chief ray"
            ),
            tool!(
                distortion,
                Read,
                None,
                "Actual chief image intercept relative to the paraxial chief intercept at the stored image plane"
            ),
            tool!(
                wavefront,
                Read,
                None,
                "Masked pupil wavefront map, RMS and peak-to-valley in waves"
            ),
            tool!(
                diffraction_psf,
                Read,
                None,
                "Unit-energy Fraunhofer diffraction intensity and sampled Strehl ratio; paraxial exit pupil mapping"
            ),
            tool!(
                diffraction_mtf,
                Read,
                None,
                "Field-local diffraction MTF from a complex OTF sum at common physical frequencies, with spectral/transmission weights and real-chief lateral color"
            ),
            tool!(
                encircled_energy,
                Read,
                None,
                "Geometric encircled energy about the transmitted-ray centroid"
            ),
            tool!(
                wavefront_zernike,
                Read,
                None,
                "Least-squares Noll Zernike decomposition of the traced pupil wavefront; unit-RMS disk normalization, coefficients in waves"
            ),
            tool!(
                huygens_psf,
                Read,
                None,
                "Unit-energy PSF from a direct Huygens-Fresnel sum over traced pupil samples at arbitrary image sampling; paraxial exit pupil mapping"
            ),
            tool!(
                huygens_mtf,
                Read,
                None,
                "Field-local diffraction MTF from the direct Huygens PSF with the same complex OTF composition as the FFT path"
            ),
            tool!(
                gaussian_beam,
                Read,
                None,
                "Paraxial Gaussian beam q-parameter propagation through the reduced system matrix: per-surface 1/e^2 radii plus image-plane waist, curvature, Rayleigh range and divergence. Input waist_z is the global z of the input waist (first vertex at 0). Unclipped, unaberrated, on-axis"
            ),
            tool!(
                fiber_coupling,
                Read,
                None,
                "Gaussian overlap coupling efficiency at the stored image plane from the propagated beam into a flat-phase fiber mode of given mode field diameter; also reports the optimum MFD and its efficiency. No offsets, tilt or aberration"
            ),
            tool!(
                chromatic_focus,
                Read,
                None,
                "Paraxial image focus shift across a wavelength interval"
            ),
            tool!(
                surface_sag,
                Read,
                None,
                "Evaluate radial sag and its first derivative, including even and odd terms"
            ),
            tool!(
                autofocus,
                Mutate,
                None,
                "Set final image distance to the finite or infinite conjugate paraxial focus"
            ),
            tool!(
                solve,
                Mutate,
                None,
                "Store a persistent gap solve and recalculate all constraints atomically. Fixed removes the relationship and retains thickness. Indices are zero-based; overall length sums the half-open gap interval [first,last)"
            ),
            tool!(
                solve_recalculate,
                Mutate,
                None,
                "Recalculate all saved gap constraints atomically and return their definitions and resolved thicknesses"
            ),
            tool!(
                aperture_report,
                Read,
                None,
                "Reference-wavelength drawing envelopes from chief and four field-local pupil rim rays, ignoring physical clipping; includes reached prefixes and geometric failures"
            ),
            tool!(
                vignetting_update,
                Mutate,
                None,
                "Compute four global-axis aperture boundaries per field, store directional vignetting factors, and resolve gap constraints atomically at the reference wavelength"
            ),
            tool!(
                project_new,
                Mutate,
                None,
                "Replace the saved project with a minimum blank prescription: one plane air stop, unclipped plane endpoints, infinity source, one on-axis field, d-line and a 10 mm entrance pupil. Clears prior project data. Requires the current revision; creation is atomic and resolves the new model before saving."
            ),
            tool!(
                project_import,
                Mutate,
                None,
                "Import native or Geopter project text and atomically replace current prescription"
            ),
            tool!(
                project_export,
                Read,
                None,
                "Export the current prescription as native or Geopter JSON text"
            ),
            tool!(
                Meta, capabilities,
                Read,
                None,
                "Describe all implemented tools and physical scope"
            ),
        ]
    });
    &TOOLS
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn registered_commands_have_unique_names_serializable_contracts_and_access_levels() {
        let mut names = HashSet::new();
        for spec in registry() {
            assert!(names.insert(spec.name), "Duplicate command {}", spec.name);
            assert!(std::ptr::eq(lookup(spec.name).unwrap(), spec));
            let schema: Value =
                serde_json::from_slice(&serde_json::to_vec(&spec.params_schema).unwrap()).unwrap();
            let result = (spec.result_schema)();
            assert!(result.is_object(), "{}", spec.name);
            assert!(serde_json::to_vec(&result).is_ok());
            assert_eq!(schema["type"], "object", "{}", spec.name);
            // Every existing mutation participates in the workspace revision protocol.
            assert_eq!(
                spec.access == Access::Mutate,
                schema["properties"].get("revision").is_some(),
                "{}",
                spec.name
            );
            let files = match spec.name {
                "batch" => FileAccess::Steps,
                "project_list" | "project_open" | "workspace_retry" => FileAccess::Read,
                "project_save_as" | "workspace_recover" => FileAccess::Write,
                _ => FileAccess::None,
            };
            assert_eq!(spec.files, files, "{}", spec.name);
        }
    }

    #[test]
    fn registry_preserves_mcp_argument_errors() {
        let dir = tempfile::tempdir().unwrap();
        let mut workspace = Workspace::open(dir.path().join("system.json")).unwrap();
        for (name, args, error) in [
            ("missing_tool", json!({}), "Unknown tool missing_tool"),
            (
                "air_index",
                json!({"wavelength":500,"extra":1}),
                "Unknown argument arguments.extra",
            ),
            (
                "air_index",
                json!({"wavelength":"500"}),
                "Invalid type at arguments.wavelength",
            ),
            (
                "air_index",
                json!({"wavelength":99}),
                "Value outside range at arguments.wavelength",
            ),
            (
                "bench_template",
                json!({"apply":true,"name":"michelson"}),
                "Missing arguments.revision",
            ),
        ] {
            assert_eq!(validate_args(name, &args).unwrap_err(), error);
            let response = rpc(
                json!({"id":1, "method":"tools/call","params":{"name":name,"arguments":args}}),
                &mut workspace,
            )
            .unwrap();
            assert_eq!(response["result"]["isError"], true);
            assert_eq!(response["result"]["content"][0]["text"], error);
        }
    }
}
