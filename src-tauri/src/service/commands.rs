use super::*;
use super::{params as p, registry::Target};
use serde::Serialize;

#[derive(Serialize, schemars::JsonSchema)]
pub(super) struct Revisioned<T> {
    revision: u64,
    #[serde(flatten)]
    data: T,
}

#[derive(Serialize, schemars::JsonSchema)]
pub(super) struct BenchReport<T> {
    saved: bool,
    #[serde(flatten)]
    snapshot: Revisioned<T>,
}

#[derive(Serialize, schemars::JsonSchema)]
pub(super) struct Preview<T> {
    applied: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    revision: Option<u64>,
    #[serde(flatten)]
    data: T,
}

pub(super) fn batch(target: Target<'_>, args: batch::Request) -> Result<batch::Report, String> {
    target.write(|store| store.engine()?.run_batch(args))
}

fn analysis<T>(
    target: &Target<'_>,
    run: impl FnOnce(&System) -> Result<T, String>,
) -> Result<Revisioned<T>, String> {
    target.system(|s, revision| {
        Ok(Revisioned {
            revision,
            data: run(s)?,
        })
    })
}

fn selected<'a>(
    s: &'a System,
    draft: &'a Option<bench::Bench>,
) -> Result<&'a bench::Bench, String> {
    draft
        .as_ref()
        .or(s.bench.as_ref())
        .ok_or_else(|| "Create a free-space bench first".into())
}

fn bench_report<T>(
    target: &Target<'_>,
    draft: Option<bench::Bench>,
    run: impl FnOnce(&bench::Bench, &System, u64) -> Result<T, String>,
) -> Result<BenchReport<T>, String> {
    target.system(|system, revision| {
        Ok(BenchReport {
            saved: draft.is_none(),
            snapshot: Revisioned {
                revision,
                data: run(selected(system, &draft)?, system, revision)?,
            },
        })
    })
}

#[derive(Serialize, schemars::JsonSchema)]
pub(super) struct BenchData<T> {
    bench: T,
}

#[derive(Serialize, schemars::JsonSchema)]
pub(super) struct SystemData {
    system: System,
}

#[derive(Serialize, schemars::JsonSchema)]
pub(super) struct ValidSystem {
    valid: bool,
    system: System,
}

#[derive(Serialize, schemars::JsonSchema)]
pub(super) struct MaterialIndex {
    index: f64,
    wavelength: f64,
    temperature: f64,
    pressure: f64,
}

#[derive(Serialize, schemars::JsonSchema)]
pub(super) struct Index {
    index: f64,
}

#[derive(Serialize, schemars::JsonSchema)]
pub(super) struct Sag {
    sag: f64,
    derivative: f64,
    height: f64,
    units: &'static str,
}

#[derive(Serialize, schemars::JsonSchema)]
pub(super) struct Export {
    text: String,
    name: String,
    revision: u64,
    format: p::ProjectFormat,
}

#[derive(Serialize, schemars::JsonSchema)]
pub(super) struct Solved {
    revision: u64,
    solve: solve::State,
}
#[derive(Serialize, schemars::JsonSchema)]
pub(super) struct Solves {
    revision: u64,
    solves: Vec<solve::State>,
}
#[derive(Serialize, schemars::JsonSchema)]
pub(super) struct Capabilities {
    tools: Vec<registry::ToolDescription>,
    units: Units,
    backend: &'static str,
    scope: Scope,
}
#[derive(Serialize, schemars::JsonSchema)]
struct ZernikeWavefrontFitScope {
    ordering: &'static str,
    max_terms: usize,
}

#[derive(Serialize, schemars::JsonSchema)]
struct OptimizationScope {
    solver: &'static str,
    max_variables: usize,
    max_operands: usize,
}

#[derive(Serialize, schemars::JsonSchema)]
struct TolerancingScope {
    sensitivity: bool,
    monte_carlo: bool,
    max_trials: usize,
}

#[derive(Serialize, schemars::JsonSchema)]
struct MultiConfigurationScope {
    max_configurations: usize,
    operands: &'static [&'static str],
}

#[derive(Serialize, schemars::JsonSchema)]
struct Scope {
    sequential: bool,
    finite_conjugates: bool,
    curved_object_surface: bool,
    curved_image_surface: bool,
    endpoint_apertures: bool,
    object_medium: bool,
    prescribed_plot_colors: bool,
    internal_stop_aiming: bool,
    centered_surfaces: bool,
    centered_reflection: bool,
    ray_inspection: bool,
    paraxial_ray_tables: bool,
    assembled_prescription_report: bool,
    startup_recovery: bool,
    surface_row_editing: bool,
    project_creation: bool,
    automatic_semidiameters: bool,
    directional_vignetting: bool,
    field_local_analyses: bool,
    continuous_field_curvature: bool,
    pupil_sampling: &'static [&'static str],
    pupil_mappings: &'static [&'static str],
    weighted_polychromatic_mtf: bool,
    analysis_revisions: bool,
    project_exchange: &'static [&'static str],
    folded_coordinates: bool,
    polarization: bool,
    nonsequential: bool,
    free_space_bench: bool,
    bench_spatial_poses: bool,
    bench_scalar_splitters: bool,
    bench_nearest_hits: bool,
    bench_assembly: bool,
    bench_lens_groups: bool,
    bench_fresnel_splitters: bool,
    bench_coatings: bool,
    bench_scatter: &'static [&'static str],
    bench_group_ghosts: usize,
    bench_polarization: bool,
    bench_gaussian_envelope: bool,
    bench_hardware_catalog: bool,
    bench_auto_aim: bool,
    bench_lab_check: bool,
    bench_field: &'static str,
    bench_readout: &'static str,
    bench_alignment_guide: bool,
    bench_blueprint_exports: &'static [&'static str],
    bench_detector_maps: bool,
    bench_lens_model: &'static str,
    surface_profiles: &'static [&'static str],
    huygens_diffraction: bool,
    zernike_wavefront_fit: ZernikeWavefrontFitScope,
    gaussian_beam: bool,
    fiber_coupling: bool,
    coordinate_breaks: bool,
    optimization: OptimizationScope,
    tolerancing: TolerancingScope,
    revision_history: bool,
    multi_project: bool,
    multi_configuration: MultiConfigurationScope,
}

