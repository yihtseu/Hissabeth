//! Session artifacts: deliverables generated on a background thread and kept
//! in memory until fetched, exported or evicted.
//!
//! Storage policy: one artifact may hold at most [`ITEM_LIMIT`] bytes; a
//! larger result fails its job. All finished artifacts together hold at most
//! [`TOTAL_LIMIT`] bytes; when a new one would exceed that, the oldest
//! finished artifacts are evicted until it fits. At most [`RECORD_LIMIT`] job
//! records are kept, again dropping the oldest finished ones first. At most
//! [`ACTIVE_LIMIT`] generator threads run at once, counting cancelled jobs
//! whose thread has not returned yet; a start beyond that is refused. Nothing
//! is persisted.
use schemars::JsonSchema;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

pub(super) const ITEM_LIMIT: usize = 16 << 20;
pub(super) const TOTAL_LIMIT: usize = 64 << 20;
const RECORD_LIMIT: usize = 256;
pub(super) const ACTIVE_LIMIT: usize = 4;

pub(super) type Shared = Arc<Mutex<Artifacts>>;

/// What a generator produces: a file name, its MIME type and its bytes.
pub(super) struct Output {
    pub filename: String,
    pub mime: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum State {
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
}

struct Job {
    kind: String,
    format: String,
    revision: u64,
    saved: bool,
    state: State,
    error: Option<String>,
    output: Option<Output>,
    /// Order of completion; finished jobs are dropped oldest first.
    finished: u64,
}

pub(super) struct Artifacts {
    next: u64,
    clock: u64,
    jobs: BTreeMap<u64, Job>,
    item_limit: usize,
    total_limit: usize,
    /// Generator threads that have not returned, cancelled or not.
    active: usize,
    active_limit: usize,
}

impl Default for Artifacts {
    fn default() -> Self {
        Self::with_limits(ITEM_LIMIT, TOTAL_LIMIT)
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct Status {
    id: String,
    kind: String,
    format: String,
    state: State,
    /// The project revision the artifact was generated from.
    revision: u64,
    /// False when it was generated from an unsaved draft.
    saved: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    progress: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    filename: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mime: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bytes: Option<usize>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct Started {
    id: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct Fetched {
    id: String,
    filename: String,
    mime: String,
    bytes: usize,
    revision: u64,
    saved: bool,
    /// Always "base64".
    encoding: &'static str,
    data: String,
}

#[derive(Serialize, JsonSchema)]
pub(super) struct Exported {
    id: String,
    path: String,
    bytes: usize,
}

fn key(id: &str) -> Result<u64, String> {
    id.strip_prefix("artifact-")
        .and_then(|n| n.parse().ok())
        .ok_or_else(|| format!("{id:?} is not an artifact id; artifact_start returns ids like \"artifact-1\""))
}

impl Artifacts {
    pub(super) fn with_limits(item_limit: usize, total_limit: usize) -> Self {
        Self { next: 1, clock: 0, jobs: BTreeMap::new(), item_limit, total_limit, active: 0, active_limit: ACTIVE_LIMIT }
    }

    fn job(&self, id: &str) -> Result<&Job, String> {
        self.jobs.get(&key(id)?).ok_or_else(|| self.unknown(id))
    }

    fn unknown(&self, id: &str) -> String {
        format!(
            "Unknown artifact {id}. Artifacts live only for this session; the oldest finished ones are evicted once they exceed {} MB in total or {RECORD_LIMIT} jobs.",
            self.total_limit >> 20
        )
    }

    pub(super) fn status(&self, id: &str) -> Result<Status, String> {
        let job = self.job(id)?;
        let output = job.output.as_ref();
        Ok(Status {
            id: id.to_owned(),
            kind: job.kind.clone(),
            format: job.format.clone(),
            state: job.state,
            revision: job.revision,
            saved: job.saved,
            progress: (job.state == State::Done).then_some(1.0),
            error: job.error.clone(),
            filename: output.map(|o| o.filename.clone()),
            mime: output.map(|o| o.mime.clone()),
            bytes: output.map(|o| o.bytes.len()),
        })
    }

    fn done(&self, id: &str) -> Result<(&Job, &Output), String> {
        let job = self.job(id)?;
        match (&job.output, job.state) {
            (Some(output), State::Done) => Ok((job, output)),
            (_, state) => Err(format!(
                "Artifact {id} is {}; fetch or export it once artifact_status reports done",
                serde_json::to_value(state).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default()
            )),
        }
    }

    pub(super) fn fetch(&self, id: &str) -> Result<Fetched, String> {
        let (job, output) = self.done(id)?;
        Ok(Fetched {
            id: id.to_owned(),
            filename: output.filename.clone(),
            mime: output.mime.clone(),
            bytes: output.bytes.len(),
            revision: job.revision,
            saved: job.saved,
            encoding: "base64",
            data: base64(&output.bytes),
        })
    }

    /// Writes a finished artifact to `path` through a temporary file in the
    /// same directory, so a reader never sees a partial file.
    pub(super) fn export(&self, id: &str, path: &Path) -> Result<Exported, String> {
        let (_, output) = self.done(id)?;
        let wanted = Path::new(&output.filename).extension().unwrap_or_default();
        if !path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case(wanted)) {
            return Err(format!(
                "Export path {} must end in .{} to match artifact {id} ({})",
                path.display(),
                wanted.to_string_lossy(),
                output.filename
            ));
        }
        let dir = path.parent().ok_or("Export path has no parent directory")?;
        let mut file = tempfile::NamedTempFile::new_in(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        std::io::Write::write_all(&mut file, &output.bytes).map_err(|e| e.to_string())?;
        file.persist(path).map_err(|e| format!("{}: {}", path.display(), e.error))?;
        Ok(Exported { id: id.to_owned(), path: path.display().to_string(), bytes: output.bytes.len() })
    }

    pub(super) fn cancel(&mut self, id: &str) -> Result<Status, String> {
        let number = key(id)?;
        let unknown = self.unknown(id);
        let job = self.jobs.get_mut(&number).ok_or(unknown)?;
        match job.state {
            State::Queued | State::Running => {
                job.state = State::Cancelled;
                self.clock += 1;
                job.finished = self.clock;
            }
            _ => return Err(format!("Artifact {id} has already finished; only queued or running artifacts can be cancelled")),
        }
        self.status(id)
    }

    fn insert(&mut self, kind: &str, format: &str, revision: u64, saved: bool) -> Result<u64, String> {
        if self.active >= self.active_limit {
            return Err(format!(
                "{} artifacts are already being generated; wait for one to finish (cancelled ones still count until their generator returns)",
                self.active
            ));
        }
        self.active += 1;
        let number = self.next;
        self.next += 1;
        self.jobs.insert(
            number,
            Job {
                kind: kind.to_owned(),
                format: format.to_owned(),
                revision,
                saved,
                state: State::Queued,
                error: None,
                output: None,
                finished: 0,
            },
        );
        while self.jobs.len() > RECORD_LIMIT {
            let Some(oldest) = self.oldest_finished(|_| true) else { break };
            self.jobs.remove(&oldest);
        }
        Ok(number)
    }

    fn oldest_finished(&self, keep: impl Fn(&Job) -> bool) -> Option<u64> {
        self.jobs
            .iter()
            .filter(|(_, job)| matches!(job.state, State::Done | State::Failed | State::Cancelled) && keep(job))
            .min_by_key(|(_, job)| job.finished)
            .map(|(number, _)| *number)
    }

    fn finish(&mut self, number: u64, result: Result<Output, String>) {
        let item_limit = self.item_limit;
        let Some(job) = self.jobs.get_mut(&number) else { return };
        if job.state == State::Cancelled {
            return;
        }
        self.clock += 1;
        job.finished = self.clock;
        match result {
            Ok(output) if output.bytes.len() > item_limit => {
                job.state = State::Failed;
                job.error = Some(format!(
                    "The {} artifact is {} bytes, over the {} MB limit for one artifact",
                    job.format,
                    output.bytes.len(),
                    item_limit >> 20
                ));
            }
            Ok(output) => {
                job.state = State::Done;
                job.output = Some(output);
                while self.held() > self.total_limit {
                    let Some(oldest) = self.oldest_finished(|job| job.output.is_some() && job.finished != self.clock) else {
                        break;
                    };
                    self.jobs.remove(&oldest);
                }
            }
            Err(error) => {
                job.state = State::Failed;
                job.error = Some(error);
            }
        }
    }

    fn held(&self) -> usize {
        self.jobs.values().filter_map(|job| job.output.as_ref()).map(|o| o.bytes.len()).sum()
    }
}

/// Queues a job and runs `work` on its own thread; its result is discarded if
/// the job was cancelled meanwhile.
pub(super) fn start(
    store: &Shared,
    kind: &str,
    format: &str,
    revision: u64,
    saved: bool,
    work: impl FnOnce() -> Result<Output, String> + Send + 'static,
) -> Result<Started, String> {
    let number = store.lock().map_err(|e| e.to_string())?.insert(kind, format, revision, saved)?;
    spawn(store, number, work)?;
    Ok(Started { id: format!("artifact-{number}") })
}

/// Runs an inserted job's generator unless it was cancelled while queued.
fn spawn(
    store: &Shared,
    number: u64,
    work: impl FnOnce() -> Result<Output, String> + Send + 'static,
) -> Result<(), String> {
    let shared = store.clone();
    let spawned = std::thread::Builder::new().name(format!("artifact-{number}")).spawn(move || {
        let run = shared.lock().is_ok_and(|mut artifacts| match artifacts.jobs.get_mut(&number) {
            Some(job) if job.state == State::Queued => {
                job.state = State::Running;
                true
            }
            _ => false,
        });
        let result = run.then(|| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(work))
                .unwrap_or_else(|_| Err("The artifact generator failed unexpectedly".into()))
        });
        if let Ok(mut artifacts) = shared.lock() {
            artifacts.active -= 1;
            if let Some(result) = result {
                artifacts.finish(number, result);
            }
        }
    });
    if let Err(e) = spawned {
        let mut artifacts = store.lock().map_err(|e| e.to_string())?;
        artifacts.active -= 1;
        artifacts.finish(number, Err(format!("Could not start the artifact generator: {e}")));
        return Err(e.to_string());
    }
    Ok(())
}

pub(super) use crate::optics::blueprint_render::base64;

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// Polls until the job leaves the queued and running states.
    pub(in crate::service) fn settle(store: &Shared, id: &str) -> Status {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let status = store.lock().unwrap().status(id).unwrap();
            if !matches!(status.state, State::Queued | State::Running) || Instant::now() > deadline {
                return status;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn output(bytes: usize) -> Output {
        Output { filename: "a.bin".into(), mime: "application/octet-stream".into(), bytes: vec![7; bytes] }
    }


    #[test]
    fn finished_artifacts_are_evicted_oldest_first_past_the_total_limit() {
        let store: Shared = Arc::new(Mutex::new(Artifacts::with_limits(40, 100)));
        let ids: Vec<_> = (0..3)
            .map(|_| {
                let id = start(&store, "test", "bin", 1, true, || Ok(output(40))).unwrap().id;
                assert_eq!(settle(&store, &id).state, State::Done);
                id
            })
            .collect();
        let artifacts = store.lock().unwrap();
        let error = artifacts.status(&ids[0]).err().unwrap();
        assert!(error.contains("evicted") && error.contains(&ids[0]), "{error}");
        assert_eq!(artifacts.status(&ids[1]).unwrap().bytes, Some(40));
        assert_eq!(artifacts.fetch(&ids[2]).unwrap().data, base64(&[7; 40]));
        assert_eq!(artifacts.held(), 80);
    }

    #[test]
    fn an_artifact_over_the_item_limit_fails() {
        let store: Shared = Arc::new(Mutex::new(Artifacts::with_limits(40, 100)));
        let id = start(&store, "test", "bin", 1, true, || Ok(output(41))).unwrap().id;
        let status = settle(&store, &id);
        assert_eq!(status.state, State::Failed);
        assert!(status.error.unwrap().contains("41 bytes"));
        assert!(store.lock().unwrap().fetch(&id).unwrap_err().contains("is failed"));
    }

    /// Waits until every generator thread has returned.
    fn drain(store: &Shared) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while store.lock().unwrap().active > 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(store.lock().unwrap().active, 0);
    }

    #[test]
    fn a_cancelled_running_artifact_discards_its_result() {
        let store: Shared = Arc::default();
        let (entered, running) = std::sync::mpsc::channel::<()>();
        let (release, gate) = std::sync::mpsc::channel::<()>();
        let id = start(&store, "test", "bin", 1, true, move || {
            entered.send(()).unwrap();
            gate.recv().unwrap();
            Ok(output(3))
        })
        .unwrap()
        .id;
        running.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(store.lock().unwrap().status(&id).unwrap().state, State::Running);
        assert_eq!(store.lock().unwrap().cancel(&id).unwrap().state, State::Cancelled);
        release.send(()).unwrap();
        drain(&store);
        let artifacts = store.lock().unwrap();
        assert_eq!(artifacts.status(&id).unwrap().state, State::Cancelled);
        assert_eq!(artifacts.held(), 0);
        assert!(artifacts.fetch(&id).unwrap_err().contains("is cancelled"));
        drop(artifacts);
        assert!(store.lock().unwrap().cancel(&id).unwrap_err().contains("already finished"));
    }

    #[test]
    fn a_cancelled_queued_artifact_never_runs() {
        let store: Shared = Arc::default();
        let number = store.lock().unwrap().insert("test", "bin", 1, true).unwrap();
        let id = format!("artifact-{number}");
        assert_eq!(store.lock().unwrap().cancel(&id).unwrap().state, State::Cancelled);
        let ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = ran.clone();
        spawn(&store, number, move || {
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(output(1))
        })
        .unwrap();
        drain(&store);
        assert!(!ran.load(std::sync::atomic::Ordering::SeqCst));
        assert_eq!(store.lock().unwrap().status(&id).unwrap().state, State::Cancelled);
    }

    #[test]
    fn starts_beyond_the_active_limit_are_refused_until_a_generator_returns() {
        let store: Shared = Arc::default();
        let (entered, running) = std::sync::mpsc::channel::<()>();
        let (release, gate) = std::sync::mpsc::channel::<()>();
        let gate = Arc::new(Mutex::new(gate));
        let ids: Vec<_> = (0..ACTIVE_LIMIT)
            .map(|_| {
                let (gate, entered) = (gate.clone(), entered.clone());
                start(&store, "test", "bin", 1, true, move || {
                    entered.send(()).unwrap();
                    gate.lock().unwrap().recv().unwrap();
                    Ok(output(1))
                })
                .unwrap()
                .id
            })
            .collect();
        for _ in 0..ACTIVE_LIMIT {
            running.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        // Cancelling does not free a slot while its generator is still running.
        assert_eq!(store.lock().unwrap().status(&ids[0]).unwrap().state, State::Running);
        store.lock().unwrap().cancel(&ids[0]).unwrap();
        let error = start(&store, "test", "bin", 1, true, || Ok(output(1))).unwrap_err();
        assert!(error.contains("already being generated"), "{error}");
        for _ in 0..ACTIVE_LIMIT {
            release.send(()).unwrap();
        }
        drain(&store);
        let id = start(&store, "test", "bin", 1, true, || Ok(output(1))).unwrap().id;
        assert_eq!(settle(&store, &id).state, State::Done);
        // Finished records stay within the record limit.
        for _ in 0..RECORD_LIMIT + 10 {
            let id = start(&store, "test", "bin", 1, true, || Ok(output(1))).unwrap().id;
            settle(&store, &id);
            drain(&store);
        }
        assert!(store.lock().unwrap().jobs.len() <= RECORD_LIMIT);
    }

    #[test]
    fn a_failing_generator_reports_its_error() {
        let store: Shared = Arc::default();
        let id = start(&store, "test", "bin", 1, true, || Err("no stages".into())).unwrap().id;
        let status = settle(&store, &id);
        assert_eq!((status.state, status.error.as_deref()), (State::Failed, Some("no stages")));
        assert!(store.lock().unwrap().status("artifact-x").unwrap_err().contains("not an artifact id"));
    }

    mod flow {
        use super::*;
        use crate::service::{Caller, Scope, SharedWorkspace, Workspace, dispatch, rpc_shared};
        use serde_json::{Value, json};
        use std::sync::RwLock;

        fn workspace() -> (tempfile::TempDir, SharedWorkspace) {
            let dir = tempfile::tempdir().unwrap();
            std::fs::create_dir(dir.path().join("data")).unwrap();
            let workspace = Workspace::open(dir.path().join("data/system.json")).unwrap();
            (dir, Arc::new(RwLock::new(workspace)))
        }

        /// A tools/call over the broker for a default-scoped agent; returns
        /// the MCP result.
        fn mcp(w: &SharedWorkspace, name: &str, arguments: Value) -> Value {
            let request = json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":arguments}});
            rpc_shared(request, &Caller::Broker(Scope::default()), w, None).unwrap()["result"].clone()
        }

        fn ok(result: &Value) -> Value {
            assert_eq!(result["isError"], false, "{result}");
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
        }

        fn wait(w: &SharedWorkspace, id: &str) -> Value {
            let store = w.read().unwrap().artifacts();
            settle(&store, id);
            ok(&mcp(w, "artifact_status", json!({"id": id})))
        }

        fn michelson(w: &SharedWorkspace) {
            let revision = dispatch(&Caller::Gui, w, "system_get", json!({}), None).unwrap()["revision"].clone();
            dispatch(&Caller::Gui, w, "bench_template", json!({"name":"michelson","apply":true,"revision":revision}), None).unwrap();
        }

        /// A valid RGBA PNG with stored (uncompressed) deflate blocks.
        fn png(width: u32, height: u32, rgba: [u8; 4]) -> Vec<u8> {
            fn crc(data: &[u8]) -> u32 {
                let mut c = !0u32;
                for &b in data {
                    c ^= u32::from(b);
                    for _ in 0..8 {
                        c = if c & 1 == 1 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
                    }
                }
                !c
            }
            fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
                out.extend((data.len() as u32).to_be_bytes());
                let start = out.len();
                out.extend(kind);
                out.extend(data);
                let sum = crc(&out[start..]);
                out.extend(sum.to_be_bytes());
            }
            // Each scanline is filter type 0 followed by its pixels.
            let row = [&[0][..], &rgba.repeat(width as usize)].concat();
            let raw = row.repeat(height as usize);
            let mut zlib = vec![0x78, 0x01];
            let blocks: Vec<_> = raw.chunks(65_535).collect();
            for (i, block) in blocks.iter().enumerate() {
                zlib.push(u8::from(i + 1 == blocks.len()));
                let len = block.len() as u16;
                zlib.extend(len.to_le_bytes());
                zlib.extend((!len).to_le_bytes());
                zlib.extend(*block);
            }
            let (mut a, mut b) = (1u32, 0u32);
            for &byte in &raw {
                a = (a + u32::from(byte)) % 65_521;
                b = (b + a) % 65_521;
            }
            zlib.extend((b << 16 | a).to_be_bytes());
            let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
            let mut header = width.to_be_bytes().to_vec();
            header.extend(height.to_be_bytes());
            header.extend([8, 6, 0, 0, 0]);
            chunk(&mut out, b"IHDR", &header);
            chunk(&mut out, b"IDAT", &zlib);
            chunk(&mut out, b"IEND", &[]);
            out
        }

        #[test]
        fn start_status_fetch_over_the_broker() {
            let (_dir, w) = workspace();
            michelson(&w);
            let started = ok(&mcp(&w, "artifact_start", json!({"kind":"blueprint","format":"svg"})));
            let id = started["id"].as_str().unwrap();
            let status = wait(&w, id);
            assert_eq!((status["state"].as_str(), status["mime"].as_str()), (Some("done"), Some("image/svg+xml")), "{status}");
            let fetched = mcp(&w, "artifact_fetch", json!({"id": id}));
            let meta = ok(&fetched);
            assert_eq!(meta["filename"], status["filename"]);
            assert!(meta.get("data").is_none(), "the bytes travel in the second content item only");
            let resource = &fetched["content"][1];
            assert_eq!(resource["type"], "resource");
            assert_eq!(resource["resource"]["mimeType"], "image/svg+xml");
            assert_eq!(resource["resource"]["uri"], format!("artifact://{id}/{}", status["filename"].as_str().unwrap()));
            // The artifact equals the synchronous export of the same revision.
            let direct = dispatch(&Caller::Gui, &w, "bench_export_blueprint", json!({"format":"svg"}), None).unwrap();
            assert_eq!(resource["resource"]["text"], direct["content"]);
            assert_eq!(status["bytes"], direct["content"].as_str().unwrap().len());
            assert_eq!(status["revision"], direct["revision"]);
            // A draft is generated from the draft, and marked unsaved.
            let mut draft = dispatch(&Caller::Gui, &w, "bench_get", json!({}), None).unwrap()["bench"].clone();
            draft["name"] = json!("Draft bench");
            let id = ok(&mcp(&w, "artifact_start", json!({"kind":"blueprint","format":"alignment_md","params":{"bench":draft}})))["id"].clone();
            let status = wait(&w, id.as_str().unwrap());
            assert_eq!((status["state"].as_str(), status["saved"].as_bool()), (Some("done"), Some(false)), "{status}");
            // Errors are reported by the job; ids are checked.
            let empty = workspace();
            let error = mcp(&empty.1, "artifact_start", json!({"kind":"blueprint","format":"svg"}));
            assert_eq!(error["isError"], true);
            assert!(mcp(&w, "artifact_fetch", json!({"id":"artifact-999"}))["content"][0]["text"].as_str().unwrap().contains("Unknown artifact"));
            // Artifacts belong to the session, not to batches.
            let batch = dispatch(&Caller::Gui, &w, "batch", json!({"steps":[{"command":"artifact_status","args":{"id":"artifact-1"}}]}), None).unwrap();
            assert!(batch["failed"]["error"].as_str().unwrap().contains("not allowed inside a batch"), "{batch}");
        }

        #[test]
        fn blueprint_png_and_pdf_reach_the_app_and_mcp_with_filename_and_type() {
            let (_dir, w) = workspace();
            michelson(&w);
            // PNG: MCP image content equal to the direct export of the same revision.
            let id = ok(&mcp(&w, "artifact_start", json!({"kind":"blueprint","format":"png","params":{"px_per_mm":2}})))["id"].clone();
            let status = wait(&w, id.as_str().unwrap());
            assert_eq!((status["state"].as_str(), status["mime"].as_str(), status["filename"].as_str()), (Some("done"), Some("image/png"), Some("hissabeth-bench.png")), "{status}");
            let fetched = mcp(&w, "artifact_fetch", json!({"id": id}));
            let image = &fetched["content"][1];
            assert_eq!((image["type"].as_str(), image["mimeType"].as_str()), (Some("image"), Some("image/png")));
            let direct = dispatch(&Caller::Gui, &w, "bench_export_blueprint", json!({"format":"png","px_per_mm":2}), None).unwrap();
            assert_eq!((direct["encoding"].as_str(), direct["mime"].as_str(), direct["filename"].as_str()), (Some("base64"), Some("image/png"), Some("hissabeth-bench.png")));
            assert_eq!(image["data"], direct["content"]);
            assert!(direct["content"].as_str().unwrap().starts_with("iVBORw0KGgo"), "PNG signature");
            // PDF: an embedded base64 blob resource for MCP, base64 data for the app.
            let id = ok(&mcp(&w, "artifact_start", json!({"kind":"blueprint","format":"pdf"})))["id"].clone();
            let status = wait(&w, id.as_str().unwrap());
            assert_eq!((status["mime"].as_str(), status["filename"].as_str()), (Some("application/pdf"), Some("hissabeth-bench.pdf")), "{status}");
            let resource = &mcp(&w, "artifact_fetch", json!({"id": id}))["content"][1]["resource"];
            assert_eq!((resource["mimeType"].as_str(), resource.get("text")), (Some("application/pdf"), None));
            assert!(resource["blob"].as_str().unwrap().starts_with("JVBERi0"), "%PDF- header");
            let app = dispatch(&Caller::Gui, &w, "artifact_fetch", json!({"id": id}), None).unwrap();
            assert_eq!((app["encoding"].as_str(), &app["data"]), (Some("base64"), &resource["blob"]));
            // Text formats keep plain content; px_per_mm is only for png.
            let svg = dispatch(&Caller::Gui, &w, "bench_export_blueprint", json!({"format":"svg"}), None).unwrap();
            assert!(svg.get("encoding").is_none());
            let refused = dispatch(&Caller::Gui, &w, "bench_export_blueprint", json!({"format":"pdf","px_per_mm":2}), None).unwrap_err();
            assert!(refused.contains("px_per_mm applies only to png"), "{refused}");
            let outside = mcp(&w, "artifact_start", json!({"kind":"blueprint","format":"png","params":{"px_per_mm":400}}));
            assert!(outside["content"][0]["text"].as_str().unwrap().contains("Value outside range at arguments.params.px_per_mm"), "{outside}");
            // In range but too many pixels: the job fails and says why.
            let id = ok(&mcp(&w, "artifact_start", json!({"kind":"blueprint","format":"png","params":{"px_per_mm":40}})))["id"].clone();
            let status = wait(&w, id.as_str().unwrap());
            assert_eq!(status["state"], "failed");
            assert!(status["error"].as_str().unwrap().contains("MP limit; lower px_per_mm"), "{status}");
        }

        #[test]
        fn layout3d_reaches_mcp_as_svg_and_as_a_png_artifact() {
            let (_dir, w) = workspace();
            michelson(&w);
            let args = json!({"format":"layout3d","view":{"azimuth_deg":-25,"elevation_deg":35},"rays_per_source":19,"color_by":"wavelength"});
            let svg = ok(&mcp(&w, "bench_export_blueprint", args.clone()));
            assert_eq!((svg["filename"].as_str(), svg["mime"].as_str()), (Some("hissabeth-layout3d.svg"), Some("image/svg+xml")), "{svg}");
            let content = svg["content"].as_str().unwrap();
            assert!(content.contains(r#"width="420mm""#) && content.contains("azimuth -25° · elevation 35° · 19 rays per source"), "{content}");
            assert_eq!(dispatch(&Caller::Gui, &w, "bench_export_blueprint", args, None).unwrap()["content"], svg["content"]);
            // Without options, layout3d uses the iso preset.
            let iso = dispatch(&Caller::Gui, &w, "bench_export_blueprint", json!({"format":"layout3d"}), None).unwrap();
            assert!(iso["content"].as_str().unwrap().contains("azimuth -45°"));
            let top = ok(&mcp(&w, "bench_export_blueprint", json!({"format":"layout3d","preset":"top","rays_per_source":6})));
            assert!(top["content"].as_str().unwrap().contains("azimuth 0° · elevation 90° · 6 rays per source"), "{top}");
            // layout3d true asks for the default 3-D layout as pdf; without it pdf is the plan.
            let pdf = dispatch(&Caller::Gui, &w, "bench_export_blueprint", json!({"format":"pdf","layout3d":true}), None).unwrap();
            assert_eq!(pdf["filename"], "hissabeth-layout3d.pdf");
            let plan = dispatch(&Caller::Gui, &w, "bench_export_blueprint", json!({"format":"pdf"}), None).unwrap();
            assert_eq!(plan["filename"], "hissabeth-bench.pdf");
            let id = ok(&mcp(&w, "artifact_start", json!({"kind":"blueprint","format":"png","params":{"px_per_mm":1,"preset":"top","background":"white"}})))["id"].clone();
            let status = wait(&w, id.as_str().unwrap());
            assert_eq!((status["state"].as_str(), status["filename"].as_str()), (Some("done"), Some("hissabeth-layout3d.png")), "{status}");
            assert_eq!(mcp(&w, "artifact_fetch", json!({"id": id}))["content"][1]["type"], "image");
            // Options are checked by the schema and refused for plan-only formats.
            for bad in [json!({"format":"layout3d","preset":"oblique"}), json!({"format":"layout3d","colour":"red"})] {
                let unknown = mcp(&w, "bench_export_blueprint", bad);
                assert_eq!(unknown["isError"], true, "{unknown}");
            }
            let refused = dispatch(&Caller::Gui, &w, "bench_export_blueprint", json!({"format":"dxf","preset":"top"}), None).unwrap_err();
            assert!(refused.contains("layout3d options apply only to layout3d, pdf and png"), "{refused}");
        }

        #[test]
        fn png_artifacts_arrive_as_mcp_image_content() {
            let (_dir, w) = workspace();
            let bytes = png(3, 2, [255, 0, 0, 255]);
            let store = w.read().unwrap().artifacts();
            let expected = bytes.clone();
            let id = start(&store, "blueprint", "png", 0, true, move || {
                Ok(Output { filename: "bench.png".into(), mime: "image/png".into(), bytes })
            })
            .unwrap()
            .id;
            settle(&store, &id);
            let fetched = mcp(&w, "artifact_fetch", json!({"id": id}));
            let image = &fetched["content"][1];
            assert_eq!(image, &json!({"type":"image","mimeType":"image/png","data":base64(&expected)}));
            assert_eq!(ok(&fetched)["bytes"], expected.len());
            // Binary data that is not an image is a base64 blob resource.
            let id = start(&store, "blueprint", "bin", 0, true, || {
                Ok(Output { filename: "a.bin".into(), mime: "application/octet-stream".into(), bytes: vec![0, 159, 146, 150] })
            })
            .unwrap()
            .id;
            settle(&store, &id);
            let resource = &mcp(&w, "artifact_fetch", json!({"id": id}))["content"][1]["resource"];
            assert_eq!((resource["blob"].as_str(), resource.get("text")), (Some("AJ+Slg=="), None));
        }

        #[test]
        fn export_is_held_to_the_file_scope() {
            let (dir, w) = workspace();
            michelson(&w);
            let id = ok(&mcp(&w, "artifact_start", json!({"kind":"blueprint","format":"dxf"})))["id"].as_str().unwrap().to_owned();
            assert_eq!(wait(&w, &id)["state"], "done");
            let outside = dir.path().join("outside");
            std::fs::create_dir_all(&outside).unwrap();
            let denied = mcp(&w, "artifact_export", json!({"id": id, "path": outside.join("bench.dxf")}));
            assert!(denied["content"][0]["text"].as_str().unwrap().contains("File access denied"), "{denied}");
            assert!(!outside.join("bench.dxf").exists());
            // Inside the data directory, relative to its projects folder.
            let exported = ok(&mcp(&w, "artifact_export", json!({"id": id, "path": "bench.dxf"})));
            let written = std::fs::read(dir.path().join("data/projects/bench.dxf")).unwrap();
            assert_eq!(exported["bytes"], written.len());
            let fetched = w.read().unwrap().artifacts().lock().unwrap().fetch(&id).unwrap();
            assert_eq!(base64(&written), fetched.data);
            // The path needs an extension, and an existing parent.
            let error = mcp(&w, "artifact_export", json!({"id": id, "path": "bench"}));
            assert!(error["content"][0]["text"].as_str().unwrap().contains("extension"), "{error}");
            let error = mcp(&w, "artifact_export", json!({"id": id, "path": "missing/bench.dxf"}));
            assert!(error["content"][0]["text"].as_str().unwrap().contains("does not exist"), "{error}");
            // The workspace's own system.json is the open project.
            let project = std::fs::read(dir.path().join("data/system.json")).unwrap();
            let error = mcp(&w, "artifact_export", json!({"id": id, "path": "../system.json"}));
            assert!(error["content"][0]["text"].as_str().unwrap().contains("open project"), "{error}");
            // Any other path must keep the artifact's extension.
            let error = mcp(&w, "artifact_export", json!({"id": id, "path": "other.json"}));
            assert!(error["content"][0]["text"].as_str().unwrap().contains("must end in .dxf"), "{error}");
            assert_eq!(std::fs::read(dir.path().join("data/system.json")).unwrap(), project);
            // A project saved under the artifact's own extension is protected
            // by its path.
            let revision = w.read().unwrap().engine().unwrap().revision;
            ok(&mcp(&w, "project_save_as", json!({"path": "design.dxf", "revision": revision})));
            let saved = std::fs::read(dir.path().join("data/projects/design.dxf")).unwrap();
            let error = mcp(&w, "artifact_export", json!({"id": id, "path": "design.dxf"}));
            assert!(error["content"][0]["text"].as_str().unwrap().contains("open project"), "{error}");
            assert_eq!(std::fs::read(dir.path().join("data/projects/design.dxf")).unwrap(), saved);
            // So is any other name for the same file, such as a hard link (or a
            // different letter case on a case-insensitive volume).
            std::fs::hard_link(dir.path().join("data/projects/design.dxf"), dir.path().join("data/projects/alias.dxf")).unwrap();
            let error = mcp(&w, "artifact_export", json!({"id": id, "path": "alias.dxf"}));
            assert!(error["content"][0]["text"].as_str().unwrap().contains("open project"), "{error}");
            assert_eq!(std::fs::read(dir.path().join("data/projects/design.dxf")).unwrap(), saved);
            // The GUI may export anywhere, and that grants the directory.
            dispatch(&Caller::Gui, &w, "artifact_export", json!({"id": id, "path": outside.join("bench.dxf")}), None).unwrap();
            ok(&mcp(&w, "artifact_export", json!({"id": id, "path": outside.join("again.dxf")})));
            assert!(outside.join("again.dxf").exists());
        }
    }
}
