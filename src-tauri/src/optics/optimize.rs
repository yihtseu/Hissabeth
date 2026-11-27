// SPDX-License-Identifier: GPL-3.0-or-later
// Damped Least Squares (Levenberg-Marquardt) sequential optical optimizer and design summary.
use super::{
    CoordinateTransform, Interaction, Override, PupilSampling, System, analysis, material, parallel,
    sag,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(schemars::JsonSchema, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis2 {
    X,
    Y,
}

#[derive(schemars::JsonSchema, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis3 {
    X,
    Y,
    Z,
}

#[derive(
    schemars::JsonSchema, Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintMode {
    #[default]
    Eq,
    Lt,
    Gt,
}

#[derive(schemars::JsonSchema, Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Variable {
    Radius {
        #[schemars(range(min = 0, max = 127))]
        surface: usize,
        #[serde(default)]
        #[schemars(range(min = -1e12, max = 1e12))]
        min: Option<f64>,
        #[serde(default)]
        #[schemars(range(min = -1e12, max = 1e12))]
        max: Option<f64>,
    },
    Thickness {
        #[schemars(range(min = 0, max = 127))]
        surface: usize,
        #[serde(default)]
        #[schemars(range(min = -1e12, max = 1e12))]
        min: Option<f64>,
        #[serde(default)]
        #[schemars(range(min = -1e12, max = 1e12))]
        max: Option<f64>,
    },
    Conic {
        #[schemars(range(min = 0, max = 127))]
        surface: usize,
        #[serde(default)]
        #[schemars(range(min = -1e12, max = 1e12))]
        min: Option<f64>,
        #[serde(default)]
        #[schemars(range(min = -1e12, max = 1e12))]
        max: Option<f64>,
    },
    EvenAsphere {
        #[schemars(range(min = 0, max = 127))]
        surface: usize,
        #[schemars(range(min = 0, max = 15))]
        index: usize,
        #[serde(default)]
        #[schemars(range(min = -1e12, max = 1e12))]
        min: Option<f64>,
        #[serde(default)]
        #[schemars(range(min = -1e12, max = 1e12))]
        max: Option<f64>,
    },
    OddAsphere {
        #[schemars(range(min = 0, max = 127))]
        surface: usize,
        #[schemars(range(min = 0, max = 19))]
        index: usize,
        #[serde(default)]
        #[schemars(range(min = -1e12, max = 1e12))]
        min: Option<f64>,
        #[serde(default)]
        #[schemars(range(min = -1e12, max = 1e12))]
        max: Option<f64>,
    },
    ModelGlassNd {
        #[schemars(range(min = 0, max = 127))]
        surface: usize,
        #[serde(default)]
        #[schemars(range(min = -1e12, max = 1e12))]
        min: Option<f64>,
        #[serde(default)]
        #[schemars(range(min = -1e12, max = 1e12))]
        max: Option<f64>,
    },
    ModelGlassVd {
        #[schemars(range(min = 0, max = 127))]
        surface: usize,
        #[serde(default)]
        #[schemars(range(min = -1e12, max = 1e12))]
        min: Option<f64>,
        #[serde(default)]
        #[schemars(range(min = -1e12, max = 1e12))]
        max: Option<f64>,
    },
    Decenter {
        #[schemars(range(min = 0, max = 127))]
        surface: usize,
        axis: Axis2,
        #[serde(default)]
        #[schemars(range(min = -1e12, max = 1e12))]
        min: Option<f64>,
        #[serde(default)]
        #[schemars(range(min = -1e12, max = 1e12))]
        max: Option<f64>,
    },
    Tilt {
        #[schemars(range(min = 0, max = 127))]
        surface: usize,
        axis: Axis3,
        #[serde(default)]
        #[schemars(range(min = -1e12, max = 1e12))]
        min: Option<f64>,
        #[serde(default)]
        #[schemars(range(min = -1e12, max = 1e12))]
        max: Option<f64>,
    },
    ConfigThickness {
        #[schemars(range(min = 0, max = 63))]
        configuration: usize,
        #[schemars(range(min = 0, max = 127))]
        surface: usize,
        #[serde(default)]
        #[schemars(range(min = -1e12, max = 1e12))]
        min: Option<f64>,
        #[serde(default)]
        #[schemars(range(min = -1e12, max = 1e12))]
        max: Option<f64>,
    },
    ConfigRadius {
        #[schemars(range(min = 0, max = 63))]
        configuration: usize,
        #[schemars(range(min = 0, max = 127))]
        surface: usize,
        #[serde(default)]
        #[schemars(range(min = -1e12, max = 1e12))]
        min: Option<f64>,
        #[serde(default)]
        #[schemars(range(min = -1e12, max = 1e12))]
        max: Option<f64>,
    },
}