#[derive(Serialize, schemars::JsonSchema)]
struct Units {
    length: &'static str,
    wavelength: &'static str,
    field_angle: &'static str,
    pressure: &'static str,
    temperature: &'static str,
}
#[derive(Serialize, schemars::JsonSchema)]
pub(super) struct ImportedCatalog {
    revision: u64,
    glasses_imported: usize,
    warnings: Vec<material::CatalogWarning>,
}
#[derive(Serialize, schemars::JsonSchema)]
struct GlassMatch {
    key: String,
    name: String,
    catalog: String,
    nd: f64,
    vd: f64,
    formula: u8,
    wavelength_range: Option<[f64; 2]>,
}
#[derive(Serialize, schemars::JsonSchema)]
pub(super) struct Materials {
    total: usize,
    glasses: Vec<GlassMatch>,
    warnings: Vec<&'static material::CatalogWarning>,
}
#[derive(Serialize, schemars::JsonSchema)]
pub(super) struct Spherochromatism {
    curves: Vec<ChromaticCurve>,
    reference_focus: f64,
    reference_wavelength: f64,
    units: &'static str,
}
#[derive(Serialize, schemars::JsonSchema)]
struct ChromaticCurve {
    #[serde(flatten)]
    curve: analysis::LongitudinalAberration,
    color: [f64; 4],
    weight: f64,
    wavelength_index: usize,
}

fn save_bench(
    e: &mut Engine,
    scene: &bench::Bench,
    revision: u64,
    action: &str,
) -> Result<u64, String> {
    scene.validate_groups(&e.system)?;
    let mut next = e.system.clone();
    next.bench = Some(scene.clone());
    e.commit_with_action(next, revision, action)?;
    Ok(e.revision)
}

pub(super) fn bench_get(
    target: &Target<'_>,
    _: p::Empty,
) -> Result<Revisioned<BenchData<Option<bench::Bench>>>, String> {
    target.read(|e| {
        Ok(Revisioned {
            revision: e.revision,
            data: BenchData {
                bench: e.system.bench.clone(),
            },
        })
    })
}

pub(super) fn bench_example(
    target: &Target<'_>,
    _: p::Empty,
) -> Result<BenchData<bench::Bench>, String> {
    target.read(|_| {
        Ok(BenchData {
            bench: bench::example(),
        })
    })
}

#[derive(Serialize, schemars::JsonSchema)]
pub(super) struct BenchSetResult {
    revision: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    trace: Option<bench::Trace>,
}

pub(super) fn bench_set(target: Target<'_>, args: p::BenchSet) -> Result<BenchSetResult, String> {
    target.edit(args.revision, |e| {
        let mut bench = args.bench;
        bench.resolve_stages()?;
        let trace = if bench.stages.is_empty() {
            bench.validate()?;
            None
        } else {
            Some(bench.trace(&e.system)?)
        };
        let mut next = e.system.clone();
        next.bench = Some(bench);
        let revision = e.commit_with_action(next, args.revision, "bench_set")?.revision;
        Ok(BenchSetResult { revision, trace })
    })
}

pub(super) fn bench_trace(
    target: &Target<'_>,
    args: p::BenchDraft,
) -> Result<BenchReport<bench::Trace>, String> {
    bench_report(target, args.bench, |b, s, _| b.trace(s))
}

pub(super) fn bench_assembly(
    target: &Target<'_>,
    args: p::BenchDraft,
) -> Result<BenchReport<bench::Assembly>, String> {
    bench_report(target, args.bench, |b, _, _| {
        let b = b.resolved_stages()?;
        b.validate()?;
        Ok(b.assembly_report())
    })
}

pub(super) fn bench_lab_check(
    target: &Target<'_>,
    args: p::BenchCheck,
) -> Result<BenchReport<bench_lab::LabCheck>, String> {
    bench_report(target, args.bench, |bench, system, _| {
        let report = bench_lab::check(bench, system)?;
        if args.require_pass {
            report.require_pass()?;
        }
        Ok(report)
    })
}

pub(super) fn bench_field(
    target: &Target<'_>,
    args: p::BenchField,
) -> Result<BenchReport<bench_lab::field::FieldReading>, String> {
    let request = bench_lab::field::FieldRequest {
        branch: args.branch,
        port: args.port,
        wavelength: args.wavelength,
        realizations: args.realizations,
        arrays: args.arrays,
    };
    bench_report(target, args.bench, |b, s, _| bench_lab::field::field(b, s, &request))
}

pub(super) fn bench_readout(
    target: &Target<'_>,
    args: p::BenchReadout,
) -> Result<BenchReport<bench_lab::field::readout::ReadoutReading>, String> {
    bench_report(target, args.bench, |b, s, _| {
        bench_lab::field::readout::readout(b, s, &args.detector, args.realizations)
    })
}

pub(super) fn bench_alignment_guide(
    target: &Target<'_>,
    args: p::BenchDraft,
) -> Result<BenchReport<bench_lab::Guide>, String> {
    bench_report(target, args.bench, |b, s, _| bench_lab::alignment(b, s))
}

pub(super) fn bench_export_blueprint(
    target: &Target<'_>,
    args: p::BenchExportBlueprint,
) -> Result<BenchReport<bench_lab::Blueprint>, String> {
    let saved = args.bench.is_none();
    bench_report(target, args.bench, |b, s, revision| {
        bench_lab::export_blueprint_report(
            b, s, &args.format, Some(bench_lab::BlueprintRevision { revision, saved }),
        )
    })
}

