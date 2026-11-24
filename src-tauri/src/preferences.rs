use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Agent {
    Codex,
    Claude,
}
impl Agent {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Appearance {
    pub theme: String,
    pub accent: String,
    pub density: String,
    pub font_size: u8,
    pub reduced_motion: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Workspace {
    pub default_view: String,
    pub restore_last_view: bool,
    pub show_navigator: bool,
    pub show_prescription_dock: bool,
    pub show_metrics: bool,
    pub show_status_bar: bool,
    pub navigator_size: f64,
    pub dock_size: f64,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Display {
    pub decimals: u8,
    pub layout_ray_arrows: bool,
    pub plot_grid: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Analysis {
    pub sampling_kind: String,
    pub grid_size: u16,
    pub hex_rings: u8,
    pub report_precision: u8,
    pub poll_interval_ms: u32,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Agents {
    pub targets: Vec<Agent>,
    pub auto_register: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Preferences {
    pub locale: String,
    pub appearance: Appearance,
    pub workspace: Workspace,
    pub display: Display,
    pub analysis: Analysis,
    pub agents: Agents,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            locale: "zh-CN".into(),
            appearance: Appearance {
                theme: "system".into(),
                accent: "blue".into(),
                density: "compact".into(),
                font_size: 13,
                reduced_motion: false,
            },
            workspace: Workspace {
                default_view: "Layout".into(),
                restore_last_view: true,
                show_navigator: true,
                show_prescription_dock: true,
                show_metrics: true,
                show_status_bar: true,
                navigator_size: 18.0,
                dock_size: 35.0,
            },
            display: Display {
                decimals: 4,
                layout_ray_arrows: true,
                plot_grid: true,
            },
            analysis: Analysis {
                sampling_kind: "grid".into(),
                grid_size: 33,
                hex_rings: 8,
                report_precision: 6,
                poll_interval_ms: 1500,
            },
            agents: Agents {
                targets: vec![],
                auto_register: false,
            },
        }
    }
}
impl Preferences {
    pub fn validate(&self) -> Result<(), String> {
        fn choice(value: &str, choices: &[&str], field: &str) -> Result<(), String> {
            if choices.contains(&value) {
                Ok(())
            } else {
                Err(format!("Invalid {field}"))
            }
        }
        choice(&self.locale, &["zh-CN", "en-US"], "locale")?;
        choice(
            &self.appearance.theme,
            &["system", "light", "dark", "high-contrast"],
            "theme",
        )?;
        choice(
            &self.appearance.accent,
            &["blue", "teal", "amber"],
            "accent",
        )?;
        choice(
            &self.appearance.density,
            &["compact", "comfortable"],
            "density",
        )?;
        choice(
            &self.workspace.default_view,
            &[
                "Layout",
                "Spot diagram",
                "MTF",
                "Analysis",
                "Ray inspection",
                "Materials",
                "Prescription",
                "Prescription report",
                "Agent connections",
            ],
            "default view",
        )?;
        choice(
            &self.analysis.sampling_kind,
            &["grid", "hexapolar"],
            "sampling kind",
        )?;
        if !(11..=18).contains(&self.appearance.font_size) {
            return Err("Font size must be between 11 and 18".into());
        }
        if !self.workspace.navigator_size.is_finite()
            || !(10.0..=40.0).contains(&self.workspace.navigator_size)
        {
            return Err("Navigator size must be between 10 and 40 percent".into());
        }
        if !self.workspace.dock_size.is_finite()
            || !(15.0..=60.0).contains(&self.workspace.dock_size)
        {
            return Err("Dock size must be between 15 and 60 percent".into());
        }
        if self.display.decimals > 8 {
            return Err("Display decimals must be between 0 and 8".into());
        }
        if !(3..=129).contains(&self.analysis.grid_size) || self.analysis.grid_size % 2 == 0 {
            return Err("Grid size must be odd, between 3 and 129".into());
        }
        if !(1..=64).contains(&self.analysis.hex_rings) {
            return Err("Hexapolar rings must be between 1 and 64".into());
        }
        if self.analysis.report_precision > 15 {
            return Err("Report precision must be between 0 and 15".into());
        }
        if !(500..=10000).contains(&self.analysis.poll_interval_ms) {
            return Err("Polling interval must be between 500 and 10000 milliseconds".into());
        }
        if self
            .agents
            .targets
            .iter()
            .enumerate()
            .any(|(i, agent)| self.agents.targets[..i].contains(agent))
        {
            return Err("Agent targets must be unique".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub preferences: Preferences,
    pub revision: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}
#[derive(Debug, Serialize)]
pub struct Error {
    pub code: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<Snapshot>,
}
impl Error {
    pub fn storage(message: impl ToString) -> Self {
        Self {
            code: "storage",
            message: message.to_string(),
            snapshot: None,
        }
    }
}
impl Snapshot {
    /// Commit memory only after persistence succeeds. The caller holds the shared window mutex.
    pub fn commit(
        &mut self,
        preferences: Preferences,
        expected_revision: u64,
        persist: impl FnOnce(&Snapshot) -> Result<(), String>,
    ) -> Result<Snapshot, Error> {
        if expected_revision != self.revision {
            return Err(Error { code: "revision_conflict", message: "Settings changed in another window. Reload the current settings before applying edits.".into(), snapshot: Some(self.clone()) });
        }
        preferences.validate().map_err(|message| Error {
            code: "validation",
            message,
            snapshot: None,
        })?;
        if preferences == self.preferences && self.warning.is_none() {
            return Ok(self.clone());
        }
        let revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| Error::storage("Settings revision exhausted"))?;
        let next = Snapshot {
            preferences,
            revision,
            warning: None,
        };
        persist(&next).map_err(Error::storage)?;
        *self = next.clone();
        Ok(next)
    }
}

/// Read without writing so malformed preferences remain inspectable until an explicit Apply.
pub fn load(path: &std::path::Path) -> Snapshot {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Stored {
        snapshot: Snapshot,
    }
    let result = match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice::<Stored>(&bytes)
            .map_err(|e| e.to_string())
            .and_then(|stored| {
                stored.snapshot.preferences.validate()?;
                Ok(stored.snapshot)
            }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Snapshot::default(),
        Err(error) => Err(error.to_string()),
    };
    result.unwrap_or_else(|error| Snapshot { warning: Some(format!("Settings could not be loaded from {}: {error}. The existing file is unchanged; Apply settings to save a new selection.", path.display())), ..Snapshot::default() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn preferences_validate_real_control_boundaries_and_strict_shapes() {
        let defaults = Preferences::default();
        defaults.validate().unwrap();
        let original = serde_json::to_value(&defaults).unwrap();
        for (path, value) in [
            (vec!["locale"], json!("xx")),
            (vec!["appearance", "theme"], json!("purple")),
            (vec!["appearance", "fontSize"], json!(10)),
            (vec!["appearance", "fontSize"], json!(19)),
            (vec!["workspace", "navigatorSize"], json!(9.9)),
            (vec!["workspace", "dockSize"], json!(60.1)),
            (vec!["workspace", "defaultView"], json!("missing")),
            (vec!["display", "decimals"], json!(9)),
            (vec!["analysis", "samplingKind"], json!("radial")),
            (vec!["analysis", "gridSize"], json!(32)),
            (vec!["analysis", "gridSize"], json!(131)),
            (vec!["analysis", "hexRings"], json!(0)),
            (vec!["analysis", "hexRings"], json!(65)),
            (vec!["analysis", "reportPrecision"], json!(16)),
            (vec!["analysis", "pollIntervalMs"], json!(499)),
            (vec!["agents", "targets"], json!(["codex", "codex"])),
        ] {
            let mut value_object = original.clone();
            let mut location = &mut value_object;
            for key in path {
                location = &mut location[key];
            }
            *location = value;
            let preferences: Preferences = serde_json::from_value(value_object).unwrap();
            assert!(preferences.validate().is_err());
        }
        for value in [json!(null), json!("13"), json!(12.5), json!(-1)] {
            let mut object = original.clone();
            object["appearance"]["fontSize"] = value;
            assert!(serde_json::from_value::<Preferences>(object).is_err());
        }
        let mut object = original.clone();
        object["appearance"]["extra"] = json!(true);
        assert!(serde_json::from_value::<Preferences>(object).is_err());
        let mut object = original;
        object["agents"]["targets"] = json!(["unknown"]);
        assert!(serde_json::from_value::<Preferences>(object).is_err());
        let mut limits = defaults;
        limits.appearance.font_size = 18;
        limits.workspace.navigator_size = 40.0;
        limits.workspace.dock_size = 15.0;
        limits.display.decimals = 8;
        limits.analysis.grid_size = 129;
        limits.analysis.hex_rings = 64;
        limits.analysis.report_precision = 15;
        limits.analysis.poll_interval_ms = 10000;
        limits.agents.targets = vec![Agent::Codex, Agent::Claude];
        limits.validate().unwrap();
    }
    #[test]
    fn preferences_commit_orders_windows_and_retains_unsaved_state_on_storage_failure() {
        let mut current = Snapshot::default();
        let mut first = current.preferences.clone();
        first.locale = "en-US".into();
        let mut writes = 0;
        let saved = current
            .commit(first.clone(), 0, |next| {
                writes += 1;
                assert_eq!(next.revision, 1);
                Ok(())
            })
            .unwrap();
        assert_eq!(saved.preferences, first);
        assert_eq!(writes, 1);
        let conflict = current
            .commit(Preferences::default(), 0, |_| {
                panic!("Conflict must not write")
            })
            .unwrap_err();
        assert_eq!(conflict.code, "revision_conflict");
        assert_eq!(conflict.snapshot.unwrap(), saved);
        let mut changed = first.clone();
        changed.display.decimals = 7;
        let error = current
            .commit(changed.clone(), 1, |_| Err("Disk full".into()))
            .unwrap_err();
        assert_eq!(error.code, "storage");
        assert_eq!(current, saved);
        let recovered = current.commit(changed, 1, |_| Ok(())).unwrap();
        assert_eq!(recovered.revision, 2);
        let same = current
            .commit(recovered.preferences.clone(), 2, |_| {
                panic!("Unchanged preferences must not write")
            })
            .unwrap();
        assert_eq!(same, recovered);
        let mut invalid = recovered.preferences.clone();
        invalid.analysis.grid_size = 4;
        assert_eq!(
            current
                .commit(invalid, 2, |_| panic!("Invalid preferences must not write"))
                .unwrap_err()
                .code,
            "validation"
        );
        assert_eq!(current, recovered);
    }
    #[test]
    fn preferences_restart_and_damaged_file_loading_preserve_source_bytes() {
        // Use only the project's existing build directory.
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!("preferences-test-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        assert_eq!(load(&path), Snapshot::default());
        let mut current = Snapshot::default();
        let mut changed = current.preferences.clone();
        changed.appearance.theme = "dark".into();
        changed.agents.targets = vec![Agent::Claude];
        changed.agents.auto_register = true;
        let saved = current
            .commit(changed, 0, |snapshot| {
                std::fs::write(
                    &path,
                    serde_json::to_vec(&json!({"snapshot":snapshot})).unwrap(),
                )
                .map_err(|e| e.to_string())
            })
            .unwrap();
        assert_eq!(load(&path), saved);
        for invalid in [
            b"broken settings\n".as_slice(),
            br#"{"snapshot":{"preferences":{},"revision":4}}"#,
        ] {
            std::fs::write(&path, invalid).unwrap();
            let loaded = load(&path);
            assert!(loaded.warning.is_some());
            assert_eq!(loaded.preferences, Preferences::default());
            assert_eq!(std::fs::read(&path).unwrap(), invalid);
        }
        let mut with_warning = load(&path);
        let saved = with_warning
            .commit(Preferences::default(), 0, |_| Ok(()))
            .unwrap();
        assert_eq!(saved.revision, 1);
        assert_eq!(saved.warning, None);
        std::fs::remove_file(path).unwrap();
    }
}
