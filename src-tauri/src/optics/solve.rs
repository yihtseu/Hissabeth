// SPDX-License-Identifier: GPL-3.0-or-later
// Persistent gap constraints derived from Geopter's solve subsystem.
use super::*;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum GapSolve {
    Fixed {},
    Pickup {
        source: usize,
        scale: f64,
        offset: f64,
    },
    EdgeThickness {
        height: f64,
        value: f64,
    },
    OverallLength {
        first: usize,
        last: usize,
        value: f64,
    },
    MarginalHeight {
        value: f64,
        zone: f64,
    },
}
impl GapSolve {
    pub fn is_fixed(&self) -> bool {
        matches!(self, Self::Fixed {})
    }
    fn is_linear(&self) -> bool {
        !matches!(self, Self::MarginalHeight { .. })
    }
    pub fn validate(&self, system: &System, gap: usize) -> Result<(), String> {
        let finite = |values: &[f64]| values.iter().all(|v| v.is_finite());
        let valid = match *self {
            Self::Fixed {} => true,
            Self::Pickup {
                source,
                scale,
                offset,
            } => source < system.surfaces.len() && source != gap && finite(&[scale, offset]),
            Self::EdgeThickness { height, value } => {
                gap + 1 < system.surfaces.len()
                    && finite(&[height, value])
                    && height >= 0.
                    && value >= 0.
            }
            Self::OverallLength { first, last, value } => {
                first <= gap && gap < last && last <= system.surfaces.len() && value.is_finite()
            }
            Self::MarginalHeight { value, zone } => finite(&[value, zone]),
        };
        if valid {
            Ok(())
        } else {
            Err(format!("Invalid solve definition for gap {gap}"))
        }
    }
}

#[derive(Clone, Copy)]
struct Equation {
    gap: usize,
    // Autofocus adds a temporary equation to the freely editable final gap.
    focus: Option<f64>,
}
impl Equation {
    fn is_linear(self, s: &System) -> bool {
        self.focus.is_none() && s.surfaces[self.gap].gap_solve.is_linear()
    }
    fn residual(self, s: &System) -> Result<f64, String> {
        self.evaluate(s)
            .map_err(|e| format!("Gap {}: {e}", self.gap))
    }
    fn evaluate(self, s: &System) -> Result<f64, String> {
        let thickness = s.surfaces[self.gap].thickness;
        let residual = if let Some(w) = self.focus {
            thickness
                - s.paraxial(w)?["image_distance"]
                    .as_f64()
                    .ok_or("No finite paraxial focus")?
        } else {
            match s.surfaces[self.gap].gap_solve {
                GapSolve::Fixed {} => {
                    return Err("Fixed gaps do not define constraint equations".into());
                }
                GapSolve::Pickup {
                    source,
                    scale,
                    offset,
                } => thickness - (scale * s.surfaces[source].thickness + offset),
                GapSolve::EdgeThickness { height, value } => {
                    thickness
                        - (value + sag(&s.surfaces[self.gap], height)?.0
                            - sag(&s.surfaces[self.gap + 1], height)?.0)
                }
                GapSolve::OverallLength { first, last, value } => {
                    s.surfaces[first..last]
                        .iter()
                        .map(|g| g.thickness)
                        .sum::<f64>()
                        - value
                }
                GapSolve::MarginalHeight { value, zone } => {
                    thickness - marginal_thickness(s, self.gap, value, zone)?
                }
            }
        };
        if residual.is_finite() {
            Ok(residual)
        } else {
            Err("Nonfinite constraint residual".into())
        }
    }
    fn linear_row(self, s: &System, equations: &[Equation]) -> Vec<f64> {
        equations
            .iter()
            .map(|unknown| match s.surfaces[self.gap].gap_solve {
                GapSolve::Pickup { source, scale, .. } => {
                    (if unknown.gap == self.gap { 1. } else { 0. })
                        - if unknown.gap == source { scale } else { 0. }
                }
                GapSolve::OverallLength { first, last, .. } => {
                    if first <= unknown.gap && unknown.gap < last {
                        1.
                    } else {
                        0.
                    }
                }
                _ => {
                    if unknown.gap == self.gap {
                        1.
                    } else {
                        0.
                    }
                }
            })
            .collect()
    }
}