pub(super) fn artifact_start(
    target: &Target<'_>,
    args: p::ArtifactStart,
) -> Result<artifacts::Started, String> {
    let (_, store) = target.artifacts("artifact_start")?;
    let draft = args.params.bench;
    let saved = draft.is_none();
    let (bench, system, revision) =
        target.system(|s, revision| Ok((selected(s, &draft)?.clone(), s.clone(), revision)))?;
    let format = args.format.clone();
    artifacts::start(&store, &args.kind, &args.format, revision, saved, move || {
        let drawing = bench_lab::export_blueprint_report(
            &bench, &system, &format, Some(bench_lab::BlueprintRevision { revision, saved }),
        )?;
        let (filename, mime, content) = drawing.into_file();
        Ok(artifacts::Output { filename, mime: mime.into(), bytes: content.into_bytes() })
    })
}

pub(super) fn artifact_status(target: &Target<'_>, args: p::ArtifactId) -> Result<artifacts::Status, String> {
    let (_, store) = target.artifacts("artifact_status")?;
    store.lock().map_err(|e| e.to_string())?.status(&args.id)
}

pub(super) fn artifact_fetch(target: &Target<'_>, args: p::ArtifactId) -> Result<artifacts::Fetched, String> {
    let (_, store) = target.artifacts("artifact_fetch")?;
    store.lock().map_err(|e| e.to_string())?.fetch(&args.id)
}

pub(super) fn artifact_export(target: &Target<'_>, args: p::ArtifactExport) -> Result<artifacts::Exported, String> {
    let (workspace_file, store) = target.artifacts("artifact_export")?;
    if args.path.trim().is_empty() {
        return Err("Export path is required".into());
    }
    if Path::new(&args.path).extension().is_none() {
        return Err(format!("Export path {:?} needs a file name with its extension, such as drawing.svg", args.path));
    }
    let path = ProjectFiles::locate(&workspace_file, &args.path, true)?;
    if ProjectFiles::reserved(&workspace_file, &path) {
        return Err("Export path is reserved for workspace storage".into());
    }
    // Never replace the project itself, whatever extension it was saved under.
    let linked = target
        .read(|e| Ok(e.projects.linked_path.clone()))
        .unwrap_or_else(|_| ProjectFiles::load(&workspace_file).linked_path);
    let normal = |p: &Path| match (p.parent().and_then(|d| d.canonicalize().ok()), p.file_name()) {
        (Some(dir), Some(name)) => dir.join(name),
        _ => p.to_path_buf(),
    };
    // Compare by file identity too: a different spelling (case on a
    // case-insensitive volume, a link) may still name the project.
    let same = |project: &PathBuf| {
        normal(project) == normal(&path) || same_file::is_same_file(project, &path).unwrap_or(false)
    };
    if std::iter::once(&workspace_file).chain(&linked).any(same) {
        return Err(format!(
            "Cannot export over {}: it is the open project; choose another file name",
            normal(&path).display()
        ));
    }
    if !path.parent().is_some_and(Path::is_dir) {
        return Err(format!("Cannot write {}: its parent directory does not exist", path.display()));
    }
    store.lock().map_err(|e| e.to_string())?.export(&args.id, &path)
}

pub(super) fn artifact_cancel(target: &Target<'_>, args: p::ArtifactId) -> Result<artifacts::Status, String> {
    let (_, store) = target.artifacts("artifact_cancel")?;
    store.lock().map_err(|e| e.to_string())?.cancel(&args.id)
}

pub(super) fn bench_catalog(
    target: &Target<'_>,
    args: p::BenchCatalog,
) -> Result<bench_lab::Catalog, String> {
    target.standalone()?;
    Ok(bench_lab::catalog_report(
        args.query.as_deref(),
        args.kind.as_deref(),
    ))
}

pub(super) fn bench_template(
    target: Target<'_>,
    args: p::BenchTemplate,
) -> Result<Preview<BenchData<bench::Bench>>, String> {
    target.standalone()?;
    let data = BenchData {
        bench: bench_lab::template(&args.name)?,
    };
    let revision = if args.apply == Some(true) {
        let revision = args.revision.ok_or("Missing arguments.revision")?;
        Some(target.edit(revision, |e| {
            save_bench(e, &data.bench, revision, "bench_template")
        })?)
    } else {
        None
    };
    Ok(Preview {
        applied: revision.is_some(),
        revision,
        data,
    })
}

pub(super) fn bench_auto_aim(
    target: Target<'_>,
    args: p::BenchAutoAim,
) -> Result<Preview<bench_lab::AimResult>, String> {
    let aim = |scene: &bench::Bench, system: &System| {
        bench_lab::aim(scene, args.snap_holes.unwrap_or(false), args.uniform_z, system)
    };
    if args.apply == Some(true) {
        let revision = args.revision.ok_or("Missing arguments.revision")?;
        return target.edit(revision, |e| {
            let data = aim(selected(&e.system, &args.bench)?, &e.system)?;
            let revision = if data.ok {
                save_bench(e, &data.bench, revision, "bench_auto_aim")?
            } else {
                revision
            };
            Ok(Preview {
                applied: data.ok,
                revision: Some(revision),
                data,
            })
        });
    }
    if let Some(scene) = args.bench {
        target.standalone()?;
        let data = if scene.elements.iter().any(|e| e.kind == bench::Kind::LensGroup) {
            target.system(|system, _| aim(&scene, system))?
        } else {
            aim(&scene, &System::default())?
        };
        return Ok(Preview {
            applied: false,
            revision: None,
            data,
        });
    }
    target.system(|s, r| {
        Ok(Preview {
            applied: false,
            revision: Some(r),
            data: aim(selected(s, &None)?, s)?,
        })
    })
}

