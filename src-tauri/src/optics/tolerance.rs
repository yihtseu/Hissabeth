// SPDX-License-Identifier: GPL-3.0-or-later
// Sequential optical tolerancing: sensitivity table and parallel Monte Carlo yield analysis.
use super::{CoordinateTransform, PupilSampling, System, analysis, material, parallel, solve};
use serde::{Deserialize, Serialize};

#[derive(schemars::JsonSchema, Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToleranceOperand {
    Radius {
        #[schemars(range(min = 0, max = 127))]
        surface: usize,
        #[serde(default)]
        #[schemars(range(min = 1e-09, max = 90.0))]
        delta_percent: Option<f64>,
        #[serde(default)]
        #[schemars(range(min = 1e-09, max = 1e6))]
        delta_mm: Option<f64>,
    },
    Thickness {
        #[schemars(range(min = 0, max = 127))]
        surface: usize,
        #[schemars(range(min = 1e-09, max = 10000.0))]
        delta: f64,
    },
    Decenter {
        #[schemars(range(min = 0, max = 127))]
        surface: usize,
        #[schemars(range(min = 1e-09, max = 10000.0))]
        delta: f64,
    },
    Tilt {
        #[schemars(range(min = 0, max = 127))]
        surface: usize,
        #[schemars(range(min = 1e-09, max = 45.0))]
        delta_deg: f64,
    },
    Index {
        #[schemars(range(min = 0, max = 127))]
        surface: usize,
        #[schemars(range(min = 1e-09, max = 0.5))]
        delta: f64,
    },
    Abbe {
        #[schemars(range(min = 0, max = 127))]
        surface: usize,
        #[schemars(range(min = 1e-09, max = 50.0))]
        delta_percent: f64,
    },
    ElementDecenter {
        #[schemars(range(min = 0, max = 127))]
        first: usize,
        #[schemars(range(min = 0, max = 127))]
        last: usize,
        #[schemars(range(min = 1e-09, max = 10000.0))]
        delta: f64,
    },
    ElementTilt {
        #[schemars(range(min = 0, max = 127))]
        first: usize,
        #[schemars(range(min = 0, max = 127))]
        last: usize,
        #[schemars(range(min = 1e-09, max = 45.0))]
        delta_deg: f64,
    },
}

#[derive(schemars::JsonSchema, Clone, Debug, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Compensator {
    #[default]
    None,
    Autofocus {
        #[serde(default)]
        #[schemars(range(min = 100.0, max = 20000.0))]
        wavelength: Option<f64>,
        #[serde(default)]
        #[schemars(range(min = 0.0, max = 1e5))]
        max_delta: Option<f64>,
    },
}

#[derive(schemars::JsonSchema, Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Criterion {
    RmsSpot {
        #[serde(default)]
        #[schemars(range(min = 0, max = 63))]
        field_index: Option<usize>,
        #[serde(default)]
        #[schemars(range(min = 100.0, max = 20000.0))]
        wavelength: Option<f64>,
        #[serde(default)]
        sampling: Option<PupilSampling>,
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
    },
    Efl {
        #[serde(default)]
        #[schemars(range(min = 100.0, max = 20000.0))]
        wavelength: Option<f64>,
    },
    GeometricMtf {
        #[serde(default)]
        #[schemars(range(min = 0, max = 63))]
        field_index: usize,
        #[schemars(range(min = 1e-06, max = 10000.0))]
        frequency: f64,
        #[serde(default)]
        #[schemars(extend("enum" = ["x", "y", null]))]
        axis: Option<String>,
        #[serde(default)]
        #[schemars(range(min = 100.0, max = 20000.0))]
        wavelength: Option<f64>,
        #[serde(default)]
        sampling: Option<PupilSampling>,
    },
}

#[derive(
    schemars::JsonSchema, Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Distribution {
    #[default]
    Normal,
    Uniform,
}

#[derive(schemars::JsonSchema, Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SensitivityRequest {
    #[schemars(length(min = 1, max = 256))]
    pub tolerances: Vec<ToleranceOperand>,
    #[serde(default)]
    pub compensator: Compensator,
    pub criterion: Criterion,
}

