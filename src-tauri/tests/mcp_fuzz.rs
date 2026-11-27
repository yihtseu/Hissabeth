use hissabeth_lib::service::{
    Caller, Scope, SharedWorkspace, Workspace, dispatch, registry::registry, rpc_shared,
};
use serde_json::{Value, json};
use std::sync::{Arc, RwLock};

const STALE: &str = "System changed; read it again before editing";

fn workspace() -> (tempfile::TempDir, SharedWorkspace) {
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::open(dir.path().join("system.json")).unwrap();
    (dir, Arc::new(RwLock::new(workspace)))
}

fn call(workspace: &SharedWorkspace, command: &str, args: Value) -> Value {
    dispatch(&Caller::Gui, workspace, command, args, None).unwrap()
}

fn get(workspace: &SharedWorkspace, command: &str) -> Value {
    call(workspace, command, json!({}))
}

fn alternatives(schema: &Value) -> Option<&[Value]> {
    schema["oneOf"]
        .as_array()
        .or_else(|| schema["anyOf"].as_array())
        .map(Vec::as_slice)
}

fn has_type(schema: &Value, kind: &str) -> bool {
    schema["type"] == kind
        || schema["type"]
            .as_array()
            .is_some_and(|types| types.iter().any(|t| t == kind))
}

fn required_object(schema: &Value, revision: u64) -> Value {
    let fields = schema["required"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|name| {
            let name = name.as_str().unwrap();
            let value = if name == "revision" {
                json!(revision)
            } else {
                example(&schema["properties"][name], revision)
            };
            (name.to_owned(), value)
        });
    Value::Object(fields.collect())
}

fn example(schema: &Value, revision: u64) -> Value {
    if let Some(value) = schema.get("const").or_else(|| schema["enum"].get(0)) {
        return value.clone();
    }
    if let Some(choices) = alternatives(schema) {
        return example(&choices[0], revision);
    }
    if has_type(schema, "object") {
        return required_object(schema, revision);
    }
    if has_type(schema, "array") {
        let count = schema["minItems"].as_u64().unwrap_or(0) as usize;
        return Value::Array(vec![example(&schema["items"], revision); count]);
    }
    if has_type(schema, "integer") {
        return json!(schema["minimum"].as_i64().unwrap_or(0));
    }
    if has_type(schema, "number") {
        let value = schema["exclusiveMinimum"]
            .as_f64()
            .map(f64::next_up)
            .or_else(|| schema["minimum"].as_f64())
            .unwrap_or(0.0);
        return json!(value);
    }
    if has_type(schema, "boolean") {
        return json!(false);
    }
    if has_type(schema, "string") {
        return json!("fixture");
    }
    Value::Null
}

fn mutate(
    schema: &Value,
    field: &str,
    revision: u64,
    replacement: &impl Fn(&Value, &str) -> Option<Value>,
) -> Option<Value> {
    if let Some(value) = replacement(schema, field) {
        return Some(value);
    }
    if let Some(choices) = alternatives(schema) {
        return choices
            .iter()
            .find_map(|choice| mutate(choice, field, revision, replacement));
    }
    if let Some(properties) = schema["properties"].as_object() {
        for (name, child) in properties {
            if let Some(value) = mutate(child, name, revision, replacement) {
                let mut object = required_object(schema, revision);
                object[name] = value;
                return Some(object);
            }
        }
    }
    let item = schema.get("items")?;
    let value = mutate(item, "", revision, replacement)?;
    let count = schema["minItems"].as_u64().unwrap_or(1).max(1) as usize;
    let mut items = vec![example(item, revision); count];
    items[0] = value;
    Some(Value::Array(items))
}

fn outside_range(schema: &Value) -> Option<Value> {
    if has_type(schema, "integer") {
        let bound = schema["maximum"].as_f64()? as u64;
        return bound.checked_add(1).map(|over| json!(over));
    }
    if has_type(schema, "number") {
        return schema["maximum"]
            .as_f64()
            .map(f64::next_up)
            .or_else(|| schema["minimum"].as_f64().map(f64::next_down))
            .or_else(|| schema["exclusiveMinimum"].as_f64())
            .or_else(|| schema["exclusiveMaximum"].as_f64())
            .map(|n| json!(n));
    }
    None
}

fn assert_rejected(
    w: &SharedWorkspace,
    snapshot: &Value,
    command: &str,
    args: Value,
    case: &str,
    expected: &str,
) {
    let request = json!({
        "jsonrpc":"2.0", "id":1, "method":"tools/call",
        "params":{"name":command, "arguments":args}
    });
    let reply = rpc_shared(request, &Caller::Broker(Scope::default()), w, None).unwrap();
    let result = &reply["result"];
    let message = result["content"][0]["text"].as_str().unwrap_or("");
    assert!(
        result["isError"] == true
            && result["content"][0]["type"] == "text"
            && message.contains(expected),
        "{command} ({case}): expected tool error containing {expected:?}, got {reply}"
    );
    assert_eq!(&get(w, "system_get"), snapshot, "{command} ({case})");
}