pub(super) fn workspace_status(
    target: &Target<'_>,
    _: p::Empty,
) -> Result<workspace::Status, String> {
    match target {
        Target::Shared(shared, _) => Ok(shared.read().map_err(|e| e.to_string())?.status_report()),
        Target::Workspace(w) => Ok(w.status_report()),
        Target::Engine(_) => Err("Unknown tool workspace_status".into()),
    }
}

pub(super) fn workspace_retry(
    target: Target<'_>,
    args: p::Revision,
) -> Result<workspace::Status, String> {
    target.write(|store| store.workspace("workspace_retry")?.retry(args.revision))
}

pub(super) fn workspace_recover(
    target: Target<'_>,
    args: p::WorkspaceRecover,
) -> Result<workspace::Recovered, String> {
    target.write(|store| {
        store
            .workspace("workspace_recover")?
            .recover(args.revision, args.source)
    })
}

pub(super) fn system_get(
    target: &Target<'_>,
    _: p::Empty,
) -> Result<Revisioned<SystemData>, String> {
    target.read(|e| {
        Ok(Revisioned {
            revision: e.revision,
            data: SystemData {
                system: e.system.clone(),
            },
        })
    })
}

pub(super) fn system_set(target: Target<'_>, args: p::SystemSet) -> Result<Revision, String> {
    target.edit(args.revision, |e| {
        e.replace(args.system, args.revision, "system_set")
    })
}

pub(super) fn system_validate(
    target: &Target<'_>,
    args: p::SystemDraft,
) -> Result<ValidSystem, String> {
    target.read(|_| Ok(()))?;
    let mut system = args.system;
    system.update()?;
    Ok(ValidSystem {
        valid: true,
        system,
    })
}

pub(super) fn system_resolved(
    target: &Target<'_>,
    _: p::Empty,
) -> Result<Revisioned<SystemData>, String> {
    analysis(target, |s| {
        Ok(SystemData {
            system: s.resolved()?,
        })
    })
}

pub(super) fn configuration_activate(
    target: Target<'_>,
    args: p::SelectIndex,
) -> Result<Revision, String> {
    target.edit(args.revision, |e| {
        if args.index >= e.system.configurations.len().max(1) {
            return Err("Configuration index outside the configuration table".into());
        }
        let mut next = e.system.clone();
        next.active_configuration = args.index;
        e.replace(next, args.revision, "configuration_activate")
    })
}

pub(super) fn surface_edit(
    target: &Target<'_>,
    args: p::SurfaceEdit,
) -> Result<editing::Edit, String> {
    target.read(|_| Ok(()))?;
    if args
        .system
        .configurations
        .iter()
        .any(|c| !c.overrides.is_empty())
    {
        return Err("Remove configuration overrides before inserting or removing surfaces".into());
    }
    editing::apply(&args.system, args.operation)
}

pub(super) fn optimize(
    target: Target<'_>,
    args: p::Optimize,
) -> Result<Preview<optimize::OptimizationResult>, String> {
    let apply = args.apply.unwrap_or(false);
    let (data, revision) = target.system(|s, revision| {
        if apply {
            check_revision(revision, args.revision.ok_or("Missing arguments.revision")?)?;
        }
        Ok((optimize::run(s, &args.request)?, revision))
    })?;
    let revision = if apply {
        target
            .edit(revision, |e| {
                e.commit_with_action(data.system.clone(), revision, "optimize")
            })?
            .revision
    } else {
        revision
    };
    Ok(Preview {
        applied: apply,
        revision: Some(revision),
        data,
    })
}

pub(super) fn tolerance_sensitivity(
    target: &Target<'_>,
    args: tolerance::SensitivityRequest,
) -> Result<Revisioned<tolerance::SensitivityResult>, String> {
    analysis(target, |s| tolerance::sensitivity(s, &args))
}

pub(super) fn tolerance_monte_carlo(
    target: &Target<'_>,
    args: tolerance::MonteCarloRequest,
) -> Result<Revisioned<tolerance::MonteCarloResult>, String> {
    analysis(target, |s| tolerance::monte_carlo(s, &args))
}

pub(super) fn history_list(
    target: &Target<'_>,
    _: p::Empty,
) -> Result<history::HistoryList, String> {
    target.read(|e| Ok(e.history.list(e.revision)))
}

pub(super) fn history_compare(
    target: &Target<'_>,
    args: p::HistoryCompare,
) -> Result<history::Comparison, String> {
    let (from, to, i, j) = target.read(|e| {
        let (from, to, i, j) = e.history.pair(args.from_index, args.to_index)?;
        Ok((from.clone(), to.clone(), i, j))
    })?;
    Ok(history::compare(&from, &to, i, j))
}

pub(super) fn project_list(
    target: &Target<'_>,
    _: p::Empty,
) -> Result<workspace::ProjectList, String> {
    target.read(|e| Ok(e.projects.list(&e.path)))
}

pub(super) fn history_undo(target: Target<'_>, args: p::Revision) -> Result<HistoryStep, String> {
    target.edit(args.revision, |e| e.step_history(args.revision, true))
}

pub(super) fn history_redo(target: Target<'_>, args: p::Revision) -> Result<HistoryStep, String> {
    target.edit(args.revision, |e| e.step_history(args.revision, false))
}

pub(super) fn project_open(
    target: Target<'_>,
    args: p::ProjectPath,
) -> Result<OpenProject, String> {
    target.edit(args.revision, |e| e.project_open(&args.path, args.revision))
}

pub(super) fn project_save_as(
    target: Target<'_>,
    args: p::ProjectPath,
) -> Result<SavedProject, String> {
    target.edit(args.revision, |e| {
        e.project_save_as(&args.path, args.revision)
    })
}

