// SPDX-License-Identifier: GPL-3.0-or-later
// Bounded in-memory revision history stack and Before/After optical diff.
use crate::optics::{PupilSampling, System, analysis};
use serde::Serialize;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_HISTORY: usize = 64;

#[derive(Clone, Debug)]
pub struct HistoryEntry {
    pub revision: u64,
    pub timestamp_ms: u64,
    pub action: String,
    pub system: System,
    pub linked_path: Option<PathBuf>,
}

#[derive(Clone, Debug, Serialize)]
pub struct HistorySummary {
    pub index: usize,
    pub revision: u64,
    pub timestamp_ms: u64,
    pub action: String,
    pub name: String,
    pub surfaces: usize,
    pub current: bool,
}

#[derive(Clone, Debug)]
pub struct History {
    entries: Vec<HistoryEntry>,
    cursor: usize,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

impl History {
    pub fn initial(revision: u64, system: System, linked_path: Option<PathBuf>) -> Self {
        Self {
            entries: vec![HistoryEntry {
                revision,
                timestamp_ms: now_ms(),
                action: "initial".into(),
                system,
                linked_path,
            }],
            cursor: 0,
        }
    }

    pub fn push(
        &mut self,
        revision: u64,
        action: impl Into<String>,
        system: System,
        linked_path: Option<PathBuf>,
    ) {
        if self.cursor + 1 < self.entries.len() {
            self.entries.truncate(self.cursor + 1);
        }
        self.entries.push(HistoryEntry {
            revision,
            timestamp_ms: now_ms(),
            action: action.into(),
            system,
            linked_path,
        });
        if self.entries.len() > MAX_HISTORY {
            let overflow = self.entries.len() - MAX_HISTORY;
            self.entries.drain(0..overflow);
        }
        self.cursor = self.entries.len() - 1;
    }

    pub fn can_undo(&self) -> bool {
        self.cursor > 0
    }

    pub fn can_redo(&self) -> bool {
        self.cursor + 1 < self.entries.len()
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn peek_undo(&self) -> Result<&HistoryEntry, String> {
        if !self.can_undo() {
            return Err("Nothing to undo".into());
        }
        Ok(&self.entries[self.cursor - 1])
    }

    pub fn peek_redo(&self) -> Result<&HistoryEntry, String> {
        if !self.can_redo() {
            return Err("Nothing to redo".into());
        }
        Ok(&self.entries[self.cursor + 1])
    }

    pub fn step_undo(&mut self, new_revision: u64) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.entries[self.cursor].revision = new_revision;
        }
    }

    pub fn step_redo(&mut self, new_revision: u64) {
        if self.cursor + 1 < self.entries.len() {
            self.cursor += 1;
            self.entries[self.cursor].revision = new_revision;
        }
    }

    pub fn get(&self, index: usize) -> Option<&HistoryEntry> {
        self.entries.get(index)
    }

    pub fn entry(&self, index: usize) -> Result<&HistoryEntry, String> {
        self.get(index)
            .ok_or_else(|| format!("History index {index} is outside the revision history"))
    }

    pub fn pair(
        &self,
        args: &Value,
    ) -> Result<(&HistoryEntry, &HistoryEntry, usize, usize), String> {
        let from = args["from_index"]
            .as_u64()
            .map_or(self.cursor.saturating_sub(1), |i| i as usize);
        let to = args["to_index"]
            .as_u64()
            .map_or(self.cursor, |i| i as usize);
        Ok((self.entry(from)?, self.entry(to)?, from, to))
    }

    pub fn list(&self, current_revision: u64) -> Value {
        let items: Vec<HistorySummary> = self
            .entries
            .iter()
            .enumerate()
            .map(|(i, e)| HistorySummary {
                index: i,
                revision: e.revision,
                timestamp_ms: e.timestamp_ms,
                action: e.action.clone(),
                name: e.system.name.clone(),
                surfaces: e.system.surfaces.len(),
                current: i == self.cursor,
            })
            .collect();
        json!({
            "current_revision": current_revision,
            "cursor": self.cursor,
            "can_undo": self.can_undo(),
            "can_redo": self.can_redo(),
            "entries": items,
        })
    }
}

