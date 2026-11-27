// SPDX-License-Identifier: GPL-3.0-or-later
use hissabeth_lib::service::Engine;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::{
    fs,
    path::{Component, Path, PathBuf},
    process::Command,
};

const MANIFEST: &str = env!("CARGO_MANIFEST_DIR");

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    name: String,
    prescription: Prescription,
    checks: Vec<Check>,
}

#[derive(Deserialize)]
#[serde(untagged, deny_unknown_fields)]
enum Prescription {
    File { file: PathBuf },
    Inline { system: Value },
}

impl Prescription {
    fn upstream_path(file: &Path) -> PathBuf {
        assert!(
            !file.as_os_str().is_empty()
                && file
                    .components()
                    .all(|part| matches!(part, Component::Normal(_))),
            "Expected a path relative to reference/upstream: {file:?}"
        );
        Path::new(MANIFEST).join("../reference/upstream").join(file)
    }

    fn import_args(&self) -> Value {
        match self {
            Self::File { file } => {
                let path = Self::upstream_path(file);
                let text = fs::read_to_string(&path)
                    .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
                json!({"format": "geopter", "text": text})
            }
            Self::Inline { system } => json!({"format": "hissabeth", "text": system.to_string()}),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Check {
    quantity: String,
    args: Map<String, Value>,
    expected: Expected,
    tolerance: Tolerance,
    source: String,
    xfail: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Expected {
    Number(f64),
    Text(String),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Tolerance {
    class: ToleranceClass,
    abs: Option<f64>,
    rel: Option<f64>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ToleranceClass {
    Exact,
    Tight,
    Loose,
}

impl Tolerance {
    fn limit(&self, expected: &Expected) -> f64 {
        let (abs, rel) = match self.class {
            ToleranceClass::Exact => {
                assert!(self.abs.is_none() && self.rel.is_none());
                return 0.;
            }
            ToleranceClass::Tight => (1e-12, 1e-9),
            ToleranceClass::Loose => (1e-6, 1e-3),
        };
        let Expected::Number(expected) = expected else {
            panic!("Numeric tolerance requires a numeric expectation");
        };
        let (abs, rel) = (self.abs.unwrap_or(abs), self.rel.unwrap_or(rel));
        assert!(abs >= 0. && rel >= 0.);
        let limit = abs.max(rel * expected.abs());
        assert!(limit.is_finite());
        limit
    }
}

impl Check {
    fn step(&self) -> Value {
        let mut args = self.args.clone();
        let command = match self.quantity.as_str() {
            "ray_image_y" => "trace_ray",
            "spot_blocked" => "spot_diagram",
            "geometric_mtf" => {
                args.remove("frequency");
                "geometric_mtf"
            }
            _ => "paraxial",
        };
        json!({"command": command, "args": args})
    }

    fn mtf_sample<'a>(&self, frequencies: &Value, values: &'a Value) -> Option<&'a Value> {
        let target = self.args.get("frequency")?.as_f64()?;
        let index = frequencies
            .as_array()?
            .iter()
            .position(|frequency| frequency.as_f64() == Some(target))?;
        values.get(index)
    }

    fn actual<'a>(&self, result: &'a Value) -> Option<&'a Value> {
        match self.quantity.as_str() {
            "ray_image_y" => result.get("points")?.as_array()?.last()?.get(1),
            "spot_blocked" => result.get("blocked"),
            "geometric_mtf" => self.mtf_sample(&result["frequencies"], &result["mtf"]),
            key => result.get(key),
        }
    }

    fn cpp_expected<'a>(&self, result: &'a Value) -> Option<&'a Value> {
        match self.quantity.as_str() {
            "ray_image_y" => result.get("chief_rays")?.get(self.field_index())?.get(1),
            "geometric_mtf" => {
                let branch = if self.args["axis"] == "x" { "S" } else { "T" };
                let name = format!("F{}_{branch}", self.field_index());
                let curve = result["curves"]
                    .as_array()?
                    .iter()
                    .find(|c| c["name"] == name)?;
                self.mtf_sample(&curve["frequencies"], &curve["values"])
            }
            key => result.get(key),
        }
    }

    fn field_index(&self) -> usize {
        self.args.get("field_index").map_or(0, |value| {
            usize::try_from(value.as_u64().expect("Expected a field index")).unwrap()
        })
    }

    fn matches(&self, actual: &Value, limit: f64) -> Option<bool> {
        match &self.expected {
            Expected::Text(expected) => Some(actual.as_str()? == expected),
            Expected::Number(expected) => Some((actual.as_f64()? - expected).abs() <= limit),
        }
    }
}

fn cases() -> Vec<Case> {
    let dir = Path::new(MANIFEST).join("tests/oracle/cases");
    let mut paths: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "No oracle cases");
    paths
        .into_iter()
        .map(|path| {
            let case: Case = serde_json::from_slice(&fs::read(&path).unwrap())
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            assert!(!case.checks.is_empty(), "{} has no checks", case.name);
            case
        })
        .collect()
}