#[derive(schemars::JsonSchema, Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operand {
    RmsSpot {
        #[serde(default)]
        #[schemars(range(min = 0, max = 63))]
        field_index: Option<usize>,
        #[serde(default)]
        #[schemars(range(min = 100.0, max = 20000.0))]
        wavelength: Option<f64>,
        #[serde(default)]
        sampling: Option<PupilSampling>,
        #[serde(default)]
        #[schemars(range(min = -1e12, max = 1e12))]
        target: f64,
        #[serde(default = "default_weight")]
        #[schemars(range(min = 0.0, max = 1e9))]
        weight: f64,
        #[serde(default)]
        mode: ConstraintMode,
        #[serde(default)]
        #[schemars(range(min = 0, max = 63))]
        configuration: Option<usize>,
    },
    RmsWavefront {
        #[serde(default)]
        #[schemars(range(min = 0, max = 63))]
        field_index: usize,
        #[serde(default)]
        #[schemars(range(min = 100.0, max = 20000.0))]
        wavelength: Option<f64>,
        #[serde(default)]
        #[schemars(range(min = 5, max = 65))]
        grid: Option<usize>,
        #[serde(default)]
        #[schemars(range(min = -1e12, max = 1e12))]
        target: f64,
        #[serde(default = "default_weight")]
        #[schemars(range(min = 0.0, max = 1e9))]
        weight: f64,
        #[serde(default)]
        mode: ConstraintMode,
        #[serde(default)]
        #[schemars(range(min = 0, max = 63))]
        configuration: Option<usize>,
    },
    Efl {
        #[serde(default)]
        #[schemars(range(min = 100.0, max = 20000.0))]
        wavelength: Option<f64>,
        #[schemars(range(min = -1e12, max = 1e12))]
        target: f64,
        #[serde(default = "default_weight")]
        #[schemars(range(min = 0.0, max = 1e9))]
        weight: f64,
        #[serde(default)]
        mode: ConstraintMode,
        #[serde(default)]
        #[schemars(range(min = 0, max = 63))]
        configuration: Option<usize>,
    },
    Bfl {
        #[serde(default)]
        #[schemars(range(min = 100.0, max = 20000.0))]
        wavelength: Option<f64>,
        #[schemars(range(min = -1e12, max = 1e12))]
        target: f64,
        #[serde(default = "default_weight")]
        #[schemars(range(min = 0.0, max = 1e9))]
        weight: f64,
        #[serde(default)]
        mode: ConstraintMode,
        #[serde(default)]
        #[schemars(range(min = 0, max = 63))]
        configuration: Option<usize>,
    },
    WorkingFNumber {
        #[serde(default)]
        #[schemars(range(min = 100.0, max = 20000.0))]
        wavelength: Option<f64>,
        #[schemars(range(min = -1e12, max = 1e12))]
        target: f64,
        #[serde(default = "default_weight")]
        #[schemars(range(min = 0.0, max = 1e9))]
        weight: f64,
        #[serde(default)]
        mode: ConstraintMode,
        #[serde(default)]
        #[schemars(range(min = 0, max = 63))]
        configuration: Option<usize>,
    },
    TotalTrack {
        #[serde(default)]
        #[schemars(range(min = 0, max = 127))]
        first: Option<usize>,
        #[serde(default)]
        #[schemars(range(min = 1, max = 128))]
        last: Option<usize>,
        #[schemars(range(min = -1e12, max = 1e12))]
        target: f64,
        #[serde(default = "default_weight")]
        #[schemars(range(min = 0.0, max = 1e9))]
        weight: f64,
        #[serde(default)]
        mode: ConstraintMode,
        #[serde(default)]
        #[schemars(range(min = 0, max = 63))]
        configuration: Option<usize>,
    },
    CenterThickness {
        #[schemars(range(min = 0, max = 127))]
        surface: usize,
        #[schemars(range(min = -1e12, max = 1e12))]
        target: f64,
        #[serde(default = "default_weight")]
        #[schemars(range(min = 0.0, max = 1e9))]
        weight: f64,
        #[serde(default)]
        mode: ConstraintMode,
        #[serde(default)]
        #[schemars(range(min = 0, max = 63))]
        configuration: Option<usize>,
    },
    EdgeThickness {
        #[schemars(range(min = 0, max = 127))]
        surface: usize,
        #[serde(default)]
        #[schemars(range(min = 0.0, max = 1e6))]
        height: Option<f64>,
        #[schemars(range(min = -1e12, max = 1e12))]
        target: f64,
        #[serde(default = "default_weight")]
        #[schemars(range(min = 0.0, max = 1e9))]
        weight: f64,
        #[serde(default)]
        mode: ConstraintMode,
        #[serde(default)]
        #[schemars(range(min = 0, max = 63))]
        configuration: Option<usize>,
    },
    Distortion {
        #[schemars(range(min = 0, max = 63))]
        field_index: usize,
        #[serde(default)]
        #[schemars(range(min = 100.0, max = 20000.0))]
        wavelength: Option<f64>,
        #[serde(default)]
        #[schemars(range(min = -1e12, max = 1e12))]
        target: f64,
        #[serde(default = "default_weight")]
        #[schemars(range(min = 0.0, max = 1e9))]
        weight: f64,
        #[serde(default)]
        mode: ConstraintMode,
        #[serde(default)]
        #[schemars(range(min = 0, max = 63))]
        configuration: Option<usize>,
    },
    AxialColor {
        #[schemars(range(min = 100.0, max = 20000.0))]
        wave1: f64,
        #[schemars(range(min = 100.0, max = 20000.0))]
        wave2: f64,
        #[serde(default)]
        #[schemars(range(min = -1e12, max = 1e12))]
        target: f64,
        #[serde(default = "default_weight")]
        #[schemars(range(min = 0.0, max = 1e9))]
        weight: f64,
        #[serde(default)]
        mode: ConstraintMode,
        #[serde(default)]
        #[schemars(range(min = 0, max = 63))]
        configuration: Option<usize>,
    },
    LateralColor {
        #[schemars(range(min = 0, max = 63))]
        field_index: usize,
        #[schemars(range(min = 100.0, max = 20000.0))]
        wave1: f64,
        #[schemars(range(min = 100.0, max = 20000.0))]
        wave2: f64,
        #[serde(default)]
        #[schemars(range(min = -1e12, max = 1e12))]
        target: f64,
        #[serde(default = "default_weight")]
        #[schemars(range(min = 0.0, max = 1e9))]
        weight: f64,
        #[serde(default)]
        mode: ConstraintMode,
        #[serde(default)]
        #[schemars(range(min = 0, max = 63))]
        configuration: Option<usize>,
    },
    ChiefRayAngle {
        #[schemars(range(min = 0, max = 63))]
        field_index: usize,
        #[serde(default)]
        #[schemars(range(min = 100.0, max = 20000.0))]
        wavelength: Option<f64>,
        #[serde(default)]
        #[schemars(range(min = -1e12, max = 1e12))]
        target: f64,
        #[serde(default = "default_weight")]
        #[schemars(range(min = 0.0, max = 1e9))]
        weight: f64,
        #[serde(default)]
        mode: ConstraintMode,
        #[serde(default)]
        #[schemars(range(min = 0, max = 63))]
        configuration: Option<usize>,
    },
    RayIntercept {
        #[schemars(range(min = 0, max = 63))]
        field_index: usize,
        #[schemars(range(min = -2.0, max = 2.0))]
        px: f64,
        #[schemars(range(min = -2.0, max = 2.0))]
        py: f64,
        #[serde(default)]
        #[schemars(range(min = 100.0, max = 20000.0))]
        wavelength: Option<f64>,
        axis: Axis2,
        #[serde(default)]
        #[schemars(range(min = -1e12, max = 1e12))]
        target: f64,
        #[serde(default = "default_weight")]
        #[schemars(range(min = 0.0, max = 1e9))]
        weight: f64,
        #[serde(default)]
        mode: ConstraintMode,
        #[serde(default)]
        #[schemars(range(min = 0, max = 63))]
        configuration: Option<usize>,
    },
}

fn default_weight() -> f64 {
    1.
}
fn default_max_iterations() -> usize {
    30
}
fn default_damping() -> f64 {
    1e-3
}
fn default_tolerance() -> f64 {
    1e-7
}

#[derive(schemars::JsonSchema, Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    #[schemars(length(min = 1, max = 64))]
    pub variables: Vec<Variable>,
    #[schemars(length(min = 1, max = 256))]
    pub operands: Vec<Operand>,
    #[serde(default = "default_max_iterations")]
    #[schemars(range(min = 1, max = 200))]
    pub max_iterations: usize,
    #[serde(default = "default_damping")]
    #[schemars(extend("maximum" = 1e6, "exclusiveMinimum" = 0.0))]
    pub initial_damping: f64,
    #[serde(default = "default_tolerance")]
    #[schemars(extend("maximum" = 1.0, "exclusiveMinimum" = 0.0))]
    pub tolerance: f64,
}

