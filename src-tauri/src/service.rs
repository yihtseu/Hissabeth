use crate::optics::{
    self, Field, PupilSampling, Spectrum, System, analysis, aperture, bench, bench_lab, editing,
    focus, gaussian, inspection, material, optimize, project, report, solve, tolerance, zernike,
    zmx,
};
use serde_json::{Value, json};
use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

mod broker;
mod history;
mod schema;
mod workspace;

pub use broker::{relay, rpc, rpc_shared, serve, serve_with_notifier};
pub use schema::{bench_schema, system_schema, tools, validate_args};
pub use workspace::Workspace;
use workspace::{ProjectFiles, persist_system};

pub type SharedWorkspace = Arc<RwLock<Workspace>>;
pub type ChangeNotifier = Arc<dyn Fn(u64) + Send + Sync>;

pub struct Engine {
    pub system: System,
    pub revision: u64,
    pub path: PathBuf,
    _lease: Arc<std::fs::File>,
    history: history::History,
    projects: ProjectFiles,
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
fn acquire_lease(path: &std::path::Path) -> Result<Arc<std::fs::File>, String> {
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
    Ok(Arc::new(lease))
}
fn fresh_revision() -> Result<u64, String> {
    let mut random = [0u8; 8];
    getrandom::fill(&mut random).map_err(|e| e.to_string())?;
    Ok(u64::from_le_bytes(random) & ((1u64 << 48) - 1))
}
pub trait ToolCaller {
    fn call_tool(&mut self, name: &str, args: Value) -> Result<Value, String>;
}
impl ToolCaller for Engine {
    fn call_tool(&mut self, name: &str, args: Value) -> Result<Value, String> {
        self.call(name, args)
    }
}
impl Engine {
    pub fn open(path: PathBuf) -> Result<Self, String> {
        let lease = acquire_lease(&path)?;
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => Some(bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.to_string()),
        };
        Self::load_bytes(path, lease, fresh_revision()?, bytes.as_deref())
    }
    fn load_bytes(
        path: PathBuf,
        lease: Arc<std::fs::File>,
        revision: u64,
        bytes: Option<&[u8]>,
    ) -> Result<Self, String> {
        let mut system: System = match bytes {
            Some(bytes) => serde_json::from_slice(bytes)
                .map_err(|e| format!("Invalid optical project: {e}"))?,
            None => System::default(),
        };
        let stored = serde_json::to_value(&system).map_err(|e| e.to_string())?;
        system.update()?;
        system.validate()?;
        let projects = ProjectFiles::load(&path);
        if let Some(bytes) = bytes {
            if stored != serde_json::to_value(&system).map_err(|e| e.to_string())? {
                if std::fs::read(&path).map_err(|e| e.to_string())? != bytes {
                    return Err("Saved file changed while loading; retry from disk".into());
                }
                persist_system(&path, &system, &projects, false)?;
            }
        }
        Ok(Self::new(path, lease, revision, system, projects))
    }

    fn new(
        path: PathBuf,
        lease: Arc<std::fs::File>,
        revision: u64,
        system: System,
        projects: ProjectFiles,
    ) -> Self {
        Self {
            history: history::History::initial(
                revision,
                system.clone(),
                projects.linked_path.clone(),
            ),
            system,
            revision,
            path,
            _lease: lease,
            projects,
        }
    }

    fn check_revision(&self, expected: u64) -> Result<(), String> {
        check_revision(self.revision, expected)
    }

    fn replace(&mut self, mut next: System, expected: u64, action: &str) -> Result<Value, String> {
        self.check_revision(expected)?;
        next.update()?;
        self.commit_with_action(next, expected, action)
    }

    fn commit_with_action(
        &mut self,
        next: System,
        expected: u64,
        action: &str,
    ) -> Result<Value, String> {
        self.commit_project(next, expected, action, self.projects.clone())
    }

    fn commit_project(
        &mut self,
        next: System,
        expected: u64,
        action: &str,
        projects: ProjectFiles,
    ) -> Result<Value, String> {
        self.check_revision(expected)?;
        self.persist(next, &projects)?;
        self.projects = projects;
        self.history.push(
            self.revision,
            action,
            self.system.clone(),
            self.projects.linked_path.clone(),
        );
        Ok(json!({"revision":self.revision}))
    }

    fn persist(&mut self, next: System, projects: &ProjectFiles) -> Result<(), String> {
        let revision = self
            .revision
            .checked_add(1)
            .ok_or("Workspace revision exhausted")?;
        persist_system(&self.path, &next, projects, projects != &self.projects)?;
        self.system = next;
        self.revision = revision;
        Ok(())
    }

    fn step_history(&mut self, expected: u64, undo: bool) -> Result<Value, String> {
        self.check_revision(expected)?;
        let entry = if undo {
            self.history.peek_undo()?
        } else {
            self.history.peek_redo()?
        };
        let mut projects = self.projects.clone();
        projects.linked_path = entry.linked_path.clone();
        self.persist(entry.system.clone(), &projects)?;
        self.projects = projects;
        if undo {
            self.history.step_undo(self.revision);
        } else {
            self.history.step_redo(self.revision);
        }
        Ok(json!({
            "revision": self.revision,
            "cursor": self.history.cursor(),
            "can_undo": self.history.can_undo(),
            "can_redo": self.history.can_redo(),
        }))
    }

