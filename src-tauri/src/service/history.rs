// SPDX-License-Identifier: GPL-3.0-or-later
// Bounded in-memory revision history stack and Before/After optical diff.
use crate::optics::{PupilSampling, System, analysis};
use serde::Serialize;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Serialize, schemars::JsonSchema)]
pub(super) struct HistoryList {
    current_revision: u64,
    cursor: usize,
    can_undo: bool,
    can_redo: bool,
    entries: Vec<HistorySummary>,
}

#[derive(Serialize, schemars::JsonSchema)]
struct Metrics {
    name: String,
    surfaces: usize,
    total_track: f64,
    efl: Option<f64>,
    bfl: Option<f64>,
    f_number: Option<f64>,
    rms_spot_mm: Vec<Option<f64>>,
}
#[derive(Serialize, schemars::JsonSchema)]
struct ComparedEntry {
    index: usize,
    revision: u64,
    action: String,
    timestamp_ms: u64,
    metrics: Metrics,
}
#[derive(Serialize, schemars::JsonSchema)]
struct Deltas {
    total_track: f64,
    efl: Option<f64>,
    bfl: Option<f64>,
    f_number: Option<f64>,
    rms_spot_mm: Vec<Option<f64>>,
}
#[derive(Serialize, schemars::JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
enum SurfaceChange {
    Modified {
        surface: usize,
        changes: Vec<FieldChange>,
    },
    Added {
        surface: usize,
        label: String,
    },
    Removed {
        surface: usize,
        label: String,
    },
}
#[derive(Serialize, schemars::JsonSchema)]
#[serde(tag = "field", rename_all = "snake_case")]
enum FieldChange {
    Radius {
        before: f64,
        after: f64,
    },
    Thickness {
        before: f64,
        after: f64,
    },
    Conic {
        before: f64,
        after: f64,
    },
    Aperture {
        before: f64,
        after: f64,
    },
    Material {
        before: String,
        after: String,
    },
    Asphere {
        before: bool,
        after: bool,
    },
    Transform {
        before: Option<crate::optics::CoordinateTransform>,
        after: Option<crate::optics::CoordinateTransform>,
    },
}

#[derive(Serialize, schemars::JsonSchema)]
pub(super) struct Comparison {
    from: ComparedEntry,
    to: ComparedEntry,
    deltas: Deltas,
    surface_changes: Vec<SurfaceChange>,
}

const MAX_HISTORY: usize = 64;

#[derive(Clone, Debug)]
pub struct HistoryEntry {
    pub revision: u64,
    pub timestamp_ms: u64,
    pub action: String,
    pub system: System,
    pub linked_path: Option<PathBuf>,
}

#[derive(schemars::JsonSchema, Clone, Debug, Serialize)]
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
        from: Option<usize>,
        to: Option<usize>,
    ) -> Result<(&HistoryEntry, &HistoryEntry, usize, usize), String> {
        let from = from.unwrap_or(self.cursor.saturating_sub(1));
        let to = to.unwrap_or(self.cursor);
        Ok((self.entry(from)?, self.entry(to)?, from, to))
    }

    pub fn list(&self, current_revision: u64) -> HistoryList {
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
        HistoryList {
            current_revision,
            cursor: self.cursor,
            can_undo: self.can_undo(),
            can_redo: self.can_redo(),
            entries: items,
        }
    }
}

fn metrics(system: &System) -> Metrics {
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
    Metrics {
        name: resolved.name,
        surfaces: resolved.surfaces.len(),
        total_track: resolved.surfaces.iter().map(|s| s.thickness).sum::<f64>(),
        efl: p.as_ref().and_then(|v| v["efl"].as_f64()),
        bfl: p.as_ref().and_then(|v| v["bfl"].as_f64()),
        f_number: p.as_ref().and_then(|v| v["f_number"].as_f64()),
        rms_spot_mm: spots,
    }
}

pub fn compare(
    from: &HistoryEntry,
    to: &HistoryEntry,
    from_index: usize,
    to_index: usize,
) -> Comparison {
    let m_from = metrics(&from.system);
    let m_to = metrics(&to.system);

    let delta = |a: Option<f64>, b: Option<f64>| Some(b? - a?);

    let mut surface_changes = vec![];
    let max_len = from.system.surfaces.len().max(to.system.surfaces.len());
    for i in 0..max_len {
        match (from.system.surfaces.get(i), to.system.surfaces.get(i)) {
            (Some(a), Some(b)) => {
                let mut diffs = vec![];
                if (a.radius - b.radius).abs() > 1e-12 {
                    diffs.push(FieldChange::Radius {
                        before: a.radius,
                        after: b.radius,
                    });
                }
                if (a.thickness - b.thickness).abs() > 1e-12 {
                    diffs.push(FieldChange::Thickness {
                        before: a.thickness,
                        after: b.thickness,
                    });
                }
                if a.material != b.material {
                    diffs.push(FieldChange::Material {
                        before: a.material.clone(),
                        after: b.material.clone(),
                    });
                }
                if (a.conic - b.conic).abs() > 1e-12 {
                    diffs.push(FieldChange::Conic {
                        before: a.conic,
                        after: b.conic,
                    });
                }
                if (a.aperture - b.aperture).abs() > 1e-12 {
                    diffs.push(FieldChange::Aperture {
                        before: a.aperture,
                        after: b.aperture,
                    });
                }
                if a.even != b.even || a.odd != b.odd {
                    diffs.push(FieldChange::Asphere {
                        before: true,
                        after: true,
                    });
                }
                if a.transform != b.transform {
                    diffs.push(FieldChange::Transform {
                        before: a.transform.clone(),
                        after: b.transform.clone(),
                    });
                }
                if !diffs.is_empty() {
                    surface_changes.push(SurfaceChange::Modified {
                        surface: i,
                        changes: diffs,
                    });
                }
            }
            (None, Some(b)) => {
                surface_changes.push(SurfaceChange::Added {
                    surface: i,
                    label: b.label.clone(),
                });
            }
            (Some(a), None) => {
                surface_changes.push(SurfaceChange::Removed {
                    surface: i,
                    label: a.label.clone(),
                });
            }
            (None, None) => {}
        }
    }

    let deltas = Deltas {
        total_track: m_to.total_track - m_from.total_track,
        efl: delta(m_from.efl, m_to.efl),
        bfl: delta(m_from.bfl, m_to.bfl),
        f_number: delta(m_from.f_number, m_to.f_number),
        rms_spot_mm: m_from
            .rms_spot_mm
            .iter()
            .zip(&m_to.rms_spot_mm)
            .map(|(&a, &b)| delta(a, b))
            .collect(),
    };
    let snapshot = |e: &HistoryEntry, index, metrics| ComparedEntry {
        index,
        revision: e.revision,
        action: e.action.clone(),
        timestamp_ms: e.timestamp_ms,
        metrics,
    };
    Comparison {
        from: snapshot(from, from_index, m_from),
        to: snapshot(to, to_index, m_to),
        deltas,
        surface_changes,
    }
}
