// SPDX-License-Identifier: GPL-3.0-or-later
use super::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Request {
    steps: Vec<Step>,
    #[serde(default = "rollback_by_default")]
    rollback: bool,
    #[schemars(default, with = "u64")]
    revision: Option<u64>,
}

fn rollback_by_default() -> bool {
    true
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Step {
    command: String,
    #[schemars(extend("additionalProperties" = true))]
    args: serde_json::Map<String, Value>,
}

#[derive(Serialize, JsonSchema)]
pub(super) struct Report {
    ok: bool,
    results: Vec<StepResult>,
    failed: Option<Failure>,
    revision: u64,
}

#[derive(Serialize, JsonSchema)]
#[serde(untagged)]
enum StepResult {
    Success {
        index: usize,
        command: String,
        #[schemars(extend("const" = true))]
        ok: bool,
        result: Value,
    },
    Error {
        #[schemars(extend("const" = false))]
        ok: bool,
        #[serde(flatten)]
        failure: Failure,
    },
}

#[derive(Clone, Serialize, JsonSchema)]
struct Failure {
    index: usize,
    command: String,
    error: String,
}

pub(super) struct Transaction {
    pub journal: workspace::FileJournal,
    pub rollback: bool,
}

impl Engine {
    fn run_step(
        &mut self,
        command: &str,
        mut args: serde_json::Map<String, Value>,
    ) -> Result<Value, String> {
        let spec = registry::lookup(command)?;
        if spec.meta {
            return Err(format!("{command} is not allowed inside a batch"));
        }
        if spec.access == registry::Access::Mutate {
            args.entry("revision")
                .or_insert_with(|| self.revision.into());
        }
        spec.call(registry::Target::Engine(self), Value::Object(args))
    }

    pub(super) fn run_batch(&mut self, request: Request) -> Result<Report, String> {
        if let Some(expected) = request.revision {
            self.check_revision(expected)?;
        }
        let revision = self.revision;
        let checkpoint = request
            .rollback
            .then(|| (self.system.clone(), self.projects.clone()));
        self.batch = Some(Transaction {
            journal: workspace::FileJournal::default(),
            rollback: request.rollback,
        });
        let mut results = Vec::new();
        let mut failed = None;
        for (index, Step { command, args }) in request.steps.into_iter().enumerate() {
            match self.run_step(&command, args) {
                Ok(result) => {
                    if !request.rollback {
                        self.batch.as_mut().unwrap().journal.keep_directories();
                    }
                    results.push(StepResult::Success {
                        index,
                        command,
                        ok: true,
                        result,
                    });
                }
                Err(error) => {
                    let failure = Failure {
                        index,
                        command,
                        error,
                    };
                    results.push(StepResult::Error {
                        ok: false,
                        failure: failure.clone(),
                    });
                    failed = Some(failure);
                    break;
                }
            }
        }
        let transaction = self.batch.take().unwrap();
        if let Some(failure) = &failed {
            if let Some((system, projects)) = checkpoint {
                self.system = system;
                self.projects = projects;
                self.revision = revision;
            }
            transaction.journal.restore().map_err(|e| {
                format!(
                    "Step {} ({}): {}; rollback: {e}",
                    failure.index, failure.command, failure.error
                )
            })?;
        }
        if self.revision != revision {
            self.record_history("batch");
        }
        Ok(Report {
            ok: failed.is_none(),
            results,
            failed,
            revision: self.revision,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace() -> (tempfile::TempDir, SharedWorkspace) {
        let dir = tempfile::tempdir().unwrap();
        let workspace = Workspace::open(dir.path().join("system.json")).unwrap();
        (dir, Arc::new(RwLock::new(workspace)))
    }

    fn get(workspace: &SharedWorkspace, command: &str) -> Value {
        dispatch(workspace, command, json!({}), None).unwrap()
    }

    #[test]
    fn successful_batch_chains_revisions_and_undoes_as_one_edit() {
        let (_dir, w) = workspace();
        let before = get(&w, "system_get");
        let start = before["revision"].as_u64().unwrap();
        let notify: ChangeNotifier = Arc::new(move |revision| assert_eq!(revision, start + 2));
        let report = dispatch(
            &w,
            "batch",
            json!({"revision":start, "steps":[
                {"command":"project_new","args":{}},
                {"command":"bench_template","args":{"name":"polarized_beam_combiner","apply":true}},
                {"command":"bench_get","args":{}}
            ]}),
            Some(&notify),
        )
        .unwrap();
        assert_eq!(report["ok"], true);
        assert_eq!(report["failed"], Value::Null);
        assert_eq!(report["revision"], start + 2);
        let results = report["results"].as_array().unwrap();
        assert_eq!(results.len(), 3);
        assert_eq!(
            results[0],
            json!({"index":0,"command":"project_new","ok":true,
            "result":{"revision":start+1}})
        );
        assert_eq!(results[1]["result"]["revision"], start + 2);
        assert_eq!(results[2]["result"], get(&w, "bench_get"));
        assert_eq!(results[1]["result"]["bench"], results[2]["result"]["bench"]);
        let history = get(&w, "history_list");
        assert_eq!(history["entries"].as_array().unwrap().len(), 2);
        assert_eq!(history["entries"][1]["action"], "batch");
        dispatch(&w, "history_undo", json!({"revision":start+2}), None).unwrap();
        let restored = get(&w, "system_get");
        assert_eq!(restored["system"], before["system"]);
        assert_eq!(restored["revision"], start + 3);
    }

    #[test]
    fn failed_batch_restores_system_projects_history_and_files() {
        let (dir, w) = workspace();
        dispatch(
            &w,
            "project_save_as",
            json!({"path":"original","revision":get(&w,"system_get")["revision"]}),
            None,
        )
        .unwrap();
        let before = get(&w, "system_get");
        let projects = get(&w, "project_list");
        let history = get(&w, "history_list");
        let paths = [
            "system.json",
            "workspace-meta.json",
            "projects/original.hissabeth.json",
        ];
        let bytes = paths.map(|p| std::fs::read(dir.path().join(p)).unwrap());
        let notify: ChangeNotifier = Arc::new(|_| panic!("rolled back batch notified observers"));
        let report = dispatch(
            &w,
            "batch",
            json!({"steps":[
                {"command":"bench_template","args":{"name":"michelson","apply":true}},
                {"command":"project_save_as","args":{"path":"sub/new"}},
                {"command":"project_new","args":{}},
                {"command":"project_open","args":{"path":"original.hissabeth.json"}},
                {"command":"bench_template","args":{"name":"invalid"}},
                {"command":"project_new","args":{}}
            ]}),
            Some(&notify),
        )
        .unwrap();
        assert_eq!(report["ok"], false);
        assert_eq!(report["revision"], before["revision"]);
        assert_eq!(report["results"].as_array().unwrap().len(), 5);
        assert_eq!(
            report["results"][4],
            json!({"index":4,"command":"bench_template",
            "ok":false,"error":"Unknown value at arguments.name"})
        );
        assert_eq!(
            report["failed"],
            json!({"index":4,"command":"bench_template",
            "error":"Unknown value at arguments.name"})
        );
        assert_eq!(get(&w, "system_get"), before);
        assert_eq!(get(&w, "project_list"), projects);
        assert_eq!(get(&w, "history_list"), history);
        assert_eq!(
            paths.map(|p| std::fs::read(dir.path().join(p)).unwrap()),
            bytes
        );
        assert!(!dir.path().join("projects/sub").exists());
    }

    #[test]
    fn partial_batch_keeps_only_the_successful_prefix() {
        let (dir, w) = workspace();
        let start = get(&w, "system_get")["revision"].as_u64().unwrap();
        let report = dispatch(
            &w,
            "batch",
            json!({"rollback":false, "steps":[
                {"command":"project_save_as","args":{"path":"kept"}},
                {"command":"bench_template","args":{"name":"michelson","apply":true}},
                {"command":"bench_trace","args":{"unexpected":1}},
                {"command":"project_new","args":{}}
            ]}),
            None,
        )
        .unwrap();
        assert_eq!(report["failed"]["index"], 2);
        assert_eq!(report["revision"], start + 2);
        assert_eq!(report["results"].as_array().unwrap().len(), 3);
        let saved = get(&w, "system_get");
        assert_eq!(
            saved["system"]["bench"],
            report["results"][1]["result"]["bench"]
        );
        let linked: Value = serde_json::from_slice(
            &std::fs::read(dir.path().join("projects/kept.hissabeth.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(linked, saved["system"]);
        assert_eq!(
            get(&w, "history_list")["entries"].as_array().unwrap().len(),
            2
        );
    }

    #[test]
    fn batch_rejects_nested_and_meta_commands() {
        let (_dir, w) = workspace();
        for command in [
            "batch",
            "history_undo",
            "history_list",
            "workspace_retry",
            "capabilities",
        ] {
            let result = dispatch(
                &w,
                "batch",
                json!({"steps":[{"command":command,"args":{}}]}),
                None,
            )
            .unwrap();
            assert_eq!(
                result["failed"],
                json!({"index":0,"command":command,
                "error":format!("{command} is not allowed inside a batch")})
            );
        }
    }

    #[test]
    fn batch_checks_the_starting_revision_before_any_step() {
        let (dir, w) = workspace();
        let before = get(&w, "system_get");
        let error = dispatch(
            &w,
            "batch",
            json!({"revision":before["revision"].as_u64().unwrap()+1,
            "rollback":false, "steps":[{"command":"project_save_as","args":{"path":"never"}}]}),
            None,
        )
        .unwrap_err();
        assert_eq!(error, "System changed; read it again before editing");
        assert_eq!(get(&w, "system_get"), before);
        assert!(!dir.path().join("projects").exists());
    }
}