#[derive(schemars::JsonSchema, Clone, Debug, Serialize)]
pub struct OperandReport {
    pub index: usize,
    pub label: String,
    pub target: f64,
    pub weight: f64,
    pub mode: ConstraintMode,
    pub before: f64,
    pub after: f64,
    pub contribution_after: f64,
}

#[derive(schemars::JsonSchema, Clone, Debug, Serialize)]
pub struct VariableReport {
    pub index: usize,
    pub label: String,
    pub before: f64,
    pub after: f64,
}

#[derive(schemars::JsonSchema, Clone, Debug, Serialize)]
pub struct IterationRecord {
    pub iteration: usize,
    pub merit: f64,
    pub damping: f64,
    pub accepted: bool,
}

#[derive(schemars::JsonSchema, Clone, Debug, Serialize)]
pub struct OptimizationResult {
    pub system: System,
    pub initial_merit: f64,
    pub final_merit: f64,
    pub iterations: usize,
    pub converged: bool,
    pub reason: String,
    pub variables: Vec<VariableReport>,
    pub operands: Vec<OperandReport>,
    pub history: Vec<IterationRecord>,
}

fn curvature_from_radius(r: f64) -> f64 {
    if r == 0. { 0. } else { 1. / r }
}

fn radius_from_curvature(c: f64) -> f64 {
    if c.abs() < 1e-12 { 0. } else { 1. / c }
}

fn parse_model_glass(s: &System, surface: usize) -> Result<(f64, f64), String> {
    let mat = &s.surfaces[surface].material;
    if let Some((nd, vd)) = mat.split_once(':') {
        let nd: f64 = nd
            .trim()
            .parse()
            .map_err(|_| format!("Invalid model glass on surface {surface}"))?;
        let vd: f64 = vd
            .trim()
            .parse()
            .map_err(|_| format!("Invalid model glass on surface {surface}"))?;
        return Ok((nd, vd));
    }
    if mat == "AIR" {
        return Err(format!(
            "Surface {surface} has AIR material; assign a glass before optimizing model glass"
        ));
    }
    let glass = s
        .custom_glasses
        .iter()
        .find(|g| g.key() == *mat || g.name == *mat)
        .cloned()
        .or_else(|| material::find(mat).ok().cloned())
        .ok_or_else(|| format!("Material {mat} on surface {surface} has no nd/vd for model glass"))?;
    Ok((glass.nd, glass.vd))
}

fn variable_label(v: &Variable) -> String {
    match v {
        Variable::Radius { surface, .. } => format!("S{} Radius", surface + 1),
        Variable::Thickness { surface, .. } => format!("S{} Thickness", surface + 1),
        Variable::Conic { surface, .. } => format!("S{} Conic", surface + 1),
        Variable::EvenAsphere { surface, index, .. } => {
            format!("S{} Even A{}", surface + 1, 4 + 2 * index)
        }
        Variable::OddAsphere { surface, index, .. } => {
            format!("S{} Odd A{}", surface + 1, 3 + index)
        }
        Variable::ModelGlassNd { surface, .. } => format!("S{} Model nd", surface + 1),
        Variable::ModelGlassVd { surface, .. } => format!("S{} Model vd", surface + 1),
        Variable::Decenter { surface, axis, .. } => format!("S{} Decenter {:?}", surface + 1, axis),
        Variable::Tilt { surface, axis, .. } => format!("S{} Tilt {:?}", surface + 1, axis),
        Variable::ConfigThickness {
            configuration,
            surface,
            ..
        } => format!("Cfg {} S{} Thickness", configuration + 1, surface + 1),
        Variable::ConfigRadius {
            configuration,
            surface,
            ..
        } => format!("Cfg {} S{} Radius", configuration + 1, surface + 1),
    }
}

/// Internal optimization coordinate (curvatures for radii, direct values for others).
fn get_var_internal(s: &System, v: &Variable) -> Result<f64, String> {
    let check_surf = |idx: usize| -> Result<(), String> {
        if idx >= s.surfaces.len() {
            Err(format!("Variable surface {idx} outside prescription"))
        } else {
            Ok(())
        }
    };
    match *v {
        Variable::Radius { surface, .. } => {
            check_surf(surface)?;
            Ok(curvature_from_radius(s.surfaces[surface].radius))
        }
        Variable::Thickness { surface, .. } => {
            check_surf(surface)?;
            if !s.surfaces[surface].gap_solve.is_fixed() {
                return Err(format!(
                    "Surface {surface} thickness is controlled by a gap solve and cannot be a variable"
                ));
            }
            Ok(s.surfaces[surface].thickness)
        }
        Variable::Conic { surface, .. } => {
            check_surf(surface)?;
            Ok(s.surfaces[surface].conic)
        }
        Variable::EvenAsphere { surface, index, .. } => {
            check_surf(surface)?;
            if index >= 16 {
                return Err("Even asphere index must be 0..15".into());
            }
            Ok(s.surfaces[surface].even.get(index).copied().unwrap_or(0.))
        }
        Variable::OddAsphere { surface, index, .. } => {
            check_surf(surface)?;
            if index >= 20 {
                return Err("Odd asphere index must be 0..19".into());
            }
            Ok(s.surfaces[surface].odd.get(index).copied().unwrap_or(0.))
        }
        Variable::ModelGlassNd { surface, .. } => {
            check_surf(surface)?;
            Ok(parse_model_glass(s, surface)?.0)
        }
        Variable::ModelGlassVd { surface, .. } => {
            check_surf(surface)?;
            Ok(parse_model_glass(s, surface)?.1)
        }
        Variable::Decenter { surface, axis, .. } => {
            check_surf(surface)?;
            let d = s.surfaces[surface]
                .transform
                .as_ref()
                .map_or([0., 0.], |t| t.decenter);
            Ok(match axis {
                Axis2::X => d[0],
                Axis2::Y => d[1],
            })
        }
        Variable::Tilt { surface, axis, .. } => {
            check_surf(surface)?;
            let t = s.surfaces[surface]
                .transform
                .as_ref()
                .map_or([0., 0., 0.], |t| t.tilt);
            Ok(match axis {
                Axis3::X => t[0],
                Axis3::Y => t[1],
                Axis3::Z => t[2],
            })
        }
        Variable::ConfigThickness {
            configuration,
            surface,
            ..
        } => {
            check_surf(surface)?;
            let cfg = s
                .configurations
                .get(configuration)
                .ok_or("Variable configuration outside table")?;
            let val = cfg
                .overrides
                .iter()
                .find_map(|o| match *o {
                    Override::Thickness {
                        surface: k,
                        value,
                    } if k == surface => Some(value),
                    _ => None,
                })
                .unwrap_or(s.surfaces[surface].thickness);
            Ok(val)
        }
        Variable::ConfigRadius {
            configuration,
            surface,
            ..
        } => {
            check_surf(surface)?;
            let cfg = s
                .configurations
                .get(configuration)
                .ok_or("Variable configuration outside table")?;
            let val = cfg
                .overrides
                .iter()
                .find_map(|o| match *o {
                    Override::Radius {
                        surface: k,
                        value,
                    } if k == surface => Some(value),
                    _ => None,
                })
                .unwrap_or(s.surfaces[surface].radius);
            Ok(curvature_from_radius(val))
        }
    }
}