fn marginal_thickness(s: &System, gap: usize, target: f64, zone: f64) -> Result<f64, String> {
    let w = s.reference();
    let radius = s.pupil_radius(w)?;
    if zone.abs() < f64::EPSILON {
        let (height, reduced_slope) = if let Some(distance) = s.object_distance {
            let slope = radius / (distance + s.entrance_pupil_distance(w)?);
            (distance * slope, s.object_index(w)? * slope)
        } else {
            (radius, 0.)
        };
        let (m, n) = s.matrix(w, 0, gap + 1)?;
        let y = m.a * height + m.b * reduced_slope;
        let u = (m.c * height + m.d * reduced_slope) / n;
        if u.abs() < 1e-15 {
            return Err("Marginal ray is parallel to axis".into());
        }
        Ok((target - y) / u)
    } else {
        // A downstream surface or image must not prevent solving this earlier gap.
        let ray = s.trace_field_to(0., radius * zone, &s.fields[0], w, gap, false)?;
        let segment = ray.last();
        if segment.direction[1].abs() < 1e-15 {
            return Err("Marginal ray is parallel to axis".into());
        }
        let vertex = s.surfaces[..gap].iter().map(|g| g.thickness).sum::<f64>();
        Ok(segment.point[2] - vertex
            + (target - segment.point[1]) * segment.direction[2] / segment.direction[1])
    }
}

// Solve independent rows with scaled elimination. Free columns retain their current
// values (zero step). Used first to satisfy affine constraints, then for Newton steps.
fn equation_step(
    mut matrix: Vec<Vec<f64>>,
    mut rhs: Vec<f64>,
    columns: usize,
) -> Result<Vec<f64>, String> {
    let rows = matrix.len();
    for (row, value) in matrix.iter_mut().zip(&mut rhs) {
        let scale = row.iter().fold(0_f64, |v, a| v.max(a.abs()));
        if !scale.is_finite() || scale == 0. || !value.is_finite() {
            return Err(
                "Solve equations are inconsistent or do not determine unique thicknesses".into(),
            );
        }
        for a in row {
            *a /= scale;
        }
        *value /= scale;
    }
    let mut pivots = vec![];
    for column in 0..columns {
        let rank = pivots.len();
        if rank == rows {
            break;
        }
        let pivot = (rank..rows)
            .max_by(|&a, &b| matrix[a][column].abs().total_cmp(&matrix[b][column].abs()))
            .unwrap();
        if matrix[pivot][column].abs() < 1e-12 {
            continue;
        }
        matrix.swap(rank, pivot);
        rhs.swap(rank, pivot);
        let divisor = matrix[rank][column];
        for a in &mut matrix[rank] {
            *a /= divisor;
        }
        rhs[rank] /= divisor;
        for row in 0..rows {
            if row == rank {
                continue;
            }
            let multiplier = matrix[row][column];
            for col in 0..columns {
                let pivot_value = matrix[rank][col];
                matrix[row][col] -= multiplier * pivot_value;
            }
            let pivot_value = rhs[rank];
            rhs[row] -= multiplier * pivot_value;
        }
        pivots.push(column);
    }
    if pivots.len() != rows {
        return Err(
            "Solve equations are inconsistent or do not determine unique thicknesses".into(),
        );
    }
    let mut step = vec![0.; columns];
    for (row, column) in pivots.into_iter().enumerate() {
        step[column] = rhs[row];
    }
    if step.iter().all(|v| v.is_finite()) {
        Ok(step)
    } else {
        Err("Nonfinite solve step".into())
    }
}