pub(super) fn history_restore(
    target: Target<'_>,
    args: p::SelectIndex,
) -> Result<Revision, String> {
    target.edit(args.revision, |e| {
        let entry = e.history.entry(args.index)?;
        let mut projects = e.projects.clone();
        projects.linked_path = entry.linked_path.clone();
        e.commit_project(entry.system.clone(), args.revision, "restore", projects)
    })
}

pub(super) fn materials_list(target: &Target<'_>, _: p::Empty) -> Result<Vec<String>, String> {
    target.read(|e| {
        let system = &e.system;
        let mut names: Vec<String> = material::catalogs()
            .iter()
            .chain(&system.custom_glasses)
            .map(|g| g.key())
            .collect();
        names.extend(["AIR".into(), "FUSED_SILICA".into()]);
        names.sort();
        names.dedup();
        Ok(names)
    })
}

pub(super) fn material_search(
    target: &Target<'_>,
    args: p::MaterialSearch,
) -> Result<Materials, String> {
    target.system(|system, _| {
        let query = args.query.as_deref().unwrap_or("").to_uppercase();
        let catalog = args.catalog.as_deref().unwrap_or("").to_uppercase();
        let offset = args.offset.unwrap_or(0);
        let limit = args.limit.unwrap_or(50);
        let matches: Vec<_> = material::catalogs()
            .iter()
            .chain(&system.custom_glasses)
            .filter(|g| {
                (catalog.is_empty() || g.catalog == catalog)
                    && (g.name.contains(&query) || g.catalog.contains(&query))
            })
            .collect();
        let glasses = matches
            .iter()
            .skip(offset)
            .take(limit)
            .map(|g| GlassMatch {
                key: g.key(),
                name: g.name.clone(),
                catalog: g.catalog.clone(),
                nd: g.nd,
                vd: g.vd,
                formula: g.formula,
                wavelength_range: g.wavelength_range,
            })
            .collect();
        let warnings = material::catalog_warnings()
            .iter()
            .filter(|w| catalog.is_empty() || w.catalog == catalog)
            .collect();
        Ok(Materials {
            total: matches.len(),
            glasses,
            warnings,
        })
    })
}

pub(super) fn material_index(
    target: &Target<'_>,
    args: p::MaterialIndex,
) -> Result<MaterialIndex, String> {
    target.system(|s, _| {
        let mut system = s.clone();
        let temperature = args.temperature.unwrap_or(system.temperature);
        let pressure = args.pressure.unwrap_or(system.pressure);
        system.temperature = temperature;
        system.pressure = pressure;
        Ok(MaterialIndex {
            index: system.material_index(&args.material, args.wavelength)?,
            wavelength: args.wavelength,
            temperature,
            pressure,
        })
    })
}

pub(super) fn air_index(target: &Target<'_>, args: p::AirIndex) -> Result<Index, String> {
    target.read(|e| {
        Ok(Index {
            index: material::air_index(
                args.wavelength / 1000.,
                args.temperature.unwrap_or(e.system.temperature),
                args.pressure.unwrap_or(e.system.pressure),
            )?,
        })
    })
}

pub(super) fn catalog_import(
    target: Target<'_>,
    args: p::CatalogImport,
) -> Result<ImportedCatalog, String> {
    target.edit(args.revision, |e| {
        let mut next = e.system.clone();
        let catalog = &args.catalog;
        let glasses = material::parse_agf(catalog, &args.text)?;
        let count = glasses.glasses.len();
        next.custom_glasses
            .retain(|g| g.catalog != catalog.to_uppercase());
        next.custom_glasses.extend(glasses.glasses);
        let revision = e.replace(next, args.revision, "catalog_import")?.revision;
        Ok(ImportedCatalog {
            revision,
            glasses_imported: count,
            warnings: glasses.warnings,
        })
    })
}

pub(super) fn autofocus(target: Target<'_>, args: p::Autofocus) -> Result<Revision, String> {
    target.edit(args.revision, |e| {
        let resolved = e.system.resolved()?;
        let w = args.wavelength.unwrap_or(e.system.reference());
        let s = solve::autofocus(&resolved, w)?;
        let mut next = e.system.clone();
        for (i, surface) in s.surfaces.iter().enumerate() {
            if surface.thickness == resolved.surfaces[i].thickness {
                continue;
            }
            let covered = next
                .configurations
                .get_mut(next.active_configuration)
                .and_then(|c| {
                    c.overrides.iter_mut().find_map(|o| match o {
                        crate::optics::Override::Thickness { surface: k, value } if *k == i => {
                            Some(value)
                        }
                        _ => None,
                    })
                });
            match covered {
                Some(value) => *value = surface.thickness,
                None => next.surfaces[i].thickness = surface.thickness,
            }
        }
        e.commit_with_action(next, args.revision, "autofocus")
    })
}

pub(super) fn solve(target: Target<'_>, args: p::Solve) -> Result<Solved, String> {
    target.edit(args.revision, |e| {
        let (next, solve) = solve::apply_solve(&e.system, args.gap, args.definition)?;
        let revision = e.commit_with_action(next, args.revision, "solve")?.revision;
        Ok(Solved { revision, solve })
    })
}

pub(super) fn solve_recalculate(target: Target<'_>, args: p::Revision) -> Result<Solves, String> {
    target.edit(args.revision, |e| {
        let revision = e
            .replace(e.system.clone(), args.revision, "solve_recalculate")?
            .revision;
        Ok(Solves {
            revision,
            solves: solve::state_reports(&e.system),
        })
    })
}