fn get_var_display(s: &System, v: &Variable) -> Result<f64, String> {
    let raw = get_var_internal(s, v)?;
    Ok(match v {
        Variable::Radius { .. } | Variable::ConfigRadius { .. } => radius_from_curvature(raw),
        _ => raw,
    })
}

fn clamp_var_internal(v: &Variable, x: f64) -> f64 {
    match *v {
        Variable::Radius { min, max, .. } | Variable::ConfigRadius { min, max, .. } => {
            // If bounds are provided on radius with matching sign, enforce them in curvature space.
            match (min, max) {
                (Some(lo), Some(hi)) if lo > 0. && hi > 0. => x.clamp(1. / hi, 1. / lo),
                (Some(lo), Some(hi)) if lo < 0. && hi < 0. => x.clamp(1. / hi, 1. / lo),
                (Some(lo), None) if lo > 0. => x.clamp(1e-9, 1. / lo),
                (None, Some(hi)) if hi < 0. => x.clamp(1. / hi, -1e-9),
                _ => x.clamp(-1.0, 1.0), // curvature limit |R| >= 1mm when unbounded
            }
        }
        Variable::ModelGlassNd { min, max, .. } => {
            x.clamp(min.unwrap_or(1.4), max.unwrap_or(2.1))
        }
        Variable::ModelGlassVd { min, max, .. } => {
            x.clamp(min.unwrap_or(18.), max.unwrap_or(95.))
        }
        Variable::Thickness { min, max, .. }
        | Variable::Conic { min, max, .. }
        | Variable::EvenAsphere { min, max, .. }
        | Variable::OddAsphere { min, max, .. }
        | Variable::Decenter { min, max, .. }
        | Variable::Tilt { min, max, .. }
        | Variable::ConfigThickness { min, max, .. } => {
            let mut val = x;
            if let Some(lo) = min {
                val = val.max(lo);
            }
            if let Some(hi) = max {
                val = val.min(hi);
            }
            val
        }
    }
}

fn var_step(v: &Variable, x: f64) -> f64 {
    match v {
        Variable::Radius { .. } | Variable::ConfigRadius { .. } => 1e-6_f64.max(x.abs() * 1e-5),
        Variable::EvenAsphere { .. } | Variable::OddAsphere { .. } => {
            1e-10_f64.max(x.abs() * 1e-4)
        }
        Variable::ModelGlassNd { .. } => 1e-4,
        Variable::ModelGlassVd { .. } => 1e-2,
        Variable::Thickness { .. }
        | Variable::Conic { .. }
        | Variable::Decenter { .. }
        | Variable::Tilt { .. }
        | Variable::ConfigThickness { .. } => 1e-4_f64.max(x.abs() * 1e-5),
    }
}

fn set_var_internal(s: &mut System, v: &Variable, x: f64) -> Result<(), String> {
    let x = clamp_var_internal(v, x);
    match *v {
        Variable::Radius { surface, .. } => {
            s.surfaces[surface].radius = radius_from_curvature(x);
        }
        Variable::Thickness { surface, .. } => {
            s.surfaces[surface].thickness = x;
        }
        Variable::Conic { surface, .. } => {
            s.surfaces[surface].conic = x;
        }
        Variable::EvenAsphere { surface, index, .. } => {
            let even = &mut s.surfaces[surface].even;
            if even.len() <= index {
                even.resize(index + 1, 0.);
            }
            even[index] = x;
        }
        Variable::OddAsphere { surface, index, .. } => {
            let odd = &mut s.surfaces[surface].odd;
            if odd.len() <= index {
                odd.resize(index + 1, 0.);
            }
            odd[index] = x;
        }
        Variable::ModelGlassNd { surface, .. } => {
            let (_, vd) = parse_model_glass(s, surface)?;
            s.surfaces[surface].material = format!("{x:.8}:{vd:.6}");
        }
        Variable::ModelGlassVd { surface, .. } => {
            let (nd, _) = parse_model_glass(s, surface)?;
            s.surfaces[surface].material = format!("{nd:.8}:{x:.6}");
        }
        Variable::Decenter { surface, axis, .. } => {
            let tr = s.surfaces[surface]
                .transform
                .get_or_insert_with(CoordinateTransform::default);
            match axis {
                Axis2::X => tr.decenter[0] = x,
                Axis2::Y => tr.decenter[1] = x,
            }
        }
        Variable::Tilt { surface, axis, .. } => {
            let tr = s.surfaces[surface]
                .transform
                .get_or_insert_with(CoordinateTransform::default);
            match axis {
                Axis3::X => tr.tilt[0] = x,
                Axis3::Y => tr.tilt[1] = x,
                Axis3::Z => tr.tilt[2] = x,
            }
        }
        Variable::ConfigThickness {
            configuration,
            surface,
            ..
        } => {
            let cfg = &mut s.configurations[configuration];
            if let Some(Override::Thickness { value, .. }) = cfg
                .overrides
                .iter_mut()
                .find(|o| matches!(o, Override::Thickness { surface: k, .. } if *k == surface))
            {
                *value = x;
            } else {
                cfg.overrides.push(Override::Thickness { surface, value: x });
            }
        }
        Variable::ConfigRadius {
            configuration,
            surface,
            ..
        } => {
            let r = radius_from_curvature(x);
            let cfg = &mut s.configurations[configuration];
            if let Some(Override::Radius { value, .. }) = cfg
                .overrides
                .iter_mut()
                .find(|o| matches!(o, Override::Radius { surface: k, .. } if *k == surface))
            {
                *value = r;
            } else {
                cfg.overrides.push(Override::Radius { surface, value: r });
            }
        }
    }
    Ok(())
}