fn residuals(s: &System, equations: &[Equation]) -> Result<Vec<f64>, String> {
    equations.iter().map(|e| e.residual(s)).collect()
}
fn score(s: &System, equations: &[Equation], residuals: &[f64]) -> f64 {
    equations
        .iter()
        .zip(residuals)
        .map(|(eq, residual)| residual.abs() / (1e-9 + 1e-12 * s.surfaces[eq.gap].thickness.abs()))
        .fold(0., f64::max)
}
fn apply_step(
    s: &System,
    equations: &[Equation],
    step: &[f64],
    fraction: f64,
) -> Result<System, String> {
    let mut next = s.clone();
    for (equation, delta) in equations.iter().zip(step) {
        next.surfaces[equation.gap].thickness += fraction * delta;
        if !next.surfaces[equation.gap].thickness.is_finite() {
            return Err("Solve produced a nonfinite gap".into());
        }
    }
    Ok(next)
}
fn jacobian(
    s: &System,
    equations: &[Equation],
    residuals: &[f64],
) -> Result<Vec<Vec<f64>>, String> {
    let mut jacobian = equations
        .iter()
        .map(|eq| {
            if eq.is_linear(s) {
                eq.linear_row(s, equations)
            } else {
                vec![0.; equations.len()]
            }
        })
        .collect::<Vec<_>>();
    if equations.iter().all(|eq| eq.is_linear(s)) {
        return Ok(jacobian);
    }
    let mut trial = s.clone();
    for (column, variable) in equations.iter().enumerate() {
        let original = s.surfaces[variable.gap].thickness;
        let delta = 6e-6 * (1. + original.abs());
        trial.surfaces[variable.gap].thickness = original + delta;
        let plus = equations
            .iter()
            .map(|eq| {
                if eq.is_linear(s) {
                    Ok(0.)
                } else {
                    eq.residual(&trial)
                }
            })
            .collect::<Vec<_>>();
        trial.surfaces[variable.gap].thickness = original - delta;
        let minus = equations
            .iter()
            .map(|eq| {
                if eq.is_linear(s) {
                    Ok(0.)
                } else {
                    eq.residual(&trial)
                }
            })
            .collect::<Vec<_>>();
        trial.surfaces[variable.gap].thickness = original;
        for (row, equation) in equations.iter().enumerate() {
            if equation.is_linear(s) {
                continue;
            }
            jacobian[row][column] = match (&plus[row], &minus[row]) {
                (Ok(p), Ok(m)) => (p - m) / (2. * delta),
                (Ok(p), Err(_)) => (p - residuals[row]) / delta,
                (Err(_), Ok(m)) => (residuals[row] - m) / delta,
                (Err(p), Err(m)) => {
                    return Err(format!(
                        "Cannot differentiate gap {}: {p}; {m}",
                        equation.gap
                    ));
                }
            };
        }
    }
    Ok(jacobian)
}

fn resolve(s: &mut System, focus: Option<f64>) -> Result<(), String> {
    let mut equations = s
        .surfaces
        .iter()
        .enumerate()
        .filter_map(|(gap, surface)| {
            if surface.gap_solve.is_fixed() {
                None
            } else {
                Some(Equation { gap, focus: None })
            }
        })
        .collect::<Vec<_>>();
    if let Some(w) = focus {
        let gap = s.surfaces.len() - 1;
        if !s.surfaces[gap].gap_solve.is_fixed() {
            return Err(
                "Image distance is controlled by a gap solve; remove that solve before autofocus"
                    .into(),
            );
        }
        equations.push(Equation {
            gap,
            focus: Some(w),
        });
    }
    if equations.is_empty() {
        return Ok(());
    }
    let linear = equations
        .iter()
        .copied()
        .filter(|eq| eq.is_linear(s))
        .collect::<Vec<_>>();
    let mut work = s.clone();
    if !linear.is_empty() {
        let matrix = linear
            .iter()
            .map(|eq| eq.linear_row(&work, &equations))
            .collect();
        let rhs = linear
            .iter()
            .map(|eq| eq.residual(&work).map(|v| -v))
            .collect::<Result<Vec<_>, _>>()?;
        let step = equation_step(matrix, rhs, equations.len())?;
        work = apply_step(&work, &equations, &step, 1.)?;
    }
    for _ in 0..40 {
        let residual = residuals(&work, &equations)?;
        let current = score(&work, &equations, &residual);
        // Rank must be checked even if the initial values happen to satisfy every row.
        let matrix = jacobian(&work, &equations, &residual)?;
        let step = equation_step(
            matrix,
            residual.iter().map(|r| -r).collect(),
            equations.len(),
        )?;
        if current <= 1. {
            if focus.is_some() && work.surfaces.last().unwrap().thickness < 0. {
                return Err("Focus lies before last surface".into());
            }
            *s = work;
            return Ok(());
        }
        let mut fraction = 1.;
        let mut improved = None;
        for _ in 0..20 {
            if let Ok(next) = apply_step(&work, &equations, &step, fraction) {
                if let Ok(next_residual) = residuals(&next, &equations) {
                    if score(&next, &equations, &next_residual) < current {
                        improved = Some(next);
                        break;
                    }
                }
            }
            fraction *= 0.5;
        }
        work = improved.ok_or("Gap solves did not converge to a traceable solution")?;
    }
    Err("Gap solves did not converge within 40 iterations".into())
}