#[derive(schemars::JsonSchema, Clone, Debug, Serialize)]
pub struct SensitivityEntry {
    pub operand_index: usize,
    pub label: String,
    pub axis: String,
    pub plus_value: Option<f64>,
    pub minus_value: Option<f64>,
    pub plus_change: Option<f64>,
    pub minus_change: Option<f64>,
    pub worst_change: f64,
    pub plus_error: Option<String>,
    pub minus_error: Option<String>,
}

#[derive(schemars::JsonSchema, Clone, Debug, Serialize)]
pub struct SensitivityResult {
    pub nominal: f64,
    pub criterion_unit: String,
    pub entries: Vec<SensitivityEntry>,
    pub rss_change: f64,
    pub estimated_performance: f64,
}

fn default_trials() -> usize {
    100
}
fn default_seed() -> u64 {
    42
}

#[derive(schemars::JsonSchema, Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MonteCarloRequest {
    #[schemars(length(min = 1, max = 256))]
    pub tolerances: Vec<ToleranceOperand>,
    #[serde(default)]
    pub compensator: Compensator,
    pub criterion: Criterion,
    #[serde(default = "default_trials")]
    #[schemars(range(min = 10, max = 2000))]
    pub trials: usize,
    #[serde(default)]
    pub distribution: Distribution,
    #[serde(default = "default_seed")]
    #[schemars(range(min = 0, max = 9_007_199_254_740_991_u64))]
    pub seed: u64,
    #[serde(default)]
    #[schemars(range(min = -1e12, max = 1e12))]
    pub pass_threshold: Option<f64>,
    #[serde(default)]
    pub lower_is_better: Option<bool>,
}

#[derive(schemars::JsonSchema, Clone, Debug, Serialize)]
pub struct HistogramBin {
    pub low: f64,
    pub high: f64,
    pub count: usize,
}

#[derive(schemars::JsonSchema, Clone, Debug, Serialize)]
pub struct MonteCarloResult {
    pub nominal: f64,
    pub criterion_unit: String,
    pub trials: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub mean: Option<f64>,
    pub std_dev: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub p50: Option<f64>,
    pub p80: Option<f64>,
    pub p90: Option<f64>,
    pub p95: Option<f64>,
    pub p98: Option<f64>,
    pub pass_rate: Option<f64>,
    pub compensator_range: Option<[f64; 2]>,
    pub histogram: Vec<HistogramBin>,
}

fn validate_tolerances(s: &System, list: &[ToleranceOperand]) -> Result<(), String> {
    if list.is_empty() || list.len() > 256 {
        return Err("Provide 1–256 tolerance operands".into());
    }
    let n = s.surfaces.len();
    for tol in list {
        match *tol {
            ToleranceOperand::Radius {
                surface,
                delta_percent,
                delta_mm,
            } => {
                if surface >= n {
                    return Err("Tolerance surface outside prescription".into());
                }
                if delta_percent.is_none() && delta_mm.is_none() {
                    return Err("Radius tolerance requires delta_percent or delta_mm".into());
                }
                if let Some(p) = delta_percent
                    && (!p.is_finite() || p <= 0. || p >= 90.)
                {
                    return Err("Radius delta_percent must be in (0, 90)".into());
                }
                if let Some(d) = delta_mm
                    && (!d.is_finite() || d <= 0.)
                {
                    return Err("Radius delta_mm must be positive".into());
                }
            }
            ToleranceOperand::Thickness { surface, delta }
            | ToleranceOperand::Decenter { surface, delta }
            | ToleranceOperand::Index { surface, delta } => {
                if surface >= n || !delta.is_finite() || delta <= 0. {
                    return Err("Invalid tolerance surface or delta".into());
                }
            }
            ToleranceOperand::Tilt { surface, delta_deg } => {
                if surface >= n || !delta_deg.is_finite() || delta_deg <= 0. || delta_deg > 45. {
                    return Err("Tilt tolerance must be in (0, 45] deg".into());
                }
            }
            ToleranceOperand::Abbe {
                surface,
                delta_percent,
            } => {
                if surface >= n
                    || !delta_percent.is_finite()
                    || delta_percent <= 0.
                    || delta_percent >= 50.
                {
                    return Err("Abbe delta_percent must be in (0, 50)".into());
                }
            }
            ToleranceOperand::ElementDecenter { first, last, delta } => {
                if first > last || last >= n || !delta.is_finite() || delta <= 0. {
                    return Err("Invalid ElementDecenter range or delta".into());
                }
            }
            ToleranceOperand::ElementTilt {
                first,
                last,
                delta_deg,
            } => {
                if first > last
                    || last >= n
                    || !delta_deg.is_finite()
                    || delta_deg <= 0.
                    || delta_deg > 45.
                {
                    return Err("Invalid ElementTilt range or delta_deg".into());
                }
            }
        }
    }
    Ok(())
}