fn operand_meta(op: &Operand) -> (String, f64, f64, ConstraintMode, Option<usize>) {
    match op {
        Operand::RmsSpot {
            field_index,
            target,
            weight,
            mode,
            configuration,
            ..
        } => (
            match field_index {
                Some(i) => format!("RMS Spot (F{})", i + 1),
                None => "RMS Spot (All Fields)".into(),
            },
            *target,
            *weight,
            *mode,
            *configuration,
        ),
        Operand::RmsWavefront {
            field_index,
            target,
            weight,
            mode,
            configuration,
            ..
        } => (
            format!("RMS Wavefront (F{})", field_index + 1),
            *target,
            *weight,
            *mode,
            *configuration,
        ),
        Operand::Efl {
            target,
            weight,
            mode,
            configuration,
            ..
        } => ("EFL".into(), *target, *weight, *mode, *configuration),
        Operand::Bfl {
            target,
            weight,
            mode,
            configuration,
            ..
        } => ("BFL".into(), *target, *weight, *mode, *configuration),
        Operand::WorkingFNumber {
            target,
            weight,
            mode,
            configuration,
            ..
        } => ("Working F/#".into(), *target, *weight, *mode, *configuration),
        Operand::TotalTrack {
            target,
            weight,
            mode,
            configuration,
            ..
        } => ("Total Track".into(), *target, *weight, *mode, *configuration),
        Operand::CenterThickness {
            surface,
            target,
            weight,
            mode,
            configuration,
        } => (
            format!("S{} Center Thickness", surface + 1),
            *target,
            *weight,
            *mode,
            *configuration,
        ),
        Operand::EdgeThickness {
            surface,
            target,
            weight,
            mode,
            configuration,
            ..
        } => (
            format!("S{} Edge Thickness", surface + 1),
            *target,
            *weight,
            *mode,
            *configuration,
        ),
        Operand::Distortion {
            field_index,
            target,
            weight,
            mode,
            configuration,
            ..
        } => (
            format!("Distortion (F{})", field_index + 1),
            *target,
            *weight,
            *mode,
            *configuration,
        ),
        Operand::AxialColor {
            target,
            weight,
            mode,
            configuration,
            ..
        } => ("Axial Color".into(), *target, *weight, *mode, *configuration),
        Operand::LateralColor {
            field_index,
            target,
            weight,
            mode,
            configuration,
            ..
        } => (
            format!("Lateral Color (F{})", field_index + 1),
            *target,
            *weight,
            *mode,
            *configuration,
        ),
        Operand::ChiefRayAngle {
            field_index,
            target,
            weight,
            mode,
            configuration,
            ..
        } => (
            format!("Chief Ray Angle (F{})", field_index + 1),
            *target,
            *weight,
            *mode,
            *configuration,
        ),
        Operand::RayIntercept {
            field_index,
            axis,
            target,
            weight,
            mode,
            configuration,
            ..
        } => (
            format!("Ray Intercept (F{}, {:?})", field_index + 1, axis),
            *target,
            *weight,
            *mode,
            *configuration,
        ),
    }
}

fn evaluate_operand(s: &System, op: &Operand) -> Result<f64, String> {
    let default_sampling = PupilSampling::Hexapolar { rings: 3 };
    match op {
        Operand::RmsSpot {
            field_index,
            wavelength,
            sampling,
            ..
        } => {
            let samp = sampling.unwrap_or(default_sampling);
            let eval_field = |fi: usize| -> Result<f64, String> {
                let field = s
                    .fields
                    .get(fi)
                    .ok_or("Operand field_index outside prescription")?;
                let val = match wavelength {
                    Some(w) => s.spot_field(field, *w, samp)?["rms_radius"]
                        .as_f64()
                        .ok_or("Missing RMS spot radius")?,
                    None => analysis::polychromatic_spot(s, field, samp)?["rms_radius"]
                        .as_f64()
                        .ok_or("Missing polychromatic RMS spot radius")?,
                };
                Ok(val)
            };
            if let Some(fi) = *field_index {
                eval_field(fi)
            } else {
                let mut sum_w = 0.;
                let mut sum_sq = 0.;
                for (fi, f) in s.fields.iter().enumerate() {
                    if f.weight <= 0. {
                        continue;
                    }
                    let rms = eval_field(fi)?;
                    sum_sq += f.weight * rms * rms;
                    sum_w += f.weight;
                }
                if sum_w <= 0. {
                    return Err("No positive-weight fields".into());
                }
                Ok((sum_sq / sum_w).sqrt())
            }
        }
        Operand::RmsWavefront {
            field_index,
            wavelength,
            grid,
            ..
        } => {
            let field = s
                .fields
                .get(*field_index)
                .ok_or("Operand field_index outside prescription")?;
            let w = wavelength.unwrap_or_else(|| s.reference());
            let wf = analysis::wavefront(s, field, w, grid.unwrap_or(11))?;
            wf.json()["rms_waves"]
                .as_f64()
                .ok_or_else(|| "Missing RMS wavefront".into())
        }
        Operand::Efl { wavelength, .. } => {
            let w = wavelength.unwrap_or_else(|| s.reference());
            s.paraxial(w)?["efl"]
                .as_f64()
                .ok_or_else(|| "Undefined EFL (afocal system)".into())
        }
        Operand::Bfl { wavelength, .. } => {
            let w = wavelength.unwrap_or_else(|| s.reference());
            s.paraxial(w)?["bfl"]
                .as_f64()
                .ok_or_else(|| "Undefined BFL (afocal system)".into())
        }
        Operand::WorkingFNumber { wavelength, .. } => {
            let w = wavelength.unwrap_or_else(|| s.reference());
            s.paraxial(w)?["f_number"]
                .as_f64()
                .ok_or_else(|| "Undefined working F-number".into())
        }
        Operand::TotalTrack { first, last, .. } => {
            let a = first.unwrap_or(0);
            let b = last.unwrap_or(s.surfaces.len());
            if a >= b || b > s.surfaces.len() {
                return Err("Invalid TotalTrack surface range".into());
            }
            Ok(s.surfaces[a..b].iter().map(|x| x.thickness).sum())
        }
        Operand::CenterThickness { surface, .. } => s
            .surfaces
            .get(*surface)
            .map(|x| x.thickness)
            .ok_or_else(|| "Operand surface outside prescription".into()),
        Operand::EdgeThickness {
            surface, height, ..
        } => {
            let i = *surface;
            let curr = s
                .surfaces
                .get(i)
                .ok_or("Operand surface outside prescription")?;
            let h = height.unwrap_or_else(|| {
                let next_sd = s
                    .surfaces
                    .get(i + 1)
                    .map_or(s.image_surface.semi_diameter, |n| n.semi_diameter);
                curr.semi_diameter.max(next_sd).max(curr.aperture * 0.5)
            });
            let z1 = sag(curr, h)?.0;
            let z2 = if let Some(next) = s.surfaces.get(i + 1) {
                sag(next, h)?.0
            } else {
                s.image_surface.sag(h)?.0
            };
            Ok(curr.thickness + z2 - z1)
        }
        Operand::Distortion {
            field_index,
            wavelength,
            ..
        } => {
            let field = s
                .fields
                .get(*field_index)
                .ok_or("Operand field_index outside prescription")?;
            let w = wavelength.unwrap_or_else(|| s.reference());
            analysis::distortion(s, field, w)?["percent"]
                .as_f64()
                .ok_or_else(|| "Undefined distortion".into())
        }
        Operand::AxialColor { wave1, wave2, .. } => {
            let f1 = s.paraxial(*wave1)?["image_distance"]
                .as_f64()
                .ok_or("Undefined focus at wave1")?;
            let f2 = s.paraxial(*wave2)?["image_distance"]
                .as_f64()
                .ok_or("Undefined focus at wave2")?;
            Ok(f1 - f2)
        }
        Operand::LateralColor {
            field_index,
            wave1,
            wave2,
            ..
        } => {
            let field = s
                .fields
                .get(*field_index)
                .ok_or("Operand field_index outside prescription")?;
            let r1 = s.trace_field(0., 0., field, *wave1)?.image();
            let r2 = s.trace_field(0., 0., field, *wave2)?.image();
            Ok((r1[0] - r2[0]).hypot(r1[1] - r2[1]))
        }
        Operand::ChiefRayAngle {
            field_index,
            wavelength,
            ..
        } => {
            let field = s
                .fields
                .get(*field_index)
                .ok_or("Operand field_index outside prescription")?;
            let w = wavelength.unwrap_or_else(|| s.reference());
            let ray = s.trace_field(0., 0., field, w)?;
            let dir = ray.direction;
            let angle = dir[0].hypot(dir[1]).atan2(dir[2].abs()).to_degrees();
            Ok(angle)
        }
        Operand::RayIntercept {
            field_index,
            px,
            py,
            wavelength,
            axis,
            ..
        } => {
            let field = s
                .fields
                .get(*field_index)
                .ok_or("Operand field_index outside prescription")?;
            let w = wavelength.unwrap_or_else(|| s.reference());
            let r = s.pupil_radius(w)?;
            let ray = s.trace_field(px * r, py * r, field, w)?;
            let img = ray.image();
            Ok(match axis {
                Axis2::X => img[0],
                Axis2::Y => img[1],
            })
        }
    }
}