fn run_corpus(mut execute: impl FnMut(Value) -> Result<Value, String>) {
    let cases = cases();
    let total: usize = cases.iter().map(|case| case.checks.len()).sum();
    let (mut passed, mut xfailed) = (0, 0);
    for case in &cases {
        let mut steps =
            vec![json!({"command": "project_import", "args": case.prescription.import_args()})];
        let indices: Vec<_> = case
            .checks
            .iter()
            .map(|check| {
                let step = check.step();
                steps
                    .iter()
                    .position(|existing| *existing == step)
                    .unwrap_or_else(|| {
                        steps.push(step);
                        steps.len() - 1
                    })
            })
            .collect();
        let report = match execute(json!({"steps": steps})) {
            Ok(report) if report["ok"] == true => report,
            Ok(report) => {
                eprintln!("FAIL {}: {}", case.name, report["failed"]);
                continue;
            }
            Err(error) => {
                eprintln!("FAIL {}: {error}", case.name);
                continue;
            }
        };
        for (check, index) in case.checks.iter().zip(indices) {
            let label = format!(
                "{} / {} {} ({})",
                case.name,
                check.quantity,
                Value::Object(check.args.clone()),
                check.source
            );
            let limit = check.tolerance.limit(&check.expected);
            let result = &report["results"][index]["result"];
            let actual = check.actual(result).unwrap_or(&Value::Null);
            match (check.matches(actual, limit), &check.xfail) {
                (Some(true), None) => passed += 1,
                (Some(true), Some(reason)) => eprintln!("XPASS {label}: {reason}"),
                (Some(false), Some(reason)) => {
                    xfailed += 1;
                    eprintln!(
                        "XFAIL {label}: {reason} (got {actual}, expected {:?}, tolerance {limit})",
                        check.expected
                    );
                }
                _ => eprintln!(
                    "FAIL {label}: got {actual}, expected {:?}, tolerance {limit}",
                    check.expected
                ),
            }
        }
    }
    let failed = total - passed - xfailed;
    println!(
        "Oracle: {passed}/{total} passed across {} cases ({xfailed} xfail, {failed} failed)",
        cases.len()
    );
    assert_eq!(failed, 0, "{failed} unexpected oracle failures");
}

#[test]
fn oracle_in_process() {
    let dir = tempfile::tempdir().unwrap();
    let mut engine = Engine::open(dir.path().join("system.json")).unwrap();
    run_corpus(|batch| engine.call("batch", batch));
}

#[test]
#[ignore = "run the same corpus through the headless executable"]
fn oracle_headless() {
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("oracle.json");
    run_corpus(|batch| {
        fs::write(&script, batch.to_string()).map_err(|error| error.to_string())?;
        let output = Command::new(env!("CARGO_BIN_EXE_hissabeth"))
            .args(["--engine", "--run"])
            .arg(&script)
            .env("HISSABETH_DATA_DIR", dir.path())
            .output()
            .map_err(|error| error.to_string())?;
        if !output.status.success() {
            return Err(format!(
                "{}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        serde_json::from_slice(&output.stdout).map_err(|error| error.to_string())
    });
}

#[test]
#[ignore = "requires a C++ Geopter build at GEOPTER_ORACLE"]
fn geopter_snapshots_match_cpp_oracle() {
    let binary = std::env::var_os("GEOPTER_ORACLE").expect("Set GEOPTER_ORACLE to geopter_oracle");
    let mut catalogs: Vec<_> = fs::read_dir(Path::new(MANIFEST).join("data/glass"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("agf"))
        })
        .collect();
    catalogs.sort();
    let mut matched = 0;
    for case in cases() {
        for (geometric, flags) in [(false, &[][..]), (true, &["--geometric"][..])] {
            let checks: Vec<_> = case
                .checks
                .iter()
                .filter(|check| {
                    check.source == "geopter" && (check.quantity == "geometric_mtf") == geometric
                })
                .collect();
            if checks.is_empty() {
                continue;
            }
            let Prescription::File { file } = &case.prescription else {
                panic!(
                    "{}: a Geopter snapshot requires an upstream file",
                    case.name
                );
            };
            let output = Command::new(&binary)
                .args(flags)
                .arg(Prescription::upstream_path(file))
                .args(&catalogs)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let result: Value = serde_json::from_slice(&output.stdout).unwrap();
            for check in checks {
                let actual = check.cpp_expected(&result).unwrap_or(&Value::Null);
                let limit = match check.expected {
                    // These C++ results depend on subtracting chief ray heights
                    // at the 1e10 mm infinity sentinel. Allow a few ulps there
                    // when reproducing the snapshot across compiler builds.
                    Expected::Number(_)
                        if matches!(
                            check.quantity.as_str(),
                            "entrance_pupil_distance" | "exit_pupil_radius"
                        ) =>
                    {
                        4. * f64::EPSILON * 1e10
                    }
                    Expected::Number(value) => 1e-12 * value.abs().max(1.),
                    Expected::Text(_) => 0.,
                };
                assert_eq!(
                    check.matches(actual, limit),
                    Some(true),
                    "{} / {} {:?}: C++ returned {actual}, fixture expects {:?}",
                    case.name,
                    check.quantity,
                    check.args,
                    check.expected
                );
                matched += 1;
            }
        }
    }
    assert!(matched > 0, "No Geopter snapshots in the corpus");
    println!("Geopter snapshots: {matched}/{matched} matched");
}