fn glass_nd_vd(s: &System, surface: usize) -> Result<(f64, f64), String> {
    let mat = &s.surfaces[surface].material;
    if mat == "AIR" {
        return Err(format!(
            "Surface {surface} is AIR; cannot perturb glass index/Abbe"
        ));
    }
    if let Some((nd, vd)) = mat.split_once(':') {
        let nd: f64 = nd.trim().parse().map_err(|_| "Invalid model glass nd")?;
        let vd: f64 = vd.trim().parse().map_err(|_| "Invalid model glass vd")?;
        return Ok((nd, vd));
    }
    let g = s
        .custom_glasses
        .iter()
        .find(|g| g.key() == *mat || g.name == *mat)
        .cloned()
        .or_else(|| material::find(mat).ok())
        .ok_or_else(|| format!("Glass {mat} on surface {surface} not found"))?;
    Ok((g.nd, g.vd))
}

fn add_local_perturbation(s: &mut System, surface: usize, dx: f64, dy: f64, rx: f64, ry: f64) {
    let tr = s.surfaces[surface]
        .transform
        .get_or_insert(CoordinateTransform {
            decenter: [0., 0.],
            tilt: [0., 0., 0.],
            tilt_first: false,
            return_after: true,
        });
    tr.decenter[0] += dx;
    tr.decenter[1] += dy;
    tr.tilt[0] += rx;
    tr.tilt[1] += ry;
}

/// Perturbation degrees of freedom expanded from a ToleranceOperand (axis label + apply closure).
fn apply_single_perturbation(
    s: &mut System,
    tol: &ToleranceOperand,
    axis: usize,
    factor: f64,
) -> Result<(), String> {
    match *tol {
        ToleranceOperand::Radius {
            surface,
            delta_percent,
            delta_mm,
        } => {
            let r = s.surfaces[surface].radius;
            if let Some(pct) = delta_percent {
                if r != 0. {
                    let c = (1. / r) * (1. + factor * pct / 100.);
                    s.surfaces[surface].radius = if c.abs() < 1e-12 { 0. } else { 1. / c };
                }
            } else if let Some(d) = delta_mm {
                s.surfaces[surface].radius = r + factor * d;
            }
        }
        ToleranceOperand::Thickness { surface, delta } => {
            s.surfaces[surface].thickness += factor * delta;
        }
        ToleranceOperand::Decenter { surface, delta } => {
            if axis == 0 {
                add_local_perturbation(s, surface, factor * delta, 0., 0., 0.);
            } else {
                add_local_perturbation(s, surface, 0., factor * delta, 0., 0.);
            }
        }
        ToleranceOperand::Tilt { surface, delta_deg } => {
            if axis == 0 {
                add_local_perturbation(s, surface, 0., 0., factor * delta_deg, 0.);
            } else {
                add_local_perturbation(s, surface, 0., 0., 0., factor * delta_deg);
            }
        }
        ToleranceOperand::Index { surface, delta } => {
            let (nd, vd) = glass_nd_vd(s, surface)?;
            let next_nd = (nd + factor * delta).max(1.01);
            s.surfaces[surface].material = format!("{next_nd:.8}:{vd:.6}");
        }
        ToleranceOperand::Abbe {
            surface,
            delta_percent,
        } => {
            let (nd, vd) = glass_nd_vd(s, surface)?;
            let next_vd = (vd * (1. + factor * delta_percent / 100.)).max(5.);
            s.surfaces[surface].material = format!("{nd:.8}:{next_vd:.6}");
        }
        ToleranceOperand::ElementDecenter { first, last, delta } => {
            for idx in first..=last {
                if axis == 0 {
                    add_local_perturbation(s, idx, factor * delta, 0., 0., 0.);
                } else {
                    add_local_perturbation(s, idx, 0., factor * delta, 0., 0.);
                }
            }
        }
        ToleranceOperand::ElementTilt {
            first,
            last,
            delta_deg,
        } => {
            for idx in first..=last {
                if axis == 0 {
                    add_local_perturbation(s, idx, 0., 0., factor * delta_deg, 0.);
                } else {
                    add_local_perturbation(s, idx, 0., 0., 0., factor * delta_deg);
                }
            }
        }
    }
    Ok(())
}