fn evaluate_all(
    base: &System,
    operands: &[Operand],
) -> Result<(Vec<f64>, Vec<f64>, f64), String> {
    // Cache resolved configurations used by operands.
    let mut resolved_map: std::collections::HashMap<usize, System> =
        std::collections::HashMap::new();
    let default_cfg = base.active_configuration;
    for op in operands {
        let cfg_idx = operand_meta(op).4.unwrap_or(default_cfg);
        if let std::collections::hash_map::Entry::Vacant(e) = resolved_map.entry(cfg_idx) {
            let mut sys = base.configured(cfg_idx)?;
            sys.update()?;
            e.insert(sys);
        }
    }
    let mut values = Vec::with_capacity(operands.len());
    let mut residuals = Vec::with_capacity(operands.len());
    let mut sum_sq = 0.;
    let mut sum_w2 = 0.;
    for op in operands {
        let (_, target, weight, mode, cfg_opt) = operand_meta(op);
        let cfg_idx = cfg_opt.unwrap_or(default_cfg);
        let sys = &resolved_map[&cfg_idx];
        let val = evaluate_operand(sys, op)?;
        if !val.is_finite() {
            return Err("Nonfinite operand value".into());
        }
        let diff = match mode {
            ConstraintMode::Eq => val - target,
            ConstraintMode::Lt => (val - target).max(0.),
            ConstraintMode::Gt => (target - val).max(0.),
        };
        let res = weight * diff;
        values.push(val);
        residuals.push(res);
        sum_sq += res * res;
        sum_w2 += weight * weight;
    }
    let merit = if sum_w2 > 0. {
        (sum_sq / sum_w2).sqrt()
    } else {
        0.
    };
    Ok((values, residuals, merit))
}

fn solve_linear(mut a: Vec<Vec<f64>>, mut b: Vec<f64>) -> Result<Vec<f64>, String> {
    let n = b.len();
    for col in 0..n {
        let mut pivot = col;
        let mut best = a[col][col].abs();
        for (row, r_vals) in a.iter().enumerate().skip(col + 1) {
            let v = r_vals[col].abs();
            if v > best {
                best = v;
                pivot = row;
            }
        }
        if best < 1e-18 || !best.is_finite() {
            return Err("Singular normal matrix".into());
        }
        if pivot != col {
            a.swap(col, pivot);
            b.swap(col, pivot);
        }
        let diag = a[col][col];
        for row in (col + 1)..n {
            let factor = a[row][col] / diag;
            a[row][col] = 0.;
            for k in (col + 1)..n {
                a[row][k] -= factor * a[col][k];
            }
            b[row] -= factor * b[col];
        }
    }
    let mut x = vec![0.; n];
    for row in (0..n).rev() {
        let mut sum = b[row];
        for k in (row + 1)..n {
            sum -= a[row][k] * x[k];
        }
        x[row] = sum / a[row][row];
        if !x[row].is_finite() {
            return Err("Nonfinite linear solve".into());
        }
    }
    Ok(x)
}