pub(super) fn vignetting_update(
    target: Target<'_>,
    args: p::Revision,
) -> Result<Revisioned<aperture::VignettingReport>, String> {
    target.edit(args.revision, |e| {
        let (solved, data) = aperture::update_vignetting(&e.system.resolved()?)?;
        let mut next = e.system.clone();
        next.fields = solved.fields;
        let revision = e
            .commit_with_action(next, args.revision, "vignetting_update")?
            .revision;
        Ok(Revisioned { revision, data })
    })
}

pub(super) fn project_new(target: Target<'_>, args: p::Revision) -> Result<Revision, String> {
    target.edit(args.revision, |e| {
        let mut next = project::blank();
        next.update()?;
        let mut projects = e.projects.clone();
        projects.linked_path = None;
        e.commit_project(next, args.revision, "project_new", projects)
    })
}

pub(super) fn project_import(
    target: Target<'_>,
    args: p::ProjectImport,
) -> Result<Revision, String> {
    target.edit(args.revision, |e| {
        let next = match args.format {
            p::ProjectFormat::Geopter => project::import(&args.text)?,
            p::ProjectFormat::Zmx => zmx::import(&args.text)?,
            p::ProjectFormat::Hissabeth => {
                serde_json::from_str(&args.text).map_err(|e| e.to_string())?
            }
        };
        e.replace(next, args.revision, "project_import")
    })
}

pub(super) fn project_export(
    target: &Target<'_>,
    args: p::ProjectExport,
) -> Result<Export, String> {
    target.system(|s, revision| {
        let text = match args.format {
            p::ProjectFormat::Geopter => serde_json::to_string_pretty(&project::export(s)?),
            p::ProjectFormat::Hissabeth => {
                serde_json::to_string_pretty(&serde_json::to_value(s).map_err(|e| e.to_string())?)
            }
            p::ProjectFormat::Zmx => Ok(zmx::export(s)?),
        }
        .map_err(|e| e.to_string())?;
        Ok(Export {
            revision,
            name: s.name.clone(),
            format: args.format,
            text,
        })
    })
}

pub(super) fn design_summary(
    target: &Target<'_>,
    _: p::Empty,
) -> Result<Revisioned<optimize::DesignSummary>, String> {
    analysis(target, |s| optimize::design_summary_report(&s.resolved()?))
}

pub(super) fn layout(
    target: &Target<'_>,
    _: p::Empty,
) -> Result<Revisioned<optics::Layout>, String> {
    analysis(target, |s| Ok(s.resolved()?.layout_report()))
}

pub(super) fn paraxial(
    target: &Target<'_>,
    args: p::FieldWavelength,
) -> Result<Revisioned<optics::Paraxial>, String> {
    analysis(target, |s| {
        s.resolved()?
            .paraxial_report(args.wavelength.unwrap_or(s.reference()))
    })
}

pub(super) fn paraxial_trace(
    target: &Target<'_>,
    args: p::ParaxialTrace,
) -> Result<Revisioned<inspection::ParaxialTrace>, String> {
    analysis(target, |s| {
        inspection::paraxial_trace_report(
            &s.resolved()?,
            &select_field(s, Some(args.field_index), None, None)?,
            args.wavelength,
        )
    })
}

pub(super) fn trace_inspect(
    target: &Target<'_>,
    args: p::TraceInspect,
) -> Result<Revisioned<inspection::Inspection>, String> {
    analysis(target, |s| {
        inspection::inspect_report(&s.resolved()?, args.launch, args.wavelength)
    })
}

pub(super) fn trace_ray(
    target: &Target<'_>,
    args: p::TraceRay,
) -> Result<Revisioned<optics::Ray>, String> {
    analysis(target, |s| {
        s.resolved()?.trace_field(
            args.x.unwrap_or(0.),
            args.y.unwrap_or(0.),
            &args.coordinates.select(s)?,
            args.wavelength.unwrap_or(s.reference()),
        )
    })
}

pub(super) fn spot_diagram(
    target: &Target<'_>,
    args: p::SpotDiagram,
) -> Result<Revisioned<optics::sampling::Spot>, String> {
    analysis(target, |s| {
        s.resolved()?
            .sample_pupil(
                &args.coordinates.select(s)?,
                args.wavelength.unwrap_or(s.reference()),
                args.sampling,
            )?
            .spot()
    })
}

pub(super) fn polychromatic_spot(
    target: &Target<'_>,
    args: p::PolychromaticSpot,
) -> Result<Revisioned<analysis::PolychromaticSpot>, String> {
    analysis(target, |s| {
        analysis::polychromatic_spot_report(
            &s.resolved()?,
            &args.coordinates.select(s)?,
            args.sampling,
        )
    })
}

pub(super) fn geometric_mtf(
    target: &Target<'_>,
    args: p::GeometricMtf,
) -> Result<Revisioned<crate::compute::GeometricMtf>, String> {
    analysis(target, |s| {
        let resolved = s.resolved()?;
        crate::compute::geometric_mtf_field(
            &resolved,
            &args.coordinates.select(&resolved)?,
            args.sampling,
            args.spectrum,
            args.axis.as_deref().unwrap_or("y"),
            args.max_frequency.unwrap_or(100.),
        )
    })
}

pub(super) fn ray_fan(
    target: &Target<'_>,
    args: p::Fan,
) -> Result<Revisioned<analysis::RayFan>, String> {
    analysis(target, |s| {
        analysis::ray_fan_report(
            &s.resolved()?,
            &args.coordinates.select(s)?,
            args.wavelength.unwrap_or(s.reference()),
            args.samples.unwrap_or(33),
        )
    })
}

pub(super) fn opd_fan(
    target: &Target<'_>,
    args: p::Fan,
) -> Result<Revisioned<analysis::OpdFan>, String> {
    analysis(target, |s| {
        analysis::opd_fan_report(
            &s.resolved()?,
            &args.coordinates.select(s)?,
            args.wavelength.unwrap_or(s.reference()),
            args.samples.unwrap_or(33),
        )
    })
}