fn operand_axes(tol: &ToleranceOperand) -> Vec<(usize, String, String)> {
    match *tol {
        ToleranceOperand::Radius { surface, .. } => {
            vec![(0, format!("S{} Radius", surface + 1), "R".into())]
        }
        ToleranceOperand::Thickness { surface, .. } => {
            vec![(0, format!("S{} Thickness", surface + 1), "Z".into())]
        }
        ToleranceOperand::Decenter { surface, .. } => vec![
            (0, format!("S{} Decenter", surface + 1), "X".into()),
            (1, format!("S{} Decenter", surface + 1), "Y".into()),
        ],
        ToleranceOperand::Tilt { surface, .. } => vec![
            (0, format!("S{} Tilt", surface + 1), "RX".into()),
            (1, format!("S{} Tilt", surface + 1), "RY".into()),
        ],
        ToleranceOperand::Index { surface, .. } => {
            vec![(0, format!("S{} Index", surface + 1), "nd".into())]
        }
        ToleranceOperand::Abbe { surface, .. } => {
            vec![(0, format!("S{} Abbe", surface + 1), "vd".into())]
        }
        ToleranceOperand::ElementDecenter { first, last, .. } => vec![
            (
                0,
                format!("Elem S{}-S{} Decenter", first + 1, last + 1),
                "X".into(),
            ),
            (
                1,
                format!("Elem S{}-S{} Decenter", first + 1, last + 1),
                "Y".into(),
            ),
        ],
        ToleranceOperand::ElementTilt { first, last, .. } => vec![
            (
                0,
                format!("Elem S{}-S{} Tilt", first + 1, last + 1),
                "RX".into(),
            ),
            (
                1,
                format!("Elem S{}-S{} Tilt", first + 1, last + 1),
                "RY".into(),
            ),
        ],
    }
}

fn apply_compensator(s: &mut System, comp: &Compensator) -> Result<Option<f64>, String> {
    match *comp {
        Compensator::None => Ok(None),
        Compensator::Autofocus {
            wavelength,
            max_delta,
        } => {
            let before = s.surfaces.last().ok_or("Empty prescription")?.thickness;
            let w = wavelength.unwrap_or_else(|| s.reference());
            let focused = solve::autofocus(s, w)?;
            let after = focused.surfaces.last().unwrap().thickness;
            let mut delta = after - before;
            if let Some(limit) = max_delta
                && limit.is_finite()
                && limit >= 0.
            {
                delta = delta.clamp(-limit, limit);
                s.surfaces.last_mut().unwrap().thickness = before + delta;
                s.update()?;
                return Ok(Some(delta));
            }
            *s = focused;
            Ok(Some(delta))
        }
    }
}

fn criterion_unit(c: &Criterion) -> &'static str {
    match c {
        Criterion::RmsSpot { .. } | Criterion::Efl { .. } => "mm",
        Criterion::RmsWavefront { .. } => "waves",
        Criterion::GeometricMtf { .. } => "contrast",
    }
}

fn criterion_default_lower_is_better(c: &Criterion) -> bool {
    !matches!(c, Criterion::GeometricMtf { .. })
}