pub fn run(source: &System, request: &Request) -> Result<OptimizationResult, String> {
    if request.variables.is_empty() || request.variables.len() > 64 {
        return Err("Optimizer requires 1–64 variables".into());
    }
    if request.operands.is_empty() || request.operands.len() > 256 {
        return Err("Optimizer requires 1–256 operands".into());
    }
    if !(1..=200).contains(&request.max_iterations) {
        return Err("max_iterations must be 1–200".into());
    }
    if !request.initial_damping.is_finite() || request.initial_damping <= 0. {
        return Err("initial_damping must be positive".into());
    }
    if !request.tolerance.is_finite() || request.tolerance <= 0. {
        return Err("tolerance must be positive".into());
    }
    for op in &request.operands {
        let (_, target, weight, _, _) = operand_meta(op);
        if !target.is_finite() || !weight.is_finite() || weight < 0. {
            return Err("Invalid operand target or weight".into());
        }
    }
    if request.operands.iter().all(|op| operand_meta(op).2 == 0.) {
        return Err("At least one positive operand weight is required".into());
    }

    let mut current = source.clone();
    current.update()?;
    let n_vars = request.variables.len();
    let n_ops = request.operands.len();

    let mut x: Vec<f64> = request
        .variables
        .iter()
        .map(|v| get_var_internal(&current, v))
        .collect::<Result<_, _>>()?;
    let initial_display: Vec<f64> = request
        .variables
        .iter()
        .map(|v| get_var_display(&current, v))
        .collect::<Result<_, _>>()?;

    let (initial_values, mut residuals, initial_merit) = evaluate_all(&current, &request.operands)?;
    let mut merit = initial_merit;
    let mut values = initial_values.clone();
    let mut damping = request.initial_damping;
    let mut history = vec![IterationRecord {
        iteration: 0,
        merit,
        damping,
        accepted: true,
    }];
    let mut converged = merit <= request.tolerance;
    let mut reason = if converged {
        "Initial merit already within tolerance".to_string()
    } else {
        "Reached maximum iterations".to_string()
    };
    let mut completed_iters = 0;

    if !converged {
        for iter in 1..=request.max_iterations {
            completed_iters = iter;
            // Compute finite-difference Jacobian columns in parallel.
            let base_ref = &current;
            let x_ref = &x;
            let res_ref = &residuals;
            let vars_ref = &request.variables;
            let ops_ref = &request.operands;
            let jac_cols: Vec<Result<Vec<f64>, String>> =
                parallel::par_map_index(n_vars, 1, |j| {
                    let h = var_step(&vars_ref[j], x_ref[j]);
                    let mut trial = base_ref.clone();
                    let x_plus = clamp_var_internal(&vars_ref[j], x_ref[j] + h);
                    let mut actual_h = x_plus - x_ref[j];
                    if actual_h.abs() < 1e-14 {
                        let x_minus = clamp_var_internal(&vars_ref[j], x_ref[j] - h);
                        actual_h = x_minus - x_ref[j];
                        if actual_h.abs() < 1e-14 {
                            return Ok(vec![0.; n_ops]);
                        }
                        set_var_internal(&mut trial, &vars_ref[j], x_minus)?;
                    } else {
                        set_var_internal(&mut trial, &vars_ref[j], x_plus)?;
                    }
                    let (_, r_trial, _) = match evaluate_all(&trial, ops_ref) {
                        Ok(out) => out,
                        Err(_) => {
                            // Try opposite finite-difference direction if forward step left valid domain.
                            let mut trial_back = base_ref.clone();
                            let x_alt = clamp_var_internal(&vars_ref[j], x_ref[j] - actual_h);
                            actual_h = x_alt - x_ref[j];
                            if actual_h.abs() < 1e-14 {
                                return Ok(vec![0.; n_ops]);
                            }
                            set_var_internal(&mut trial_back, &vars_ref[j], x_alt)?;
                            evaluate_all(&trial_back, ops_ref)?
                        }
                    };
                    Ok((0..n_ops)
                        .map(|i| (r_trial[i] - res_ref[i]) / actual_h)
                        .collect())
                });

            let mut j_cols = Vec::with_capacity(n_vars);
            for col in jac_cols {
                j_cols.push(col?);
            }

            // Build normal equations: A = J^T J, g = J^T r
            let mut jtj = vec![vec![0.; n_vars]; n_vars];
            let mut g = vec![0.; n_vars];
            for col in 0..n_vars {
                let mut sum_g = 0.;
                for i in 0..n_ops {
                    sum_g += j_cols[col][i] * residuals[i];
                }
                g[col] = sum_g;
                for col2 in col..n_vars {
                    let mut sum = 0.;
                    for i in 0..n_ops {
                        sum += j_cols[col][i] * j_cols[col2][i];
                    }
                    jtj[col][col2] = sum;
                    jtj[col2][col] = sum;
                }
            }

            if g.iter().all(|v| v.abs() < 1e-15) {
                converged = true;
                reason = "Gradient vanished".into();
                break;
            }

            let mut step_accepted = false;
            for _retry in 0..10 {
                let mut a = jtj.clone();
                for idx in 0..n_vars {
                    let diag_scale = a[idx][idx].max(1e-10);
                    a[idx][idx] += damping * diag_scale;
                }
                let rhs: Vec<f64> = g.iter().map(|v| -v).collect();
                let Ok(dx) = solve_linear(a, rhs) else {
                    damping = (damping * 10.).min(1e12);
                    continue;
                };
                let mut candidate = current.clone();
                let mut x_next = vec![0.; n_vars];
                let mut apply_ok = true;
                for idx in 0..n_vars {
                    let val = clamp_var_internal(&request.variables[idx], x[idx] + dx[idx]);
                    x_next[idx] = val;
                    if set_var_internal(&mut candidate, &request.variables[idx], val).is_err() {
                        apply_ok = false;
                        break;
                    }
                }
                if !apply_ok || candidate.update().is_err() {
                    damping = (damping * 5.).min(1e12);
                    continue;
                }
                if let Ok((next_vals, next_res, next_merit)) =
                    evaluate_all(&candidate, &request.operands)
                {
                    if next_merit < merit {
                        let rel = (merit - next_merit) / merit.max(1e-12);
                        current = candidate;
                        x = x_next;
                        values = next_vals;
                        residuals = next_res;
                        merit = next_merit;
                        damping = (damping * 0.3).max(1e-12);
                        history.push(IterationRecord {
                            iteration: iter,
                            merit,
                            damping,
                            accepted: true,
                        });
                        step_accepted = true;
                        if merit <= request.tolerance || rel < request.tolerance {
                            converged = true;
                            reason = "Converged within tolerance".into();
                        }
                        break;
                    }
                }
                damping = (damping * 5.).min(1e12);
            }
            if !step_accepted {
                history.push(IterationRecord {
                    iteration: iter,
                    merit,
                    damping,
                    accepted: false,
                });
                reason = "No further merit improvement found".into();
                break;
            }
            if converged {
                break;
            }
        }
    }

    current.update()?;
    let final_display: Vec<f64> = request
        .variables
        .iter()
        .map(|v| get_var_display(&current, v))
        .collect::<Result<_, _>>()?;
    let variables = request
        .variables
        .iter()
        .enumerate()
        .map(|(i, v)| VariableReport {
            index: i,
            label: variable_label(v),
            before: initial_display[i],
            after: final_display[i],
        })
        .collect();
    let operands = request
        .operands
        .iter()
        .enumerate()
        .map(|(i, op)| {
            let (label, target, weight, mode, _) = operand_meta(op);
            OperandReport {
                index: i,
                label,
                target,
                weight,
                mode,
                before: initial_values[i],
                after: values[i],
                contribution_after: residuals[i] * residuals[i],
            }
        })
        .collect();

    Ok(OptimizationResult {
        system: current,
        initial_merit,
        final_merit: merit,
        iterations: completed_iters,
        converged,
        reason,
        variables,
        operands,
        history,
    })
}

/// High-density diagnostic summary designed for AI agents and quick engineering review.