pub fn recalculate(s: &mut System) -> Result<(), String> {
    resolve(s, None)
}
pub fn autofocus(system: &System, w: f64) -> Result<System, String> {
    let mut next = system.clone();
    next.validate_parameters()?;
    resolve(&mut next, Some(w))?;
    next.validate()?;
    next.update_semi_diameters()?;
    Ok(next)
}
pub fn state(s: &System, gap: usize) -> Value {
    json!({"gap":gap,"thickness":s.surfaces[gap].thickness,"definition":s.surfaces[gap].gap_solve})
}
pub fn states(s: &System) -> Vec<Value> {
    s.surfaces
        .iter()
        .enumerate()
        .filter(|(_, g)| !g.gap_solve.is_fixed())
        .map(|(i, _)| state(s, i))
        .collect()
}
pub fn apply(system: &System, gap: usize, definition: GapSolve) -> Result<(System, Value), String> {
    if gap >= system.surfaces.len() {
        return Err("Gap index outside prescription".into());
    }
    let mut next = system.clone();
    next.surfaces[gap].gap_solve = definition;
    next.update()?;
    let result = state(&next, gap);
    Ok((next, result))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn zero_zone_focuses_paraxial_axial_ray() {
        for distance in [None, Some(200.)] {
            let mut s = System::default();
            s.object_distance = distance;
            s.stop_index = 1;
            let expected = s.paraxial(s.reference()).unwrap()["image_distance"]
                .as_f64()
                .unwrap();
            let (solved, _) = apply(
                &s,
                1,
                GapSolve::MarginalHeight {
                    value: 0.,
                    zone: 0.,
                },
            )
            .unwrap();
            assert!((solved.surfaces[1].thickness - expected).abs() < 1e-8);
        }
    }
    #[test]
    fn real_zone_uses_first_prescribed_field() {
        let mut s = System::default();
        s.fields = vec![Field::meridional(3.)];
        let target = 2.;
        let (solved, _) = apply(
            &s,
            1,
            GapSolve::MarginalHeight {
                value: target,
                zone: 0.7,
            },
        )
        .unwrap();
        let ray = solved
            .trace_field(
                0.,
                solved.pupil_radius(solved.reference()).unwrap() * 0.7,
                &solved.fields[0],
                solved.reference(),
            )
            .unwrap();
        assert!((ray.image()[1] - target).abs() < 1e-9);
    }
    #[test]
    fn overfilled_marginal_zone_traces_when_apertures_allow() {
        let (s, _) = apply(
            &System::default(),
            1,
            GapSolve::MarginalHeight {
                value: 0.,
                zone: 1.5,
            },
        )
        .unwrap();
        let ray = s
            .trace_field(
                0.,
                1.5 * s.pupil_radius(s.reference()).unwrap(),
                &s.fields[0],
                s.reference(),
            )
            .unwrap();
        assert!(ray.image()[1].abs() < 1e-9);
    }
    #[test]
    fn edge_thickness_reacts_to_curvature() {
        let s = System::default();
        let (mut s, _) = apply(
            &s,
            0,
            GapSolve::EdgeThickness {
                height: 5.,
                value: 2.,
            },
        )
        .unwrap();
        s.surfaces[0].radius = 30.;
        s.update().unwrap();
        let edge = -sag(&s.surfaces[0], 5.).unwrap().0
            + s.surfaces[0].thickness
            + sag(&s.surfaces[1], 5.).unwrap().0;
        assert!((edge - 2.).abs() < 1e-10);
    }
    #[test]
    fn overall_length_and_pickup_are_persistent() {
        let s = System::default();
        let (mut s, _) = apply(
            &s,
            1,
            GapSolve::OverallLength {
                first: 0,
                last: 2,
                value: 70.,
            },
        )
        .unwrap();
        s.surfaces[0].thickness = 10.;
        s.update().unwrap();
        assert!((s.surfaces[1].thickness - 60.).abs() < 1e-10);
        let (mut s, _) = apply(
            &s,
            1,
            GapSolve::Pickup {
                source: 0,
                scale: 2.,
                offset: 1.,
            },
        )
        .unwrap();
        s.surfaces[0].thickness = 7.;
        s.update().unwrap();
        assert!((s.surfaces[1].thickness - 15.).abs() < 1e-10);
        let (mut s, _) = apply(&s, 1, GapSolve::Fixed {}).unwrap();
        s.surfaces[0].thickness = 8.;
        s.update().unwrap();
        assert!((s.surfaces[1].thickness - 15.).abs() < 1e-10);
    }
    #[test]
    fn coupled_pickups_resolve_and_singular_constraints_fail() {
        let mut s = System::default();
        s.surfaces[0].gap_solve = GapSolve::Pickup {
            source: 1,
            scale: 2.,
            offset: 1.,
        };
        s.surfaces[1].gap_solve = GapSolve::Pickup {
            source: 0,
            scale: 0.25,
            offset: 2.,
        };
        s.update().unwrap();
        assert!((s.surfaces[0].thickness - 10.).abs() < 1e-10);
        assert!((s.surfaces[1].thickness - 4.5).abs() < 1e-10);
        for offset in [0., 1.] {
            s.surfaces[0].gap_solve = GapSolve::Pickup {
                source: 1,
                scale: 1.,
                offset: 0.,
            };
            s.surfaces[1].gap_solve = GapSolve::Pickup {
                source: 0,
                scale: 1.,
                offset,
            };
            assert!(s.update().unwrap_err().contains("unique"));
        }
    }
    #[test]
    fn nonlinear_ray_constraint_resolves_after_lens_edit() {
        let (mut s, _) = apply(
            &System::default(),
            1,
            GapSolve::MarginalHeight {
                value: 0.,
                zone: 0.8,
            },
        )
        .unwrap();
        s.surfaces[0].radius = 60.;
        s.update().unwrap();
        let ray = s
            .trace_field(
                0.,
                s.pupil_radius(s.reference()).unwrap() * 0.8,
                &s.fields[0],
                s.reference(),
            )
            .unwrap();
        assert!(ray.image()[1].abs() < 1e-9);
    }
    #[test]
    fn marginal_constraint_before_internal_stop_is_solved_implicitly() {
        let mut s = System::default();
        s.stop_index = 1;
        let (mut s, _) = apply(
            &s,
            0,
            GapSolve::MarginalHeight {
                value: 1.5,
                zone: 0.4,
            },
        )
        .unwrap();
        for radius in [50., 60.] {
            s.surfaces[0].radius = radius;
            s.update().unwrap();
            let ray = s
                .trace_field_to(
                    0.,
                    s.pupil_radius(s.reference()).unwrap() * 0.4,
                    &s.fields[0],
                    s.reference(),
                    0,
                    false,
                )
                .unwrap();
            let seg = ray.last();
            let height = seg.point[1]
                + (s.surfaces[0].thickness - seg.point[2]) * seg.direction[1] / seg.direction[2];
            assert!((height - 1.5).abs() < 1e-9);
        }
    }
    #[test]
    fn earlier_ray_solve_does_not_trace_through_unsolved_downstream_geometry() {
        let mut s = System::default();
        s.surfaces[1].thickness = -1.;
        let radius = s.pupil_radius(s.reference()).unwrap();
        let (s, _) = apply(
            &s,
            0,
            GapSolve::MarginalHeight {
                value: 1.,
                zone: 0.4,
            },
        )
        .unwrap();
        let ray = s
            .trace_field_to(0., radius * 0.4, &s.fields[0], s.reference(), 0, false)
            .unwrap();
        let seg = ray.last();
        let y = seg.point[1]
            + (s.surfaces[0].thickness - seg.point[2]) * seg.direction[1] / seg.direction[2];
        assert!((y - 1.).abs() < 1e-9);
    }
    #[test]
    fn autofocus_solves_feedback_from_an_upstream_pickup() {
        let mut s = System::default();
        s.surfaces[0].gap_solve = GapSolve::Pickup {
            source: 1,
            scale: 0.05,
            offset: 2.5,
        };
        s.update().unwrap();
        let focused = autofocus(&s, 656.2725).unwrap();
        let distance = focused.paraxial(656.2725).unwrap()["image_distance"]
            .as_f64()
            .unwrap();
        assert!((focused.surfaces[1].thickness - distance).abs() < 1e-8);
        assert!(
            (focused.surfaces[0].thickness - 0.05 * focused.surfaces[1].thickness - 2.5).abs()
                < 1e-9
        );
        let (s, _) = apply(
            &System::default(),
            1,
            GapSolve::Pickup {
                source: 0,
                scale: 2.,
                offset: 0.,
            },
        )
        .unwrap();
        assert!(
            autofocus(&s, s.reference())
                .unwrap_err()
                .contains("controlled")
        );
    }
}