fn evaluate_criterion(s: &System, criterion: &Criterion) -> Result<f64, String> {
    let default_sampling = PupilSampling::Hexapolar { rings: 3 };
    match criterion {
        Criterion::RmsSpot {
            field_index,
            wavelength,
            sampling,
        } => {
            let samp = sampling.unwrap_or(default_sampling);
            let eval_one = |fi: usize| -> Result<f64, String> {
                let field = s
                    .fields
                    .get(fi)
                    .ok_or("Criterion field_index outside prescription")?;
                match wavelength {
                    Some(w) => s.spot_field(field, *w, samp)?["rms_radius"]
                        .as_f64()
                        .ok_or_else(|| "Missing RMS spot radius".into()),
                    None => analysis::polychromatic_spot(s, field, samp)?["rms_radius"]
                        .as_f64()
                        .ok_or_else(|| "Missing polychromatic RMS spot".into()),
                }
            };
            if let Some(fi) = *field_index {
                eval_one(fi)
            } else {
                let mut sw = 0.;
                let mut sq = 0.;
                for (fi, f) in s.fields.iter().enumerate() {
                    if f.weight <= 0. {
                        continue;
                    }
                    let r = eval_one(fi)?;
                    sq += f.weight * r * r;
                    sw += f.weight;
                }
                if sw <= 0. {
                    return Err("No positive-weight fields".into());
                }
                Ok((sq / sw).sqrt())
            }
        }
        Criterion::RmsWavefront {
            field_index,
            wavelength,
            grid,
        } => {
            let field = s
                .fields
                .get(*field_index)
                .ok_or("Criterion field_index outside prescription")?;
            let w = wavelength.unwrap_or_else(|| s.reference());
            analysis::wavefront(s, field, w, grid.unwrap_or(11))?.json()["rms_waves"]
                .as_f64()
                .ok_or_else(|| "Missing RMS wavefront".into())
        }
        Criterion::Efl { wavelength } => {
            let w = wavelength.unwrap_or_else(|| s.reference());
            s.paraxial(w)?["efl"]
                .as_f64()
                .ok_or_else(|| "Undefined EFL".into())
        }
        Criterion::GeometricMtf {
            field_index,
            frequency,
            axis,
            wavelength,
            sampling,
        } => {
            if !frequency.is_finite() || *frequency <= 0. {
                return Err("MTF frequency must be positive".into());
            }
            let field = s
                .fields
                .get(*field_index)
                .ok_or("Criterion field_index outside prescription")?;
            let samp = sampling.unwrap_or(default_sampling);
            let spec = match wavelength {
                Some(w) => super::Spectrum::Monochromatic { wavelength: *w },
                None => super::Spectrum::Polychromatic {},
            };
            let (bundle, _) = s.sample_spectrum(field, samp, spec)?;
            let origin = bundle.centroid()?;
            let total = bundle.transmitted_weight();
            let ax = if axis.as_deref() == Some("x") { 0 } else { 1 };
            let (re, im) =
                bundle
                    .points
                    .iter()
                    .zip(&bundle.weights)
                    .fold((0., 0.), |(re, im), (pt, wt)| {
                        let angle = std::f64::consts::TAU * frequency * (pt[ax] - origin[ax]);
                        let (sin, cos) = angle.sin_cos();
                        (re + wt * cos / total, im - wt * sin / total)
                    });
            Ok(re.hypot(im))
        }
    }
}

pub fn sensitivity(source: &System, req: &SensitivityRequest) -> Result<SensitivityResult, String> {
    let mut base = source.clone();
    base.update()?;
    validate_tolerances(&base, &req.tolerances)?;
    let nominal = evaluate_criterion(&base, &req.criterion)?;

    let mut tasks: Vec<(usize, usize, String, String)> = Vec::new();
    for (idx, tol) in req.tolerances.iter().enumerate() {
        for (axis, label, axis_name) in operand_axes(tol) {
            tasks.push((idx, axis, label, axis_name));
        }
    }

    let entries_raw: Vec<SensitivityEntry> =
        parallel::par_map(&tasks, 1, |&(idx, axis, ref label, ref axis_name)| {
            let tol = &req.tolerances[idx];
            let eval_dir = |sign: f64| -> Result<f64, String> {
                let mut trial = base.clone();
                apply_single_perturbation(&mut trial, tol, axis, sign)?;
                trial.update()?;
                apply_compensator(&mut trial, &req.compensator)?;
                evaluate_criterion(&trial, &req.criterion)
            };
            let plus = eval_dir(1.);
            let minus = eval_dir(-1.);
            let (plus_value, plus_error) = match plus {
                Ok(v) => (Some(v), None),
                Err(e) => (None, Some(e)),
            };
            let (minus_value, minus_error) = match minus {
                Ok(v) => (Some(v), None),
                Err(e) => (None, Some(e)),
            };
            let plus_change = plus_value.map(|v| v - nominal);
            let minus_change = minus_value.map(|v| v - nominal);
            let worst_change = [plus_change, minus_change]
                .into_iter()
                .flatten()
                .map(f64::abs)
                .fold(0., f64::max);
            SensitivityEntry {
                operand_index: idx,
                label: label.clone(),
                axis: axis_name.clone(),
                plus_value,
                minus_value,
                plus_change,
                minus_change,
                worst_change,
                plus_error,
                minus_error,
            }
        });

    let mut entries = entries_raw;
    entries.sort_by(|a, b| b.worst_change.total_cmp(&a.worst_change));
    let rss_change = entries
        .iter()
        .map(|e| e.worst_change * e.worst_change)
        .sum::<f64>()
        .sqrt();
    let lower_better = criterion_default_lower_is_better(&req.criterion);
    let estimated_performance = if lower_better {
        nominal + rss_change
    } else {
        (nominal - rss_change).max(0.)
    };

    Ok(SensitivityResult {
        nominal,
        criterion_unit: criterion_unit(&req.criterion).into(),
        entries,
        rss_change,
        estimated_performance,
    })
}