#[derive(serde::Serialize, schemars::JsonSchema)]
struct ParaxialMetrics {
    efl: Option<f64>,
    bfl: Option<f64>,
    f_number: Option<f64>,
    image_distance: Option<f64>,
    entrance_pupil_radius: Option<f64>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
struct AxialColor {
    min_nm: f64,
    max_nm: f64,
    shift_mm: f64,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
struct ThicknessMinimum {
    surface: usize,
    mm: f64,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
struct DesignField {
    field_index: usize,
    x: f64,
    y: f64,
    weight: f64,
    rms_spot_mm: Option<f64>,
    geo_radius_mm: Option<f64>,
    distortion_percent: Option<f64>,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub(crate) struct DesignSummary {
    name: String,
    surfaces: usize,
    configurations: usize,
    active_configuration: usize,
    has_transforms: bool,
    total_track_mm: f64,
    paraxial: ParaxialMetrics,
    axial_color: Option<AxialColor>,
    min_center_thickness: Option<ThicknessMinimum>,
    min_edge_thickness: Option<ThicknessMinimum>,
    fields: Vec<DesignField>,
}

pub fn design_summary(s: &System) -> Result<Value, String> {
    serde_json::to_value(design_summary_report(s)?).map_err(|e| e.to_string())
}
pub(crate) fn design_summary_report(s: &System) -> Result<DesignSummary, String> {
    let w_ref = s.reference();
    let paraxial = s.paraxial(w_ref)?;
    let samp = PupilSampling::Hexapolar { rings: 4 };
    let mut fields = Vec::with_capacity(s.fields.len());
    for (i, f) in s.fields.iter().enumerate() {
        let spot = analysis::polychromatic_spot(s, f, samp).ok();
        let distortion = analysis::distortion(s, f, w_ref)
            .ok()
            .and_then(|d| d["percent"].as_f64());
        fields.push(DesignField {
            field_index: i,
            x: f.x,
            y: f.y,
            weight: f.weight,
            rms_spot_mm: spot.as_ref().and_then(|v| v["rms_radius"].as_f64()),
            geo_radius_mm: spot.as_ref().and_then(|v| v["geo_radius"].as_f64()),
            distortion_percent: distortion,
        });
    }
    let min_center_thickness = s
        .surfaces
        .iter()
        .enumerate()
        .map(|(i, surf)| (i, surf.thickness))
        .min_by(|a, b| a.1.total_cmp(&b.1));
    let mut min_edge: Option<(usize, f64)> = None;
    for (i, surf) in s.surfaces.iter().enumerate() {
        if surf.interaction == Interaction::Dummy {
            continue;
        }
        let next_sd = s
            .surfaces
            .get(i + 1)
            .map_or(s.image_surface.semi_diameter, |n| n.semi_diameter);
        let h = surf.semi_diameter.max(next_sd);
        if let (Ok((z1, _)), Ok((z2, _))) = (
            sag(surf, h),
            s.surfaces
                .get(i + 1)
                .map_or_else(|| s.image_surface.sag(h), |n| sag(n, h)),
        ) {
            let et = surf.thickness + z2 - z1;
            if min_edge.is_none_or(|(_, best)| et < best) {
                min_edge = Some((i, et));
            }
        }
    }
    let mut axial_color = None;
    if s.wavelengths.len() >= 2 {
        let w_min = s
            .wavelengths
            .iter()
            .map(|w| w.value)
            .fold(f64::INFINITY, f64::min);
        let w_max = s
            .wavelengths
            .iter()
            .map(|w| w.value)
            .fold(f64::NEG_INFINITY, f64::max);
        if let (Ok(p1), Ok(p2)) = (s.paraxial(w_min), s.paraxial(w_max)) {
            if let (Some(f1), Some(f2)) = (p1["image_distance"].as_f64(), p2["image_distance"].as_f64())
            {
                axial_color = Some(AxialColor {
                    min_nm: w_min,
                    max_nm: w_max,
                    shift_mm: f1 - f2,
                });
            }
        }
    }
    Ok(DesignSummary {
        name: s.name.clone(),
        surfaces: s.surfaces.len(),
        configurations: s.configurations.len(),
        active_configuration: s.active_configuration,
        has_transforms: s.has_transforms(),
        total_track_mm: s.surfaces.iter().map(|x| x.thickness).sum::<f64>(),
        paraxial: ParaxialMetrics {
            efl: paraxial["efl"].as_f64(),
            bfl: paraxial["bfl"].as_f64(),
            f_number: paraxial["f_number"].as_f64(),
            image_distance: paraxial["image_distance"].as_f64(),
            entrance_pupil_radius: paraxial["entrance_pupil_radius"].as_f64(),
        },
        axial_color,
        min_center_thickness: min_center_thickness
            .map(|(i, v)| ThicknessMinimum { surface: i, mm: v }),
        min_edge_thickness: min_edge.map(|(i, v)| ThicknessMinimum { surface: i, mm: v }),
        fields,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dls_optimizer_improves_singlet_spot_and_hits_target_efl() {
        let mut s = System::default();
        s.update().unwrap();
        let req = Request {
            variables: vec![
                Variable::Radius {
                    surface: 0,
                    min: Some(15.),
                    max: Some(500.),
                },
                Variable::Radius {
                    surface: 1,
                    min: Some(-500.),
                    max: Some(-15.),
                },
                Variable::Thickness {
                    surface: 1,
                    min: Some(20.),
                    max: Some(150.),
                },
            ],
            operands: vec![
                Operand::Efl {
                    wavelength: None,
                    target: 60.,
                    weight: 10.,
                    mode: ConstraintMode::Eq,
                    configuration: None,
                },
                Operand::RmsSpot {
                    field_index: Some(0),
                    wavelength: Some(587.5618),
                    sampling: Some(PupilSampling::Hexapolar { rings: 3 }),
                    target: 0.,
                    weight: 50.,
                    mode: ConstraintMode::Eq,
                    configuration: None,
                },
            ],
            max_iterations: 25,
            initial_damping: 1e-3,
            tolerance: 1e-7,
        };
        let res = run(&s, &req).unwrap();
        assert!(res.final_merit < res.initial_merit * 0.2, "merit: {} -> {}", res.initial_merit, res.final_merit);
        let efl = res.system.paraxial(res.system.reference()).unwrap()["efl"]
            .as_f64()
            .unwrap();
        assert!((efl - 60.).abs() < 0.05, "efl = {efl}");
    }

    #[test]
    fn inequality_constraint_enforces_minimum_center_thickness() {
        let mut s = System::default();
        s.surfaces[0].thickness = 1.0;
        s.update().unwrap();
        let req = Request {
            variables: vec![Variable::Thickness {
                surface: 0,
                min: Some(0.5),
                max: Some(20.),
            }],
            operands: vec![Operand::CenterThickness {
                surface: 0,
                target: 4.0,
                weight: 10.,
                mode: ConstraintMode::Gt,
                configuration: None,
            }],
            max_iterations: 15,
            initial_damping: 1e-3,
            tolerance: 1e-7,
        };
        let res = run(&s, &req).unwrap();
        assert!(res.system.surfaces[0].thickness >= 3.99);
    }
}