pub(super) fn longitudinal_aberration(
    target: &Target<'_>,
    args: p::Fan,
) -> Result<Revisioned<analysis::LongitudinalAberration>, String> {
    analysis(target, |s| {
        analysis::longitudinal_report(
            &s.resolved()?,
            args.wavelength.unwrap_or(s.reference()),
            args.samples.unwrap_or(33),
        )
    })
}

pub(super) fn astigmatism(
    target: &Target<'_>,
    args: p::FieldWavelength,
) -> Result<Revisioned<analysis::Astigmatism>, String> {
    analysis(target, |s| {
        analysis::astigmatism_report(
            &s.resolved()?,
            &args.coordinates.select(s)?,
            args.wavelength.unwrap_or(s.reference()),
        )
    })
}

pub(super) fn distortion(
    target: &Target<'_>,
    args: p::FieldWavelength,
) -> Result<Revisioned<analysis::Distortion>, String> {
    analysis(target, |s| {
        analysis::distortion_report(
            &s.resolved()?,
            &args.coordinates.select(s)?,
            args.wavelength.unwrap_or(s.reference()),
        )
    })
}

pub(super) fn wavefront(
    target: &Target<'_>,
    args: p::Wavefront,
) -> Result<Revisioned<analysis::WavefrontMap>, String> {
    analysis(target, |s| {
        Ok(analysis::wavefront(
            &s.resolved()?,
            &args.coordinates.select(s)?,
            args.wavelength.unwrap_or(s.reference()),
            args.grid.unwrap_or(17),
        )?
        .map())
    })
}

pub(super) fn diffraction_psf(
    target: &Target<'_>,
    args: p::DiffractionPsf,
) -> Result<Revisioned<analysis::DiffractionPsf>, String> {
    analysis(target, |s| {
        analysis::diffraction_psf_report(
            &s.resolved()?,
            &args.coordinates.select(s)?,
            args.wavelength.unwrap_or(s.reference()),
            args.grid.unwrap_or(32),
        )
    })
}

pub(super) fn diffraction_mtf(
    target: &Target<'_>,
    args: p::DiffractionMtf,
) -> Result<Revisioned<analysis::DiffractionMtf>, String> {
    analysis(target, |s| {
        analysis::diffraction_mtf_report(
            &s.resolved()?,
            &args.coordinates.select(s)?,
            args.spectrum,
            args.grid.unwrap_or(32),
            args.max_frequency,
        )
    })
}

pub(super) fn wavefront_zernike(
    target: &Target<'_>,
    args: p::WavefrontZernike,
) -> Result<Revisioned<zernike::ZernikeFit>, String> {
    analysis(target, |s| {
        zernike::fit_report(
            &analysis::wavefront(
                &s.resolved()?,
                &args.coordinates.select(s)?,
                args.wavelength.unwrap_or(s.reference()),
                args.grid.unwrap_or(17),
            )?,
            args.terms.unwrap_or(37),
        )
    })
}

pub(super) fn huygens_psf(
    target: &Target<'_>,
    args: p::HuygensPsf,
) -> Result<Revisioned<analysis::HuygensPsf>, String> {
    analysis(target, |s| {
        analysis::huygens_psf_report(
            &s.resolved()?,
            &args.coordinates.select(s)?,
            args.wavelength.unwrap_or(s.reference()),
            args.grid.unwrap_or(32),
            args.size,
            args.pitch,
        )
    })
}

pub(super) fn huygens_mtf(
    target: &Target<'_>,
    args: p::HuygensMtf,
) -> Result<Revisioned<analysis::DiffractionMtf>, String> {
    analysis(target, |s| {
        analysis::huygens_mtf_report(
            &s.resolved()?,
            &args.coordinates.select(s)?,
            args.spectrum,
            args.grid.unwrap_or(32),
            args.max_frequency,
        )
    })
}

pub(super) fn gaussian_beam(
    target: &Target<'_>,
    args: p::GaussianBeam,
) -> Result<Revisioned<gaussian::GaussianBeam>, String> {
    analysis(target, |s| {
        gaussian::gaussian_beam_report(
            &s.resolved()?,
            args.wavelength.unwrap_or(s.reference()),
            args.waist,
            args.waist_z.unwrap_or(0.),
        )
    })
}

pub(super) fn fiber_coupling(
    target: &Target<'_>,
    args: p::FiberCoupling,
) -> Result<Revisioned<gaussian::FiberCoupling>, String> {
    analysis(target, |s| {
        gaussian::fiber_coupling_report(
            &s.resolved()?,
            args.wavelength.unwrap_or(s.reference()),
            args.waist,
            args.waist_z.unwrap_or(0.),
            args.mode_field_diameter,
        )
    })
}

pub(super) fn encircled_energy(
    target: &Target<'_>,
    args: p::EncircledEnergy,
) -> Result<Revisioned<analysis::EncircledEnergy>, String> {
    analysis(target, |s| {
        analysis::encircled_energy_report(
            &s.resolved()?,
            &args.coordinates.select(s)?,
            args.wavelength.unwrap_or(s.reference()),
            args.sampling,
            args.samples.unwrap_or(33),
        )
    })
}

pub(super) fn field_curvature(
    target: &Target<'_>,
    args: p::FieldCurvature,
) -> Result<Revisioned<focus::FieldCurvature>, String> {
    analysis(target, |s| {
        focus::field_curvature_report(&s.resolved()?, args.field_end, args.samples)
    })
}

pub(super) fn surface_sag(
    target: &Target<'_>,
    args: p::SurfaceSag,
) -> Result<Revisioned<Sag>, String> {
    analysis(target, |s| {
        let resolved = s.resolved()?;
        let surface = resolved
            .surfaces
            .get(args.surface)
            .ok_or("Surface index outside prescription")?;
        let (sag, derivative) = optics::sag(surface, args.height)?;
        Ok(Sag {
            sag,
            derivative,
            height: args.height,
            units: "mm",
        })
    })
}