/// Deterministic SplitMix64 PRNG for reproducible Monte Carlo trials across parallel threads.
struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed)
    }
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    /// Uniform in (0, 1).
    fn uniform01(&mut self) -> f64 {
        ((self.next_u64() >> 11) as f64 + 0.5) * (1.0 / ((1u64 << 53) as f64))
    }
    /// Standard normal N(0, 1) via Box-Muller.
    fn normal01(&mut self) -> f64 {
        let u1 = self.uniform01().max(1e-15);
        let u2 = self.uniform01();
        (-2. * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
    }
    /// Draw factor in approximately [-1, 1] (Normal uses 2-sigma = 1.0, clamped to ±1.5).
    fn factor(&mut self, dist: Distribution) -> f64 {
        match dist {
            Distribution::Uniform => 2. * self.uniform01() - 1.,
            Distribution::Normal => (self.normal01() * 0.5).clamp(-1.5, 1.5),
        }
    }
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.;
    }
    let idx = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

pub fn monte_carlo(source: &System, req: &MonteCarloRequest) -> Result<MonteCarloResult, String> {
    if !(10..=2000).contains(&req.trials) {
        return Err("Monte Carlo trials must be 10–2000".into());
    }
    let mut base = source.clone();
    base.update()?;
    validate_tolerances(&base, &req.tolerances)?;
    let nominal = evaluate_criterion(&base, &req.criterion)?;

    let outcomes: Vec<Result<(f64, Option<f64>), String>> =
        parallel::par_map_index(req.trials, 4, |trial_idx| {
            let mut rng = Rng::new(
                req.seed.wrapping_mul(0x517c_c1b7_2722_0a95).wrapping_add(
                    (trial_idx as u64)
                        .wrapping_add(1)
                        .wrapping_mul(0x9e37_79b9_7f4a_7c15),
                ),
            );
            let mut trial = base.clone();
            for tol in &req.tolerances {
                for (axis, _, _) in operand_axes(tol) {
                    let f = rng.factor(req.distribution);
                    apply_single_perturbation(&mut trial, tol, axis, f)?;
                }
            }
            trial.update()?;
            let comp = apply_compensator(&mut trial, &req.compensator)?;
            let val = evaluate_criterion(&trial, &req.criterion)?;
            if !val.is_finite() {
                return Err("Nonfinite criterion".into());
            }
            Ok((val, comp))
        });

    let mut values = Vec::new();
    let mut comp_min = f64::INFINITY;
    let mut comp_max = f64::NEG_INFINITY;
    let mut has_comp = false;
    let mut failed = 0usize;
    for res in outcomes {
        match res {
            Ok((v, c)) => {
                values.push(v);
                if let Some(cd) = c {
                    has_comp = true;
                    comp_min = comp_min.min(cd);
                    comp_max = comp_max.max(cd);
                }
            }
            Err(_) => failed += 1,
        }
    }

    let succeeded = values.len();
    if values.is_empty() {
        return Ok(MonteCarloResult {
            nominal,
            criterion_unit: criterion_unit(&req.criterion).into(),
            trials: req.trials,
            succeeded: 0,
            failed,
            mean: None,
            std_dev: None,
            min: None,
            max: None,
            p50: None,
            p80: None,
            p90: None,
            p95: None,
            p98: None,
            pass_rate: req.pass_threshold.map(|_| 0.),
            compensator_range: None,
            histogram: vec![],
        });
    }

    values.sort_by(|a, b| a.total_cmp(b));
    let mean = values.iter().sum::<f64>() / succeeded as f64;
    let var = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / succeeded as f64;
    let min = *values.first().unwrap();
    let max = *values.last().unwrap();
    let lower_better = req
        .lower_is_better
        .unwrap_or_else(|| criterion_default_lower_is_better(&req.criterion));
    let pass_rate = req.pass_threshold.map(|thr| {
        let passed = values
            .iter()
            .filter(|&&v| if lower_better { v <= thr } else { v >= thr })
            .count();
        passed as f64 / req.trials as f64
    });

    // Build 12-bin histogram.
    let bin_count = 12usize;
    let span = (max - min).max(1e-12);
    let width = span / bin_count as f64;
    let mut counts = vec![0usize; bin_count];
    for &v in &values {
        let idx = (((v - min) / width).floor() as usize).min(bin_count - 1);
        counts[idx] += 1;
    }
    let histogram = counts
        .into_iter()
        .enumerate()
        .map(|(i, count)| HistogramBin {
            low: min + i as f64 * width,
            high: min + (i + 1) as f64 * width,
            count,
        })
        .collect();

    Ok(MonteCarloResult {
        nominal,
        criterion_unit: criterion_unit(&req.criterion).into(),
        trials: req.trials,
        succeeded,
        failed,
        mean: Some(mean),
        std_dev: Some(var.sqrt()),
        min: Some(min),
        max: Some(max),
        p50: Some(percentile(&values, 0.50)),
        p80: Some(percentile(&values, 0.80)),
        p90: Some(percentile(&values, 0.90)),
        p95: Some(percentile(&values, 0.95)),
        p98: Some(percentile(&values, 0.98)),
        pass_rate,
        compensator_range: has_comp.then_some([comp_min, comp_max]),
        histogram,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sensitivity_and_monte_carlo_run_reproducibly() {
        let mut s = System::default();
        s.update().unwrap();
        let tolerances = vec![
            ToleranceOperand::Radius {
                surface: 0,
                delta_percent: Some(0.5),
                delta_mm: None,
            },
            ToleranceOperand::Thickness {
                surface: 0,
                delta: 0.1,
            },
            ToleranceOperand::Decenter {
                surface: 0,
                delta: 0.05,
            },
            ToleranceOperand::Tilt {
                surface: 0,
                delta_deg: 0.1,
            },
            ToleranceOperand::Index {
                surface: 0,
                delta: 0.001,
            },
        ];
        let sens = sensitivity(
            &s,
            &SensitivityRequest {
                tolerances: tolerances.clone(),
                compensator: Compensator::Autofocus {
                    wavelength: None,
                    max_delta: Some(2.0),
                },
                criterion: Criterion::RmsSpot {
                    field_index: Some(0),
                    wavelength: Some(587.5618),
                    sampling: Some(PupilSampling::Hexapolar { rings: 3 }),
                },
            },
        )
        .unwrap();
        assert_eq!(sens.entries.len(), 7); // R, T, DX, DY, RX, RY, nd
        assert!(sens.rss_change >= 0.);

        let mc_req = MonteCarloRequest {
            tolerances,
            compensator: Compensator::Autofocus {
                wavelength: None,
                max_delta: Some(2.0),
            },
            criterion: Criterion::RmsSpot {
                field_index: Some(0),
                wavelength: Some(587.5618),
                sampling: Some(PupilSampling::Hexapolar { rings: 3 }),
            },
            trials: 30,
            distribution: Distribution::Normal,
            seed: 1234,
            pass_threshold: Some(0.2),
            lower_is_better: Some(true),
        };
        let mc1 = monte_carlo(&s, &mc_req).unwrap();
        let mc2 = monte_carlo(&s, &mc_req).unwrap();
        assert_eq!(mc1.succeeded, 30);
        assert_eq!(mc1.mean, mc2.mean);
        assert_eq!(mc1.p90, mc2.p90);
        assert!(mc1.pass_rate.unwrap() >= 0. && mc1.pass_rate.unwrap() <= 1.);
    }
}