#[test]
fn registered_tools_reject_malformed_arguments_without_changing_workspace() {
    let (_dir, w) = workspace();
    let snapshot = get(&w, "system_get");
    let revision = snapshot["revision"].as_u64().unwrap();
    for spec in registry() {
        let (name, schema) = (spec.name, &spec.params_schema);
        assert_rejected(
            &w,
            &snapshot,
            name,
            json!(false),
            "argument type",
            "Invalid type",
        );
        let deep = (0..1000).fold(Value::Null, |value, _| Value::Array(vec![value]));
        let error = dispatch(&Caller::Gui, &w, name, deep, None).unwrap_err();
        assert_eq!(error, "Invalid type at arguments", "{name} (deep nesting)");
        assert_eq!(get(&w, "system_get"), snapshot, "{name} (deep nesting)");
        let check =
            |case, args, expected| assert_rejected(&w, &snapshot, name, args, case, expected);
        if let Some(args) = mutate(schema, "", revision, &|s, _| outside_range(s)) {
            check("range", args, "Value outside range at arguments.");
        }
        for token in ["NaN", "Infinity", "-Infinity"] {
            if let Some(args) = mutate(schema, "", revision, &|s, _| {
                (has_type(s, "number") || has_type(s, "integer")).then(|| json!(token))
            }) {
                check(token, args, "Invalid type at arguments.");
            }
        }
        if let Some(args) = mutate(schema, "", revision, &|s, _| {
            has_type(s, "array").then(|| {
                let limit = s["maxItems"].as_u64().unwrap_or(100_000) as usize;
                Value::Array(vec![Value::Null; limit + 1])
            })
        }) {
            check(
                "oversized array",
                args,
                "Invalid array length at arguments.",
            );
        }
        if let Some(args) = mutate(schema, "", revision, &|_, field| {
            (field == "path").then(|| json!(""))
        }) {
            check("empty path", args, "Project path is required");
        }
        if schema["properties"].get("revision").is_some() {
            let mut args = required_object(schema, revision);
            args["revision"] = json!(revision + 1);
            if schema["properties"].get("apply").is_some() {
                args["apply"] = json!(true);
            }
            let expected = if matches!(name, "workspace_retry" | "workspace_recover") {
                "Workspace is ready; recovery is not active"
            } else {
                STALE
            };
            check("future revision", args, expected);
        }
    }
}

#[test]
fn malformed_batch_rolls_back_without_reissuing_discarded_revisions() {
    let (dir, w) = workspace();
    let start = get(&w, "system_get")["revision"].as_u64().unwrap();
    let opened = call(
        &w,
        "project_save_as",
        json!({"revision":start, "path":"original"}),
    );
    let before = get(&w, "system_get");
    let revision = opened["revision"].as_u64().unwrap();
    let projects = get(&w, "project_list");
    let history = get(&w, "history_list");
    let files = [
        "system.json",
        "workspace-meta.json",
        "projects/original.hissabeth.json",
    ];
    let saved = files.map(|p| std::fs::read(dir.path().join(p)).unwrap());

    let report = call(
        &w,
        "batch",
        json!({"revision":revision, "steps":[
            {"command":"project_new","args":{}},
            {"command":"project_save_as","args":{"path":"discarded/new"}},
            {"command":"paraxial","args":{"wavelength":"Infinity"}},
            {"command":"project_new","args":{}}
        ]}),
    );
    assert_eq!(report["ok"], false);
    assert_eq!(
        report["failed"],
        json!({
            "index":2, "command":"paraxial", "error":"Invalid type at arguments.wavelength"
        })
    );
    let results = report["results"].as_array().unwrap();
    assert_eq!(results.len(), 3);
    assert_eq!(results[0]["result"]["revision"], revision + 1);
    assert_eq!(results[1]["result"]["revision"], revision + 2);
    assert_eq!(report["revision"], revision);
    assert_eq!(get(&w, "system_get"), before);
    assert_eq!(get(&w, "project_list"), projects);
    assert_eq!(get(&w, "history_list"), history);
    assert_eq!(
        files.map(|p| std::fs::read(dir.path().join(p)).unwrap()),
        saved
    );
    assert!(!dir.path().join("projects/discarded").exists());

    let after = call(
        &w,
        "batch",
        json!({"revision":revision, "rollback":false, "steps":[
            {"command":"project_new","args":{}},
            {"command":"paraxial","args":{"wavelength":"NaN"}}
        ]}),
    );
    assert_eq!(after["failed"]["index"], 1);
    assert_eq!(after["revision"], revision + 3);
    assert_eq!(after["results"][0]["result"]["revision"], after["revision"]);
    let kept = get(&w, "system_get");
    assert_eq!(kept["revision"], after["revision"]);
    assert_eq!(kept["system"]["name"], "Untitled system");
    assert_rejected(
        &w,
        &kept,
        "project_new",
        json!({"revision":revision+1}),
        "discarded revision",
        STALE,
    );
    let undone = call(&w, "history_undo", json!({"revision":revision+3}));
    assert_eq!(undone["revision"], revision + 4);
    assert_eq!(get(&w, "system_get")["system"], before["system"]);
}