pub(super) fn aperture_report(
    target: &Target<'_>,
    _: p::Empty,
) -> Result<Revisioned<aperture::ApertureReport>, String> {
    analysis(target, |s| aperture::semi_diameters(&s.resolved()?))
}

pub(super) fn spherochromatism(
    target: &Target<'_>,
    args: p::Fan,
) -> Result<Revisioned<Spherochromatism>, String> {
    analysis(target, |s| {
        let resolved = s.resolved()?;
        let reference_wavelength = resolved.reference();
        let reference_focus = resolved.paraxial(reference_wavelength)?["image_distance"]
            .as_f64()
            .ok_or("No finite reference focus")?;
        let curves: Vec<_> = resolved
            .wavelengths
            .iter()
            .enumerate()
            .map(|(index, wv)| {
                Ok(ChromaticCurve {
                    curve: analysis::longitudinal_report(
                        &resolved,
                        wv.value,
                        args.samples.unwrap_or(33),
                    )?,
                    color: wv.color,
                    weight: wv.weight,
                    wavelength_index: index,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Spherochromatism {
            curves,
            reference_focus,
            reference_wavelength,
            units: "mm",
        })
    })
}

pub(super) fn chromatic_focus(
    target: &Target<'_>,
    args: p::ChromaticFocus,
) -> Result<Revisioned<analysis::ChromaticFocus>, String> {
    analysis(target, |s| {
        let resolved = s.resolved()?;
        analysis::chromatic_focus_report(
            &resolved,
            args.lower.unwrap_or_else(|| {
                resolved
                    .wavelengths
                    .iter()
                    .map(|w| w.value)
                    .fold(f64::INFINITY, f64::min)
            }),
            args.upper.unwrap_or_else(|| {
                resolved
                    .wavelengths
                    .iter()
                    .map(|w| w.value)
                    .fold(f64::NEG_INFINITY, f64::max)
            }),
            args.samples.unwrap_or(33),
        )
    })
}

pub(super) fn prescription_report(
    target: &Target<'_>,
    args: p::PrescriptionReport,
) -> Result<report::PrescriptionReport, String> {
    target.system(|s, r| {
        report::generate(
            &s.resolved()?,
            r,
            args.sections,
            args.wavelength,
            args.precision,
        )
    })
}

pub(super) fn capabilities(target: &Target<'_>, _: p::Empty) -> Result<Capabilities, String> {
    target.read(|_| Ok(()))?;
    let scope = Scope {
        sequential: true,
        finite_conjugates: true,
        curved_object_surface: true,
        curved_image_surface: true,
        endpoint_apertures: true,
        object_medium: true,
        prescribed_plot_colors: true,
        internal_stop_aiming: true,
        centered_surfaces: true,
        centered_reflection: true,
        ray_inspection: true,
        paraxial_ray_tables: true,
        assembled_prescription_report: true,
        startup_recovery: true,
        surface_row_editing: true,
        project_creation: true,
        automatic_semidiameters: true,
        directional_vignetting: true,
        field_local_analyses: true,
        continuous_field_curvature: true,
        pupil_sampling: &["grid", "hexapolar"],
        pupil_mappings: &["reference_entrance_pupil", "real_stop"],
        weighted_polychromatic_mtf: true,
        analysis_revisions: true,
        project_exchange: &["hissabeth", "geopter", "zmx"],
        folded_coordinates: true,
        polarization: false,
        nonsequential: false,
        free_space_bench: true,
        bench_spatial_poses: true,
        bench_scalar_splitters: true,
        bench_nearest_hits: true,
        bench_assembly: true,
        bench_lens_groups: true,
        bench_fresnel_splitters: true,
        bench_coatings: true,
        bench_scatter: &["lambertian", "abg"],
        bench_group_ghosts: 2,
        bench_polarization: true,
        bench_gaussian_envelope: true,
        bench_hardware_catalog: true,
        bench_auto_aim: true,
        bench_lab_check: true,
        bench_field: "band-limited angular spectrum per traced arm with thin-element phases and spatial actions",
        bench_readout: "ROI modal projection (Hermite-Gauss, Laguerre-Gauss, custom) and balanced quadrature pairs",
        bench_alignment_guide: true,
        bench_blueprint_exports: bench_lab::FORMATS,
        bench_detector_maps: true,
        bench_lens_model: "ideal thin lens or real current-prescription lens group",
        surface_profiles: &[
            "standard",
            "even_asphere",
            "odd_asphere",
            "toroid",
            "zernike_fringe_sag",
        ],
        huygens_diffraction: true,
        zernike_wavefront_fit: ZernikeWavefrontFitScope {
            ordering: "noll",
            max_terms: 45,
        },
        gaussian_beam: true,
        fiber_coupling: true,
        coordinate_breaks: true,
        optimization: OptimizationScope {
            solver: "dls",
            max_variables: 64,
            max_operands: 256,
        },
        tolerancing: TolerancingScope {
            sensitivity: true,
            monte_carlo: true,
            max_trials: 2000,
        },
        revision_history: true,
        multi_project: true,
        multi_configuration: MultiConfigurationScope {
            max_configurations: 64,
            operands: &[
                "thickness",
                "radius",
                "conic",
                "aperture",
                "material",
                "object_distance",
            ],
        },
    };
    Ok(Capabilities {
        tools: registry::descriptions(),
        units: Units {
            length: "mm",
            wavelength: "nm",
            field_angle: "degrees",
            pressure: "Pa",
            temperature: "C",
        },
        backend: "cpu",
        scope,
    })
}