fn metrics(system: &System) -> Value {
    let resolved = system.resolved().unwrap_or_else(|_| system.clone());
    let w = resolved.reference();
    let p = resolved.paraxial(w).ok();
    let samp = PupilSampling::Hexapolar { rings: 3 };
    let spots: Vec<Option<f64>> = resolved
        .fields
        .iter()
        .map(|f| {
            analysis::polychromatic_spot(&resolved, f, samp)
                .ok()
                .and_then(|v| v["rms_radius"].as_f64())
        })
        .collect();
    json!({
        "name": resolved.name,
        "surfaces": resolved.surfaces.len(),
        "total_track": resolved.surfaces.iter().map(|s| s.thickness).sum::<f64>(),
        "efl": p.as_ref().and_then(|v| v["efl"].as_f64()),
        "bfl": p.as_ref().and_then(|v| v["bfl"].as_f64()),
        "f_number": p.as_ref().and_then(|v| v["f_number"].as_f64()),
        "rms_spot_mm": spots,
    })
}

pub fn compare(
    from: &HistoryEntry,
    to: &HistoryEntry,
    from_index: usize,
    to_index: usize,
) -> Value {
    let m_from = metrics(&from.system);
    let m_to = metrics(&to.system);

    let sub_opt = |a: &Value, b: &Value| match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => json!(y - x),
        _ => Value::Null,
    };

    let mut surface_changes = vec![];
    let max_len = from.system.surfaces.len().max(to.system.surfaces.len());
    for i in 0..max_len {
        match (from.system.surfaces.get(i), to.system.surfaces.get(i)) {
            (Some(a), Some(b)) => {
                let mut diffs = vec![];
                if (a.radius - b.radius).abs() > 1e-12 {
                    diffs.push(json!({"field":"radius","before":a.radius,"after":b.radius}));
                }
                if (a.thickness - b.thickness).abs() > 1e-12 {
                    diffs.push(
                        json!({"field":"thickness","before":a.thickness,"after":b.thickness}),
                    );
                }
                if a.material != b.material {
                    diffs.push(json!({"field":"material","before":a.material,"after":b.material}));
                }
                if (a.conic - b.conic).abs() > 1e-12 {
                    diffs.push(json!({"field":"conic","before":a.conic,"after":b.conic}));
                }
                if (a.aperture - b.aperture).abs() > 1e-12 {
                    diffs.push(json!({"field":"aperture","before":a.aperture,"after":b.aperture}));
                }
                if a.even != b.even || a.odd != b.odd {
                    diffs.push(json!({"field":"asphere","before":true,"after":true}));
                }
                if a.transform != b.transform {
                    diffs.push(
                        json!({"field":"transform","before":a.transform,"after":b.transform}),
                    );
                }
                if !diffs.is_empty() {
                    surface_changes
                        .push(json!({"surface": i, "status": "modified", "changes": diffs}));
                }
            }
            (None, Some(b)) => {
                surface_changes.push(json!({"surface": i, "status": "added", "label": b.label}));
            }
            (Some(a), None) => {
                surface_changes.push(json!({"surface": i, "status": "removed", "label": a.label}));
            }
            (None, None) => {}
        }
    }

    let spot_deltas: Vec<Value> = match (
        m_from["rms_spot_mm"].as_array(),
        m_to["rms_spot_mm"].as_array(),
    ) {
        (Some(a), Some(b)) => a.iter().zip(b.iter()).map(|(x, y)| sub_opt(x, y)).collect(),
        _ => vec![],
    };

    json!({
        "from": {
            "index": from_index,
            "revision": from.revision,
            "action": from.action,
            "timestamp_ms": from.timestamp_ms,
            "metrics": m_from,
        },
        "to": {
            "index": to_index,
            "revision": to.revision,
            "action": to.action,
            "timestamp_ms": to.timestamp_ms,
            "metrics": m_to,
        },
        "deltas": {
            "efl": sub_opt(&m_from["efl"], &m_to["efl"]),
            "bfl": sub_opt(&m_from["bfl"], &m_to["bfl"]),
            "f_number": sub_opt(&m_from["f_number"], &m_to["f_number"]),
            "total_track": sub_opt(&m_from["total_track"], &m_to["total_track"]),
            "rms_spot_mm": spot_deltas,
        },
        "surface_changes": surface_changes,
    })
}