    fn project_open(&mut self, path: &str, expected: u64) -> Result<Value, String> {
        self.check_revision(expected)?;
        let path = ProjectFiles::resolve(&self.path, path, false)?;
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let linked = !path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("zmx"));
        let mut next: System = if linked {
            serde_json::from_str(&text).map_err(|e| format!("Invalid optical project: {e}"))?
        } else {
            zmx::import(&text)?
        };
        next.update()?;
        next.validate()?;
        let mut projects = self.projects.clone();
        projects.select(&path, linked);
        let mut result = self.commit_project(next, expected, "project_open", projects)?;
        result["path"] = json!(path);
        result["linked"] = json!(linked);
        Ok(result)
    }

    fn project_save_as(&mut self, path: &str, expected: u64) -> Result<Value, String> {
        self.check_revision(expected)?;
        let path = ProjectFiles::resolve(&self.path, path, true)?;
        let mut projects = self.projects.clone();
        projects.select(&path, true);
        let mut result =
            self.commit_project(self.system.clone(), expected, "project_save_as", projects)?;
        result["path"] = json!(path);
        Ok(result)
    }

    pub fn call(&mut self, name: &str, args: Value) -> Result<Value, String> {
        validate_args(name, &args)?;
        self.call_validated(name, args)
    }

    fn call_validated(&mut self, name: &str, args: Value) -> Result<Value, String> {
        match name {
            "history_list" => Ok(self.history.list(self.revision)),
            "history_compare" => {
                let (from, to, from_index, to_index) = self.history.pair(&args)?;
                Ok(history::compare(from, to, from_index, to_index))
            }
            "project_list" => Ok(self.projects.list(&self.path)),
            "optimize" if args["apply"] == true => {
                let expected = args["revision"].as_u64().unwrap();
                self.check_revision(expected)?;
                let result = run_optimization(&self.system, args)?;
                self.apply_optimization(result, expected)
            }
            _ if is_mutating(name, &args) => self.mutate(name, args),
            _ => read_system(&self.system, self.revision, name, args),
        }
    }

    fn apply_optimization(
        &mut self,
        result: optimize::OptimizationResult,
        expected: u64,
    ) -> Result<Value, String> {
        let mut response = optimization_response(&result, true, expected)?;
        let committed = self.commit_with_action(result.system, expected, "optimize")?;
        response["revision"] = committed["revision"].clone();
        Ok(response)
    }

    fn mutate(&mut self, name: &str, args: Value) -> Result<Value, String> {
        let expected = args["revision"].as_u64().unwrap();
        self.check_revision(expected)?;
        match name {
            "bench_set" => {
                let scene: bench::Bench =
                    serde_json::from_value(args["bench"].clone()).map_err(|e| e.to_string())?;
                scene.validate()?;
                let mut next = self.system.clone();
                next.bench = Some(scene);
                self.commit_with_action(next, expected, name)
            }
            "bench_template" | "bench_auto_aim" => {
                let mut result = bench_preview(&self.system, name, &args)?;
                let scene = serde_json::from_value::<bench::Bench>(result["bench"].clone())
                    .map_err(|e| e.to_string())?;
                scene.validate_groups(&self.system)?;
                let mut next = self.system.clone();
                next.bench = Some(scene);
                let committed = self.commit_with_action(next, expected, name)?;
                result["revision"] = committed["revision"].clone();
                result["applied"] = json!(true);
                Ok(result)
            }
            "system_set" => {
                let s: System =
                    serde_json::from_value(args["system"].clone()).map_err(|e| e.to_string())?;
                self.replace(s, expected, name)
            }
            "configuration_activate" => {
                let index = args["index"].as_u64().unwrap() as usize;
                if index >= self.system.configurations.len().max(1) {
                    return Err("Configuration index outside the configuration table".into());
                }
                let mut next = self.system.clone();
                next.active_configuration = index;
                self.replace(next, expected, name)
            }
            "catalog_import" => {
                let mut next = self.system.clone();
                let catalog = args["catalog"].as_str().unwrap();
                let glasses = material::parse_agf(catalog, args["text"].as_str().unwrap())?;
                let count = glasses.glasses.len();
                next.custom_glasses
                    .retain(|g| g.catalog != catalog.to_uppercase());
                next.custom_glasses.extend(glasses.glasses);
                let mut r = self.replace(next, expected, name)?;
                r["glasses_imported"] = json!(count);
                r["warnings"] = json!(glasses.warnings);
                Ok(r)
            }
            "autofocus" => {
                let resolved = self.system.resolved()?;
                let w = args["wavelength"]
                    .as_f64()
                    .unwrap_or(self.system.reference());
                let s = solve::autofocus(&resolved, w)?;
                let mut next = self.system.clone();
                for (i, surface) in s.surfaces.iter().enumerate() {
                    if surface.thickness == resolved.surfaces[i].thickness {
                        continue;
                    }
                    let covered = next
                        .configurations
                        .get_mut(next.active_configuration)
                        .and_then(|c| {
                            c.overrides.iter_mut().find_map(|o| match o {
                                crate::optics::Override::Thickness { surface: k, value }
                                    if *k == i =>
                                {
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
                self.commit_with_action(next, expected, name)
            }
            "solve" => {
                let definition = serde_json::from_value(args["definition"].clone())
                    .map_err(|e| e.to_string())?;
                let (next, result) = solve::apply(
                    &self.system,
                    args["gap"].as_u64().unwrap() as usize,
                    definition,
                )?;
                let mut r = self.commit_with_action(next, expected, name)?;
                r["solve"] = result;
                Ok(r)
            }
            "solve_recalculate" => {
                let mut r = self.replace(self.system.clone(), expected, name)?;
                r["solves"] = json!(solve::states(&self.system));
                Ok(r)
            }
            "vignetting_update" => {
                let resolved = self.system.resolved()?;
                let (solved, report) = aperture::update_vignetting(&resolved)?;
                let mut next = self.system.clone();
                next.fields = solved.fields;
                let mut result = serde_json::to_value(report).map_err(|e| e.to_string())?;
                let committed = self.commit_with_action(next, expected, name)?;
                result["revision"] = committed["revision"].clone();
                Ok(result)
            }
            "project_import" => {
                let text = args["text"].as_str().unwrap();
                let next = match args["format"].as_str().unwrap() {
                    "geopter" => project::import(text)?,
                    "zmx" => zmx::import(text)?,
                    "hissabeth" => serde_json::from_str(text).map_err(|e| e.to_string())?,
                    _ => unreachable!(),
                };
                self.replace(next, expected, name)
            }
            "project_new" => {
                let mut next = project::blank();
                next.update()?;
                let mut projects = self.projects.clone();
                projects.linked_path = None;
                self.commit_project(next, expected, name, projects)
            }
            "project_open" => self.project_open(args["path"].as_str().unwrap(), expected),
            "project_save_as" => self.project_save_as(args["path"].as_str().unwrap(), expected),
            "history_undo" => self.step_history(expected, true),
            "history_redo" => self.step_history(expected, false),
            "history_restore" => {
                let index = args["index"].as_u64().unwrap() as usize;
                let entry = self.history.entry(index)?;
                let mut projects = self.projects.clone();
                projects.linked_path = entry.linked_path.clone();
                self.commit_project(entry.system.clone(), expected, "restore", projects)
            }
            _ => Err(format!("Unknown mutating tool {name}")),
        }
    }
}

fn check_revision(current: u64, expected: u64) -> Result<(), String> {
    if current != expected {
        return Err("System changed; read it again before editing".into());
    }
    Ok(())
}

fn run_optimization(
    system: &System,
    mut args: Value,
) -> Result<optimize::OptimizationResult, String> {
    let fields = args.as_object_mut().unwrap();
    fields.remove("apply");
    fields.remove("revision");
    let request = serde_json::from_value(args).map_err(|e| e.to_string())?;
    optimize::run(system, &request)
}

fn optimization_response(
    result: &optimize::OptimizationResult,
    applied: bool,
    revision: u64,
) -> Result<Value, String> {
    let mut response = serde_json::to_value(result).map_err(|e| e.to_string())?;
    response["applied"] = json!(applied);
    response["revision"] = json!(revision);
    Ok(response)
}

fn is_mutating(name: &str, args: &Value) -> bool {
    matches!(
        name,
        "system_set"
            | "bench_set"
            | "configuration_activate"
            | "catalog_import"
            | "autofocus"
            | "solve"
            | "solve_recalculate"
            | "vignetting_update"
            | "project_new"
            | "project_open"
            | "project_save_as"
            | "project_import"
            | "workspace_retry"
            | "workspace_recover"
            | "history_undo"
            | "history_redo"
            | "history_restore"
    ) || (matches!(name, "optimize" | "bench_template" | "bench_auto_aim") && args["apply"] == true)
}

pub fn dispatch(
    workspace: &SharedWorkspace,
    name: &str,
    args: Value,
    on_change: Option<&ChangeNotifier>,
) -> Result<Value, String> {
    schema::validate_args(name, &args)?;
    if !is_mutating(name, &args) {
        return dispatch_read(workspace, name, args);
    }
    let optimization = if name == "optimize" {
        let expected = args["revision"].as_u64().unwrap();
        let system = {
            let guard = workspace.read().map_err(|e| e.to_string())?;
            let engine = guard.engine()?;
            engine.check_revision(expected)?;
            engine.system.clone()
        };
        Some((run_optimization(&system, args.clone())?, expected))
    } else {
        None
    };
    let (result, before, after) = {
        let mut guard = workspace.write().map_err(|e| e.to_string())?;
        let before = guard.revision();
        let result = if let Some((result, expected)) = optimization {
            guard.engine_mut()?.apply_optimization(result, expected)?
        } else {
            guard.call_validated(name, args)?
        };
        (result, before, guard.revision())
    };
    if let Some(notify) = on_change.filter(|_| before != after) {
        notify(after);
    }
    Ok(result)
}

fn supplied_bench(args: &Value) -> Result<Option<bench::Bench>, String> {
    args.get("bench")
        .map(|value| serde_json::from_value(value.clone()).map_err(|e| e.to_string()))
        .transpose()
}

fn selected_bench<'a>(
    system: &'a System,
    supplied: &'a Option<bench::Bench>,
) -> Result<&'a bench::Bench, String> {
    supplied
        .as_ref()
        .or(system.bench.as_ref())
        .ok_or_else(|| "Create a free-space bench first".into())
}

fn bench_template_preview(args: &Value) -> Result<Value, String> {
    let scene = bench_lab::template(args["name"].as_str().unwrap())?;
    Ok(json!({"bench":scene,"applied":false}))
}

fn bench_aim_preview(scene: &bench::Bench, args: &Value) -> Result<Value, String> {
    let mut result = bench_lab::auto_aim(
        scene,
        args["snap_holes"].as_bool().unwrap_or(false),
        args["uniform_z"].as_f64(),
    )?;
    result["applied"] = json!(false);
    Ok(result)
}

fn bench_preview(system: &System, name: &str, args: &Value) -> Result<Value, String> {
    if name == "bench_template" {
        bench_template_preview(args)
    } else {
        bench_aim_preview(selected_bench(system, &supplied_bench(args)?)?, args)
    }
}

fn lab_catalog(args: &Value) -> Value {
    bench_lab::catalog(args["query"].as_str(), args["kind"].as_str())
}

fn dispatch_read(workspace: &SharedWorkspace, name: &str, args: Value) -> Result<Value, String> {
    match name {
        "bench_catalog" => return Ok(lab_catalog(&args)),
        "bench_template" => return bench_template_preview(&args),
        "bench_auto_aim" if args.get("bench").is_some() => {
            return bench_aim_preview(&supplied_bench(&args)?.unwrap(), &args);
        }
        _ => {}
    }
    let guard = workspace.read().map_err(|e| e.to_string())?;
    if name == "workspace_status" {
        return Ok(guard.status());
    }
    let engine = guard.engine()?;
    match name {
        "history_list" => Ok(engine.history.list(engine.revision)),
        "history_compare" => {
            let (from, to, from_index, to_index) = engine.history.pair(&args)?;
            let (from, to) = (from.clone(), to.clone());
            drop(guard);
            Ok(history::compare(&from, &to, from_index, to_index))
        }
        "project_list" => {
            let (path, projects) = (engine.path.clone(), engine.projects.clone());
            drop(guard);
            Ok(projects.list(&path))
        }
        _ => {
            let (system, revision) = (engine.system.clone(), engine.revision);
            drop(guard);
            read_system(&system, revision, name, args)
        }
    }
}

fn read_system(system: &System, revision: u64, name: &str, args: Value) -> Result<Value, String> {
    let w = args["wavelength"].as_f64().unwrap_or(system.reference());
    let field = || field_argument(system, &args);
    let resolved = || system.resolved();
    let grid = args["grid"].as_u64().unwrap_or(17) as usize;
    let samples = args["samples"].as_u64().unwrap_or(33) as usize;
    let mut result = match name {
        "bench_get" => Ok(json!({"bench":system.bench,"revision":revision})),
        "bench_example" => Ok(json!({"bench":bench::example()})),
        "bench_catalog" => Ok(lab_catalog(&args)),
        "bench_template" => bench_template_preview(&args),
        "bench_auto_aim" => {
            let mut result = bench_preview(system, name, &args)?;
            if args.get("bench").is_none() {
                result["revision"] = json!(revision);
            }
            Ok(result)
        }
        "bench_trace"
        | "bench_assembly"
        | "bench_lab_check"
        | "bench_alignment_guide"
        | "bench_export_blueprint" => {
            let supplied = supplied_bench(&args)?;
            let scene = selected_bench(system, &supplied)?;
            let mut value = match name {
                "bench_trace" => {
                    serde_json::to_value(scene.trace(system)?).map_err(|e| e.to_string())?
                }
                "bench_assembly" => {
                    scene.validate()?;
                    scene.assembly()
                }
                "bench_lab_check" => bench_lab::lab_check(scene, system)?,
                "bench_alignment_guide" => bench_lab::alignment_guide(scene, system)?,
                _ => bench_lab::export_blueprint(scene, system, args["format"].as_str().unwrap())?,
            };
            value["revision"] = json!(revision);
            value["saved"] = json!(supplied.is_none());
            Ok(value)
        }
        "system_get" => Ok(json!({"system":system,"revision":revision})),
        "system_validate" => {
            let mut s: System =
                serde_json::from_value(args["system"].clone()).map_err(|e| e.to_string())?;
            s.update()?;
            Ok(json!({"valid":true,"system":s}))
        }
        "system_resolved" => Ok(json!({"system":resolved()?,"revision":revision})),
        "surface_edit" => {
            let system: System =
                serde_json::from_value(args["system"].clone()).map_err(|e| e.to_string())?;
            let operation: editing::Operation =
                serde_json::from_value(args["operation"].clone()).map_err(|e| e.to_string())?;
            if system
                .configurations
                .iter()
                .any(|c| !c.overrides.is_empty())
            {
                return Err(
                    "Remove configuration overrides before inserting or removing surfaces".into(),
                );
            }
            serde_json::to_value(editing::apply(&system, operation)?).map_err(|e| e.to_string())
        }
        "materials_list" => {
            let mut names: Vec<String> = material::catalogs()
                .iter()
                .chain(&system.custom_glasses)
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
                .chain(&system.custom_glasses)
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
            let temperature = args["temperature"].as_f64().unwrap_or(system.temperature);
            let pressure = args["pressure"].as_f64().unwrap_or(system.pressure);
            let mut s = system.clone();
            s.temperature = temperature;
            s.pressure = pressure;
            let n = s.material_index(name, w)?;
            Ok(json!({"index":n,"wavelength":w,"temperature":temperature,"pressure":pressure}))
        }
        "air_index" => Ok(
            json!({"index":material::air_index(w/1000.,args["temperature"].as_f64().unwrap_or(system.temperature),args["pressure"].as_f64().unwrap_or(system.pressure))?}),
        ),
        "prescription_report" => serde_json::to_value(report::generate(
            &resolved()?,
            revision,
            serde_json::from_value(args["sections"].clone()).map_err(|e| e.to_string())?,
            w,
            args["precision"].as_u64().unwrap() as usize,
        )?)
        .map_err(|e| e.to_string()),
        "paraxial" => resolved()?.paraxial(w),
        "paraxial_trace" => inspection::paraxial_trace(&resolved()?, &field()?, w),
        "trace_inspect" => inspection::inspect(
            &resolved()?,
            serde_json::from_value(args["launch"].clone()).map_err(|e| e.to_string())?,
            w,
        ),
        "trace_ray" => serde_json::to_value(resolved()?.trace_field(
            args["x"].as_f64().unwrap_or(0.),
            args["y"].as_f64().unwrap_or(0.),
            &field()?,
            w,
        )?)
        .map_err(|e| e.to_string()),
        "spot_diagram" => resolved()?.spot_field(
            &field()?,
            w,
            serde_json::from_value::<PupilSampling>(args["sampling"].clone())
                .map_err(|e| e.to_string())?,
        ),
        "polychromatic_spot" => analysis::polychromatic_spot(
            &resolved()?,
            &field()?,
            serde_json::from_value::<PupilSampling>(args["sampling"].clone())
                .map_err(|e| e.to_string())?,
        ),
        "layout" => Ok(resolved()?.layout()),
        "geometric_mtf" => crate::compute::geometric_mtf(&resolved()?, &args),
        "ray_fan" => analysis::ray_fan(&resolved()?, &field()?, w, samples),
        "opd_fan" => analysis::opd_fan(&resolved()?, &field()?, w, samples),
        "chromatic_focus" => {
            let resolved = system.resolved()?;
            analysis::chromatic_focus(
                &resolved,
                args["lower"].as_f64().unwrap_or_else(|| {
                    resolved
                        .wavelengths
                        .iter()
                        .map(|w| w.value)
                        .fold(f64::INFINITY, f64::min)
                }),
                args["upper"].as_f64().unwrap_or_else(|| {
                    resolved
                        .wavelengths
                        .iter()
                        .map(|w| w.value)
                        .fold(f64::NEG_INFINITY, f64::max)
                }),
                samples,
            )
        }
        "longitudinal_aberration" => analysis::longitudinal(&resolved()?, w, samples),
        "spherochromatism" => {
            let resolved = system.resolved()?;
            let reference_wavelength = resolved.reference();
            let reference_focus = resolved.paraxial(reference_wavelength)?["image_distance"]
                .as_f64()
                .ok_or("No finite reference focus")?;
            let curves: Vec<_> = resolved
                .wavelengths
                .iter()
                .enumerate()
                .map(|(index, wv)| -> Result<Value, String> {
                    let mut curve = analysis::longitudinal(&resolved, wv.value, samples)?;
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
        "astigmatism" => analysis::astigmatism(&resolved()?, &field()?, w),
        "field_curvature" => focus::field_curvature(
            &resolved()?,
            args.get("field_end")
                .map(|v| serde_json::from_value(v.clone()).map_err(|e| e.to_string()))
                .transpose()?,
            samples,
        ),
        "distortion" => analysis::distortion(&resolved()?, &field()?, w),
        "wavefront" => Ok(analysis::wavefront(&resolved()?, &field()?, w, grid)?.json()),
        "diffraction_psf" => analysis::diffraction_psf(
            &resolved()?,
            &field()?,
            w,
            args["grid"].as_u64().unwrap_or(32) as usize,
        ),
        "diffraction_mtf" => analysis::diffraction_mtf(
            &resolved()?,
            &field()?,
            serde_json::from_value::<Spectrum>(args["spectrum"].clone())
                .map_err(|e| e.to_string())?,
            args["grid"].as_u64().unwrap_or(32) as usize,
            args["max_frequency"].as_f64(),
        ),
        "wavefront_zernike" => zernike::fit(
            &analysis::wavefront(&resolved()?, &field()?, w, grid)?,
            args["terms"].as_u64().unwrap_or(37) as usize,
        ),
        "huygens_psf" => analysis::huygens_psf(
            &resolved()?,
            &field()?,
            w,
            args["grid"].as_u64().unwrap_or(32) as usize,
            args["size"].as_u64().map(|v| v as usize),
            args["pitch"].as_f64(),
        ),
        "huygens_mtf" => analysis::huygens_mtf(
            &resolved()?,
            &field()?,
            serde_json::from_value::<Spectrum>(args["spectrum"].clone())
                .map_err(|e| e.to_string())?,
            args["grid"].as_u64().unwrap_or(32) as usize,
            args["max_frequency"].as_f64(),
        ),
        "gaussian_beam" => gaussian::gaussian_beam(
            &resolved()?,
            w,
            args["waist"].as_f64().unwrap(),
            args["waist_z"].as_f64().unwrap_or(0.),
        ),
        "fiber_coupling" => gaussian::fiber_coupling(
            &resolved()?,
            w,
            args["waist"].as_f64().unwrap(),
            args["waist_z"].as_f64().unwrap_or(0.),
            args["mode_field_diameter"].as_f64().unwrap(),
        ),
        "encircled_energy" => analysis::encircled_energy(
            &resolved()?,
            &field()?,
            w,
            serde_json::from_value::<PupilSampling>(args["sampling"].clone())
                .map_err(|e| e.to_string())?,
            samples,
        ),
        "surface_sag" => {
            let i = args["surface"].as_u64().unwrap() as usize;
            let resolved = resolved()?;
            let s = resolved
                .surfaces
                .get(i)
                .ok_or("Surface index outside prescription")?;
            let h = args["height"].as_f64().unwrap();
            let (z, dz) = optics::sag(s, h)?;
            Ok(json!({"sag":z,"derivative":dz,"height":h,"units":"mm"}))
        }
        "aperture_report" => {
            let mut report = serde_json::to_value(aperture::semi_diameters(&resolved()?)?)
                .map_err(|e| e.to_string())?;
            report["revision"] = json!(revision);
            Ok(report)
        }
        "project_export" => {
            let v = if args["format"] == "geopter" {
                project::export(system)?
            } else if args["format"] == "zmx" {
                return Ok(
                    json!({"text":zmx::export(system)?,"revision":revision,"name":system.name,"format":args["format"]}),
                );
            } else {
                serde_json::to_value(system).map_err(|e| e.to_string())?
            };
            Ok(
                json!({"text":serde_json::to_string_pretty(&v).map_err(|e|e.to_string())?,"revision":revision,"name":system.name,"format":args["format"]}),
            )
        }
        "design_summary" => optimize::design_summary(&resolved()?),
        "optimize" => optimization_response(&run_optimization(system, args)?, false, revision),
        "tolerance_sensitivity" => {
            let request = serde_json::from_value(args).map_err(|e| e.to_string())?;
            serde_json::to_value(tolerance::sensitivity(system, &request)?)
                .map_err(|e| e.to_string())
        }
        "tolerance_monte_carlo" => {
            let request = serde_json::from_value(args).map_err(|e| e.to_string())?;
            serde_json::to_value(tolerance::monte_carlo(system, &request)?)
                .map_err(|e| e.to_string())
        }
        "capabilities" => {
            let mut scope = json!({"sequential":true,"finite_conjugates":true,"curved_object_surface":true,"curved_image_surface":true,"endpoint_apertures":true,"object_medium":true,"prescribed_plot_colors":true,"internal_stop_aiming":true,"centered_surfaces":true,"centered_reflection":true,"ray_inspection":true,"paraxial_ray_tables":true,"assembled_prescription_report":true,"startup_recovery":true,"surface_row_editing":true,"project_creation":true,"automatic_semidiameters":true,"directional_vignetting":true,"field_local_analyses":true,"continuous_field_curvature":true,"pupil_sampling":["grid","hexapolar"],"pupil_mappings":["reference_entrance_pupil","real_stop"],"weighted_polychromatic_mtf":true,"analysis_revisions":true,"project_exchange":["hissabeth","geopter","zmx"],"folded_coordinates":true,"polarization":false,"nonsequential":false,"free_space_bench":true,"bench_spatial_poses":true,"bench_scalar_splitters":true,"bench_nearest_hits":true,"bench_assembly":true,"bench_lens_groups":true});
            scope["bench_fresnel_splitters"] = json!(true);
            scope["bench_coatings"] = json!(true);
            scope["bench_scatter"] = json!(["lambertian", "abg"]);
            scope["bench_group_ghosts"] = json!(2);
            scope["bench_polarization"] = json!(true);
            scope["bench_gaussian_envelope"] = json!(true);
            scope["bench_hardware_catalog"] = json!(true);
            scope["bench_auto_aim"] = json!(true);
            scope["bench_lab_check"] = json!(true);
            scope["bench_alignment_guide"] = json!(true);
            scope["bench_blueprint_exports"] = json!(bench_lab::FORMATS);
            scope["bench_detector_maps"] = json!(true);
            scope["bench_lens_model"] =
                json!("ideal thin lens or real current-prescription lens group");
            scope["surface_profiles"] = json!([
                "standard",
                "even_asphere",
                "odd_asphere",
                "toroid",
                "zernike_fringe_sag"
            ]);
            scope["huygens_diffraction"] = json!(true);
            scope["zernike_wavefront_fit"] = json!({"ordering":"noll","max_terms":45});
            scope["gaussian_beam"] = json!(true);
            scope["fiber_coupling"] = json!(true);
            scope["coordinate_breaks"] = json!(true);
            scope["optimization"] = json!({"solver":"dls","max_variables":64,"max_operands":256});
            scope["tolerancing"] = json!({"sensitivity":true,"monte_carlo":true,"max_trials":2000});
            scope["revision_history"] = json!(true);
            scope["multi_project"] = json!(true);
            scope["multi_configuration"] = json!({"max_configurations":64,"operands":["thickness","radius","conic","aperture","material","object_distance"]});
            Ok(
                json!({"tools":tools(),"units":{"length":"mm","wavelength":"nm","field_angle":"degrees","pressure":"Pa","temperature":"C"},"backend":"cpu","scope":scope}),
            )
        }
        _ => Err(format!("Unknown tool {name}")),
    }?;
    if [
        "prescription_report",
        "paraxial",
        "paraxial_trace",
        "trace_inspect",
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
        "field_curvature",
        "distortion",
        "wavefront",
        "diffraction_psf",
        "diffraction_mtf",
        "wavefront_zernike",
        "huygens_psf",
        "huygens_mtf",
        "gaussian_beam",
        "fiber_coupling",
        "encircled_energy",
        "surface_sag",
        "design_summary",
        "tolerance_sensitivity",
        "tolerance_monte_carlo",
    ]
    .contains(&name)
    {
        result["revision"] = json!(revision);
    }
    Ok(result)
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
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lab_tools_preview_report_export_and_commit_with_revision() {
        let (_dir, workspace) = shared_workspace();
        let call = |name, args| dispatch(&workspace, name, args, None);
        let initial = call("bench_get", json!({})).unwrap();
        let catalog = call("bench_catalog", json!({"query":"PBS251","kind":"pbs"})).unwrap();
        assert_eq!(catalog["items"][0]["part"], "PBS251");
        let preview = call("bench_template", json!({"name":"polarized_beam_combiner"})).unwrap();
        assert_eq!(preview["applied"], false);
        assert_eq!(call("bench_get", json!({})).unwrap(), initial);
        let mut draft = preview["bench"].clone();
        draft["sources"][0]["polarization"] = json!({"kind":"linear","angle_deg":45.});
        draft["sources"][0]["m2"] = json!(1.2);
        draft["elements"][1]["screw_tpi"] = json!(100.);
        let aim = call(
            "bench_auto_aim",
            json!({"bench":draft,"snap_holes":true,"uniform_z":80.}),
        )
        .unwrap();
        assert_eq!(aim["bench"]["sources"][0]["position"][2], 80.);
        assert_eq!(aim["applied"], false);
        let check = call("bench_lab_check", json!({"bench":aim["bench"]})).unwrap();
        assert_eq!(check["passed"], true);
        assert_eq!(check["saved"], false);
        let guide = call("bench_alignment_guide", json!({"bench":aim["bench"]})).unwrap();
        assert_eq!(guide["steps"][1]["id"], "HWP");
        let export = call(
            "bench_export_blueprint",
            json!({"bench":aim["bench"],"format":"svg"}),
        )
        .unwrap();
        assert_eq!(export["mime"], "image/svg+xml");
        assert_eq!(call("bench_get", json!({})).unwrap(), initial);
        assert!(call("bench_template", json!({"name":"michelson","apply":true})).is_err());
        let applied = call(
            "bench_template",
            json!({"name":"michelson","apply":true,"revision":initial["revision"]}),
        )
        .unwrap();
        assert_eq!(applied["applied"], true);
        assert!(
            call(
                "bench_auto_aim",
                json!({"bench":aim["bench"],"apply":true,"revision":initial["revision"]})
            )
            .is_err()
        );
        let committed = workspace
            .write()
            .unwrap()
            .call(
                "bench_auto_aim",
                json!({
                    "bench":aim["bench"],"apply":true,"revision":applied["revision"]
                }),
            )
            .unwrap();
        assert_eq!(committed["applied"], true);
        assert_eq!(
            call("bench_get", json!({})).unwrap()["bench"],
            committed["bench"]
        );
        let saved = call("bench_lab_check", json!({})).unwrap();
        assert_eq!(saved["saved"], true);
        assert_eq!(saved["revision"], committed["revision"]);
        let mut bad = aim["bench"].clone();
        bad["sources"][0]["polarization"] = json!({"kind":"horizontal","angle_deg":45.});
        assert!(call("bench_lab_check", json!({"bench":bad})).is_err());
    }

    fn shared_workspace() -> (tempfile::TempDir, SharedWorkspace) {
        let dir = tempfile::tempdir().unwrap();
        let workspace = Workspace::open(dir.path().join("system.json")).unwrap();
        (dir, Arc::new(RwLock::new(workspace)))
    }

    #[test]
    fn optimization_preview_and_apply_share_results_and_notify_after_commit() {
        let (dir, workspace) = shared_workspace();
        let initial = dispatch(&workspace, "system_get", json!({}), None).unwrap();
        let revision = initial["revision"].as_u64().unwrap();
        let mut args = json!({
            "variables": [{"kind":"radius","surface":0,"min":15.0,"max":500.0}],
            "operands": [{"kind":"efl","target":60.0}],
        });
        let observed = Arc::new(std::sync::Mutex::new(Vec::new()));
        let notifier: ChangeNotifier = {
            let workspace = workspace.clone();
            let observed = observed.clone();
            Arc::new(move |revision| {
                let guard = workspace
                    .try_write()
                    .expect("notifier runs after releasing the write lock");
                assert_eq!(guard.revision(), revision);
                observed.lock().unwrap().push(revision);
            })
        };
        let preview = dispatch(&workspace, "optimize", args.clone(), Some(&notifier)).unwrap();
        assert_eq!(preview["applied"], false);
        assert_eq!(preview["revision"], revision);
        assert!(
            preview["final_merit"].as_f64().unwrap() < preview["initial_merit"].as_f64().unwrap()
        );
        assert!(!dir.path().join("system.json").exists());
        assert!(observed.lock().unwrap().is_empty());

        args["apply"] = json!(true);
        assert!(dispatch(&workspace, "optimize", args.clone(), Some(&notifier)).is_err());
        args["revision"] = json!(revision);
        let applied = dispatch(&workspace, "optimize", args.clone(), Some(&notifier)).unwrap();
        assert_eq!(applied["applied"], true);
        assert_eq!(applied["revision"], revision + 1);
        assert_eq!(applied["system"], preview["system"]);
        assert_eq!(
            dispatch(&workspace, "system_get", json!({}), None).unwrap()["system"],
            applied["system"]
        );
        assert!(dispatch(&workspace, "optimize", args, Some(&notifier)).is_err());
        assert_eq!(*observed.lock().unwrap(), [revision + 1]);
        let history = dispatch(&workspace, "history_list", json!({}), None).unwrap();
        assert_eq!(history["entries"][1]["action"], "optimize");
    }

    #[test]
    fn optical_summary_and_tolerance_reads_can_run_concurrently() {
        let (_dir, workspace) = shared_workspace();
        let read = workspace.read().unwrap();
        let revision = read.revision();
        let tolerance = json!({
            "tolerances": [{"kind":"radius","surface":0,"delta_percent":0.5}],
            "criterion": {"kind":"efl"},
        });
        let mut monte_carlo = tolerance.clone();
        monte_carlo["trials"] = json!(10);
        let calls = [
            ("design_summary", json!({})),
            ("tolerance_sensitivity", tolerance),
            ("tolerance_monte_carlo", monte_carlo),
        ];
        let (send, receive) = std::sync::mpsc::channel();
        let workers: Vec<_> = calls
            .into_iter()
            .map(|(name, args)| {
                let workspace = workspace.clone();
                let send = send.clone();
                std::thread::spawn(move || {
                    send.send((name, dispatch(&workspace, name, args, None)))
                        .unwrap();
                })
            })
            .collect();
        let results: Vec<_> = (0..workers.len())
            .map(|_| receive.recv_timeout(std::time::Duration::from_secs(5)))
            .collect();
        drop(read);
        for worker in workers {
            worker.join().unwrap();
        }
        for result in results {
            let (name, value) =
                result.expect("optical reads must finish while another reader holds the lock");
            let value = value.unwrap();
            assert_eq!(value["revision"], revision);
            match name {
                "design_summary" => {
                    assert_eq!(value["name"], "BK7 singlet");
                    assert!(value["paraxial"]["efl"].as_f64().unwrap() > 0.);
                }
                "tolerance_sensitivity" => {
                    assert_eq!(value["entries"].as_array().unwrap().len(), 1);
                    assert!(value["rss_change"].as_f64().unwrap() > 0.);
                }
                "tolerance_monte_carlo" => assert_eq!(value["succeeded"], 10),
                _ => unreachable!(),
            }
        }
    }

    #[test]
    fn history_navigation_persists_snapshots_and_restore_is_undoable() {
        let (dir, workspace) = shared_workspace();
        let call = |name, args| dispatch(&workspace, name, args, None).unwrap();
        let initial = call("system_get", json!({}));
        let mut system = initial["system"].clone();
        let initial_radius = system["surfaces"][0]["radius"].as_f64().unwrap();
        system["surfaces"][0]["radius"] = json!(initial_radius + 10.);
        let edited = call(
            "system_set",
            json!({"revision":initial["revision"],"system":system}),
        );
        let compared = call("history_compare", json!({}));
        assert_eq!(compared["from"]["index"], 0);
        assert_eq!(compared["to"]["index"], 1);
        assert_eq!(
            compared["surface_changes"][0]["changes"][0]["field"],
            "radius"
        );

        let undone = call("history_undo", json!({"revision":edited["revision"]}));
        assert_eq!(undone["cursor"], 0);
        assert_eq!(undone["can_undo"], false);
        assert_eq!(undone["can_redo"], true);
        let disk = || {
            serde_json::from_slice::<Value>(&std::fs::read(dir.path().join("system.json")).unwrap())
                .unwrap()
        };
        assert_eq!(disk(), initial["system"]);

        let redone = call("history_redo", json!({"revision":undone["revision"]}));
        assert_eq!(redone["cursor"], 1);
        assert_eq!(redone["can_redo"], false);
        let restored = call(
            "history_restore",
            json!({"index":0,"revision":redone["revision"]}),
        );
        assert_eq!(disk(), initial["system"]);
        let history = call("history_list", json!({}));
        assert_eq!(history["entries"][2]["action"], "restore");
        assert_eq!(history["current_revision"], restored["revision"]);
        call("history_undo", json!({"revision":restored["revision"]}));
        assert_eq!(disk()["surfaces"][0]["radius"], initial_radius + 10.);
        let reversed = call("history_compare", json!({"from_index":1,"to_index":0}));
        assert_eq!(
            reversed["surface_changes"][0]["changes"][0]["after"],
            initial_radius
        );
    }

    #[test]
    fn project_files_link_update_reopen_and_detach() {
        let (dir, workspace) = shared_workspace();
        let path = dir.path().join("system.json");
        let call = |name, args| dispatch(&workspace, name, args, None).unwrap();
        let initial = call("system_get", json!({}));
        let first = call(
            "project_save_as",
            json!({"path":"singlet","revision":initial["revision"]}),
        );
        let first_path = dir.path().join("projects/singlet.hissabeth.json");
        assert_eq!(first["path"], json!(first_path));
        let mut system = initial["system"].clone();
        system["name"] = json!("Modified singlet");
        let edited = call(
            "system_set",
            json!({"system":system,"revision":first["revision"]}),
        );
        assert_eq!(
            std::fs::read(&path).unwrap(),
            std::fs::read(&first_path).unwrap()
        );

        let second = call(
            "project_save_as",
            json!({"path":"second.json","revision":edited["revision"]}),
        );
        system["name"] = json!("Second project");
        let edited_second = call(
            "system_set",
            json!({"system":system,"revision":second["revision"]}),
        );
        let first_contents = std::fs::read(&first_path).unwrap();
        let opened = call(
            "project_open",
            json!({"path":"singlet.hissabeth.json","revision":edited_second["revision"]}),
        );
        assert_eq!(opened["linked"], true);
        assert_eq!(opened["path"], first["path"]);
        let list = call("project_list", json!({}));
        assert_eq!(list["active_file"], first["path"]);
        assert_eq!(list["workspace_file"], json!(path));
        assert_eq!(list["recent"], json!([first["path"], second["path"]]));
        assert_eq!(
            list["saved_projects"],
            json!([second["path"], first["path"]])
        );
        let undone = call("history_undo", json!({"revision":opened["revision"]}));
        assert_eq!(
            call("project_list", json!({}))["active_file"],
            second["path"]
        );
        assert_eq!(
            call("system_get", json!({}))["system"]["name"],
            "Second project"
        );
        assert_eq!(std::fs::read(&first_path).unwrap(), first_contents);
        call("history_redo", json!({"revision":undone["revision"]}));

        drop(workspace);
        let workspace = Arc::new(RwLock::new(Workspace::open(path).unwrap()));
        let call = |name, args| dispatch(&workspace, name, args, None).unwrap();
        assert_eq!(call("project_list", json!({})), list);
        let status = call("workspace_status", json!({}));
        let zmx_path = dir.path().join("projects/import.ZMX");
        let exported = call("project_export", json!({"format":"zmx"}));
        let zmx_text = exported["text"].as_str().unwrap();
        std::fs::write(&zmx_path, zmx_text).unwrap();
        let imported = call(
            "project_open",
            json!({"path":"import.ZMX","revision":status["revision"]}),
        );
        assert_eq!(imported["linked"], false);
        assert_eq!(call("project_list", json!({}))["active_file"], Value::Null);
        assert_eq!(std::fs::read_to_string(&zmx_path).unwrap(), zmx_text);

        let opened = call(
            "project_open",
            json!({"path":"singlet.hissabeth.json","revision":imported["revision"]}),
        );
        let saved = std::fs::read(&first_path).unwrap();
        call("project_new", json!({"revision":opened["revision"]}));
        assert_eq!(std::fs::read(&first_path).unwrap(), saved);
        assert_eq!(call("project_list", json!({}))["active_file"], Value::Null);
    }

    #[test]
    fn bench_transaction_preview_roundtrip_and_restart() {
        let mut e = engine("bench-transaction");
        let revision = e.revision;
        let sequential = e.system.surfaces.clone();
        let scene = bench::example();
        let before = std::fs::read(&e.path).ok();
        let preview = e.call("bench_trace", json!({"bench":scene})).unwrap();
        assert_eq!(preview["saved"], false);
        assert_eq!(e.revision, revision);
        assert_eq!(before, std::fs::read(&e.path).ok());
        for args in [
            json!({"bench":scene}),
            json!({"bench":scene,"revision":revision+1}),
            json!({"bench":scene,"revision":revision,"extra":0}),
        ] {
            assert!(e.call("bench_set", args).is_err());
        }
        e.call("bench_set", json!({"bench":scene,"revision":revision}))
            .unwrap();
        let bytes = std::fs::read(&e.path).unwrap();
        assert!(
            e.call("bench_set", json!({"bench":scene,"revision":revision}))
                .is_err()
        );
        assert_eq!(bytes, std::fs::read(&e.path).unwrap());
        let mut invalid = scene.clone();
        invalid.connections[0].to = "missing".into();
        assert!(
            e.call("bench_set", json!({"bench":invalid,"revision":e.revision}))
                .is_err()
        );
        assert_eq!(bytes, std::fs::read(&e.path).unwrap());
        assert_eq!(
            serde_json::to_value(sequential).unwrap(),
            serde_json::to_value(&e.system.surfaces).unwrap()
        );
        assert_eq!(e.call("bench_trace", json!({})).unwrap()["saved"], true);
        let report = e.call("bench_assembly", json!({})).unwrap();
        assert_eq!(report["items"].as_array().unwrap().len(), 5);
        let export = e
            .call("project_export", json!({"format":"hissabeth"}))
            .unwrap();
        assert!(
            e.call("project_export", json!({"format":"geopter"}))
                .is_err()
        );
        let path = e.path.clone();
        drop(e);
        let mut reopened = Engine::open(path).unwrap();
        assert_eq!(
            serde_json::to_value(reopened.system.bench.as_ref().unwrap()).unwrap(),
            serde_json::to_value(&scene).unwrap()
        );
        let snapshot = reopened.call("system_get", json!({})).unwrap();
        let mut system = snapshot["system"].clone();
        system["name"] = json!("edited prescription");
        reopened
            .call(
                "system_set",
                json!({"system":system,"revision":reopened.revision}),
            )
            .unwrap();
        assert!(reopened.system.bench.is_some());
        let current = reopened.revision;
        reopened
            .call(
                "project_import",
                json!({"format":"hissabeth","text":export["text"],"revision":current}),
            )
            .unwrap();
        assert_eq!(reopened.system.bench.unwrap().name, scene.name);
    }
    #[test]
    fn project_new_clears_project_data_saves_and_reloads() {
        let mut e = engine("project-new");
        let mut old = e.system.clone();
        old.name = "Prior project".into();
        old.note = "Prior notes".into();
        old.object_distance = Some(200.);
        old.object_material = "FUSED_SILICA".into();
        old.object_label = "Prior object".into();
        old.image_label = "Prior image".into();
        old.object_surface.radius = 1000.;
        old.image_surface.even = vec![1e-9];
        old.stop_index = 1;
        old.pupil.value = 6.;
        old.surfaces[0].gap_solve = solve::GapSolve::Pickup {
            source: 1,
            scale: 0.1,
            offset: 0.,
        };
        let mut glass = material::catalogs()[0].clone();
        glass.catalog = "LOCAL".into();
        glass.name = "PriorGlass".into();
        old.custom_glasses = vec![glass];
        e.call("system_set", json!({"system":old,"revision":e.revision}))
            .unwrap();
        let previous = e.call("system_get", json!({})).unwrap();
        let bytes = std::fs::read(&e.path).unwrap();
        for args in [
            json!({}),
            json!({"revision":e.revision+1}),
            json!({"revision":e.revision,"extra":0}),
            json!({"revision":1.5}),
        ] {
            assert!(e.call("project_new", args).is_err());
        }
        assert_eq!(e.call("system_get", json!({})).unwrap(), previous);
        assert_eq!(std::fs::read(&e.path).unwrap(), bytes);
        let revision = e
            .call("project_new", json!({"revision":e.revision}))
            .unwrap()["revision"]
            .as_u64()
            .unwrap();
        assert_eq!(revision, previous["revision"].as_u64().unwrap() + 1);
        let mut expected = project::blank();
        expected.update().unwrap();
        assert_eq!(
            serde_json::to_value(&e.system).unwrap(),
            serde_json::to_value(&expected).unwrap()
        );
        let snapshot = e.call("system_get", json!({})).unwrap();
        let path = e.path.clone();
        drop(e);
        let mut reopened = Engine::open(path).unwrap();
        assert_eq!(
            reopened.call("system_get", json!({})).unwrap()["system"],
            snapshot["system"]
        );
        assert!(
            reopened.call("capabilities", json!({})).unwrap()["scope"]["project_creation"]
                .as_bool()
                .unwrap()
        );
    }
    #[test]
    fn project_new_failed_persistence_and_recovery_leave_source_unchanged() {
        let mut e = engine("project-new-failure");
        e.call(
            "system_set",
            json!({"system":e.system,"revision":e.revision}),
        )
        .unwrap();
        let before = e.call("system_get", json!({})).unwrap();
        let bytes = std::fs::read(&e.path).unwrap();
        let pending = e.path.with_extension("pending");
        std::fs::create_dir(&pending).unwrap();
        assert!(
            e.call("project_new", json!({"revision":e.revision}))
                .is_err()
        );
        assert_eq!(e.call("system_get", json!({})).unwrap(), before);
        assert_eq!(std::fs::read(&e.path).unwrap(), bytes);
        std::fs::remove_dir(pending).unwrap();
        let path = e.path.clone();
        drop(e);
        std::fs::write(&path, b"{ broken").unwrap();
        let mut recovery = Workspace::open(path.clone()).unwrap();
        let revision = recovery.call("workspace_status", json!({})).unwrap()["revision"].clone();
        assert!(
            recovery
                .call("project_new", json!({"revision":revision}))
                .unwrap_err()
                .contains("recovery")
        );
        assert_eq!(std::fs::read(path).unwrap(), b"{ broken");
    }
    #[test]
    fn surface_edits_stage_drafts_and_apply_constraints_on_save() {
        let mut e = engine("surface-edit");
        let saved = e.system.clone();
        e.call("system_set", json!({"system":saved,"revision":e.revision}))
            .unwrap();
        let before = e.call("system_get", json!({})).unwrap();
        let bytes = std::fs::read(&e.path).unwrap();
        let mut draft = e.system.clone();
        draft.name = "Unapplied edited source".into();
        draft.surfaces[0].gap_solve = solve::GapSolve::Pickup {
            source: 1,
            scale: 0.1,
            offset: 2.,
        };
        let result = e
            .call(
                "surface_edit",
                json!({"system":draft,"operation":{"kind":"insert","before":1,"count":2}}),
            )
            .unwrap();
        assert_eq!(result["system"]["name"], draft.name);
        assert_eq!(result["system"]["surfaces"][0]["gap_solve"]["source"], 3);
        assert_eq!(
            result["system"]["surfaces"][0]["thickness"],
            draft.surfaces[0].thickness
        );
        assert!(result.get("revision").is_none());
        for operation in [
            json!({"kind":"insert","before":1,"count":0}),
            json!({"kind":"insert","before":1,"count":1.5}),
            json!({"kind":"insert","before":1,"count":1,"extra":true}),
            json!({"kind":"remove","first":0,"count":2}),
            json!({"kind":"remove","first":1,"count":1}),
        ] {
            assert!(
                e.call(
                    "surface_edit",
                    json!({"system":draft,"operation":operation})
                )
                .is_err()
            );
        }
        let mut unfinished = draft.clone();
        unfinished.surfaces[0].material = "UnfinishedGlass".into();
        assert!(
            e.call(
                "surface_edit",
                json!({"system":unfinished,"operation":{"kind":"insert","before":0,"count":1}})
            )
            .is_ok()
        );
        assert_eq!(e.call("system_get", json!({})).unwrap(), before);
        assert_eq!(std::fs::read(&e.path).unwrap(), bytes);
        e.call(
            "system_set",
            json!({"system":result["system"],"revision":before["revision"]}),
        )
        .unwrap();
        assert_eq!(e.system.surfaces.len(), 4);
        assert!(
            (e.system.surfaces[0].thickness - (draft.surfaces[1].thickness * 0.1 + 2.)).abs()
                < 1e-10
        );
        assert_eq!(e.revision, before["revision"].as_u64().unwrap() + 1);
        assert_eq!(
            serde_json::from_slice::<Value>(&std::fs::read(&e.path).unwrap()).unwrap(),
            serde_json::to_value(&e.system).unwrap()
        );
        let path = engine("surface-edit-recovery").path.clone();
        std::fs::write(&path, b"{ invalid optical project").unwrap();
        let mut workspace = Workspace::open(path).unwrap();
        assert!(
            workspace
                .call(
                    "surface_edit",
                    json!({"system":draft,"operation":{"kind":"insert","before":0,"count":1}})
                )
                .unwrap_err()
                .contains("recovery")
        );
    }
    #[test]
    fn prescription_reports_validate_requests_and_leave_saved_state_unchanged() {
        let mut e = engine("prescription-report");
        let saved = e.system.clone();
        e.call("system_set", json!({"system":saved,"revision":e.revision}))
            .unwrap();
        let before = e.call("system_get", json!({})).unwrap();
        let bytes = std::fs::read(&e.path).unwrap();
        let report = e
            .call(
                "prescription_report",
                json!({"sections":["lens_data","general"],"wavelength":587.5618,"precision":4}),
            )
            .unwrap();
        assert_eq!(report["revision"], before["revision"]);
        assert_eq!(report["sections"][0]["id"], "general");
        assert_eq!(report["sections"][1]["id"], "lens_data");
        for args in [
            json!({}),
            json!({"sections":[],"wavelength":500}),
            json!({"sections":["general","general"],"wavelength":500,"precision":4}),
            json!({"sections":["nope"],"wavelength":500,"precision":4}),
            json!({"sections":[],"wavelength":500,"precision":4.5}),
            json!({"sections":[],"wavelength":500,"precision":16}),
            json!({"sections":[],"wavelength":99,"precision":4}),
            json!({"sections":[],"wavelength":500,"precision":4,"revision":0}),
        ] {
            assert!(e.call("prescription_report", args).is_err());
        }
        assert_eq!(e.call("system_get", json!({})).unwrap(), before);
        assert_eq!(std::fs::read(&e.path).unwrap(), bytes);
    }
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
    #[test]
    fn active_configuration_drives_analysis_and_writes() {
        use crate::optics::{Configuration, Override};
        let mut e = engine("configurations");
        let mut system = e.system.clone();
        system.configurations = vec![
            Configuration {
                name: "nominal".into(),
                overrides: vec![],
            },
            Configuration {
                name: "shifted".into(),
                overrides: vec![Override::Thickness {
                    surface: 1,
                    value: 50.,
                }],
            },
        ];
        let revision = e.revision;
        e.call("system_set", json!({"system":system,"revision":revision}))
            .unwrap();
        let doc = e.call("system_get", json!({})).unwrap();
        assert_eq!(doc["system"]["configurations"].as_array().unwrap().len(), 2);
        let resolved = e.call("system_resolved", json!({})).unwrap();
        assert_eq!(resolved["system"]["surfaces"][1]["thickness"], 46.64);
        let base_layout = e.call("layout", json!({})).unwrap();
        e.call(
            "configuration_activate",
            json!({"index":1,"revision":e.revision}),
        )
        .unwrap();
        let resolved = e.call("system_resolved", json!({})).unwrap();
        assert_eq!(resolved["system"]["surfaces"][1]["thickness"], 50.);
        assert_eq!(resolved["revision"], e.revision);
        let shifted_layout = e.call("layout", json!({})).unwrap();
        assert_ne!(base_layout["rays"], shifted_layout["rays"]);
        assert!(
            e.call(
                "configuration_activate",
                json!({"index":2,"revision":e.revision})
            )
            .is_err()
        );
        // Autofocus writes through the active configuration's override and
        // leaves the shared base thickness untouched.
        e.call("autofocus", json!({"revision":e.revision})).unwrap();
        let doc = e.call("system_get", json!({})).unwrap();
        assert_eq!(doc["system"]["surfaces"][1]["thickness"], 46.64);
        let focused = e.system.configurations[1].overrides[0].clone();
        let crate::optics::Override::Thickness { value, .. } = focused else {
            panic!("thickness override retained")
        };
        assert!((value - 50.).abs() > 1e-9);
        let resolved = e.call("system_resolved", json!({})).unwrap();
        assert_eq!(resolved["system"]["surfaces"][1]["thickness"], value);
        // With the plain configuration active, autofocus writes the base gap.
        e.call(
            "configuration_activate",
            json!({"index":0,"revision":e.revision}),
        )
        .unwrap();
        e.call("autofocus", json!({"revision":e.revision})).unwrap();
        let doc = e.call("system_get", json!({})).unwrap();
        let thickness = doc["system"]["surfaces"][1]["thickness"].as_f64().unwrap();
        assert!((thickness - 46.64).abs() > 1e-9);
        // Surface row editing is rejected while overrides index surfaces.
        let draft = e.call("system_get", json!({})).unwrap()["system"].clone();
        assert!(
            e.call(
                "surface_edit",
                json!({"system":draft,"operation":{"kind":"insert","before":1,"count":1}})
            )
            .is_err()
        );
        std::fs::remove_file(&e.path).unwrap();
    }
}
