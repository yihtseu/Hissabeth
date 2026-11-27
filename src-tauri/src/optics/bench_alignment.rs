use super::{
    Bench, Branch, Kind, PathBreak, Port, System, Trace, V3, bench, dot, norm, normal_rotation,
    reached, sub, unit,
};
use bench::NominalPaths;
use serde::Serialize;
use std::collections::HashSet;

const TOLERANCE: f64 = 1e-6;

#[derive(Clone, Copy, schemars::JsonSchema, Serialize)]
pub(super) struct Separation {
    pub position_mm: f64,
    pub angle_mrad: f64,
}

pub(super) struct Overlap {
    pub separation: Separation,
    spot_delta: V3,
    direction_delta: V3,
}

impl Overlap {
    fn aligned(&self) -> bool {
        self.separation.position_mm < TOLERANCE && self.separation.angle_mrad < TOLERANCE
    }
}

pub(super) fn measure(
    trace: &Trace,
    arrivals: [usize; 2],
    input: &Port,
) -> Result<Overlap, PathBreak> {
    let missing = || PathBreak::new(&input.element, &input.port, "no_common_output");
    let [first, second] = arrivals.map(|i| &trace.segments[i]);
    let [a, b] = arrivals.map(|i| trace.splitter_outputs.get(&i).copied().unwrap_or_default());
    // A thin splitter's other output reflects both of these output directions.
    // The angle is invariant under that reflection, so one available pairing suffices.
    let (a, b) = a[0]
        .zip(b[1])
        .or_else(|| a[1].zip(b[0]))
        .ok_or_else(missing)?;
    let (a, b) = (
        unit(a).map_err(|_| missing())?,
        unit(b).map_err(|_| missing())?,
    );
    let spot_delta = sub(second.to, first.to);
    Ok(Overlap {
        separation: Separation {
            position_mm: norm(spot_delta),
            angle_mrad: 1000. * norm(bench::cross(a, b)).atan2(dot(a, b)),
        },
        spot_delta,
        direction_delta: sub(b, a).map(|value| 1000. * value),
    })
}

struct Input {
    from: Port,
    at: Port,
    branch: Option<(String, Port)>,
}

pub(super) struct Combiner {
    pub index: usize,
    inputs: [Input; 2],
}

impl Combiner {
    pub(super) fn find(bench: &Bench, paths: &NominalPaths<'_>) -> Vec<Self> {
        bench
            .elements
            .iter()
            .enumerate()
            .filter_map(|(index, element)| {
                if !matches!(element.kind, Kind::Splitter | Kind::Pbs) {
                    return None;
                }
                let mut inputs: Vec<_> = bench
                    .connections
                    .iter()
                    .filter(|c| c.to == element.id)
                    .map(|c| Port::new(&c.from, &c.port))
                    .collect();
                if inputs.len() == 3 {
                    // In a Michelson the launch cable also leads to the combiner.
                    inputs.retain(|input| !bench.sources.iter().any(|s| s.id == input.element));
                }
                let inputs: [Port; 2] = inputs.try_into().ok()?;
                Some(Self {
                    index,
                    inputs: inputs.map(|from| {
                        let at = Port::new(&element.id, paths.input(&from));
                        let branch = paths
                            .branch_ending_at(&from, &element.id)
                            .map(|Branch { id, from, .. }| (id.clone(), from.clone()));
                        Input { from, at, branch }
                    }),
                })
            })
            .collect()
    }

    pub(super) fn nominal_normal(&self, bench: &Bench) -> Option<V3> {
        let position = bench.elements[self.index].position;
        let direction = |input: &Input| {
            let id = &input.from.element;
            let from = bench
                .elements
                .iter()
                .find(|e| &e.id == id)
                .map(|e| e.position)
                .or_else(|| {
                    bench
                        .sources
                        .iter()
                        .find(|s| &s.id == id)
                        .map(|s| s.position)
                })?;
            unit(sub(position, from)).ok()
        };
        let [a, b] = self.inputs.each_ref().map(direction);
        let normal = unit(sub(a?, b?)).ok()?;
        let element = &bench.elements[self.index];
        let reference = element
            .rotation
            .map_or(b?, |r| bench::rotate([1., 0., 0.], r));
        Some(if dot(normal, reference) < 0. {
            normal.map(|v| -v)
        } else {
            normal
        })
    }

    fn observe(&self, trace: &Trace) -> Result<Overlap, PathBreak> {
        let arrival = |input: &Input| {
            if let Some((branch, from)) = &input.branch {
                let report = trace
                    .branches
                    .iter()
                    .find(|r| &r.id == branch)
                    .expect("trace includes declared branches");
                return Ok(reached(report, from)?
                    .end_segment
                    .expect("reached branch endpoint"));
            }
            let mut arrivals = trace.segments.iter().enumerate().filter(|(_, segment)| {
                segment.axial
                    && segment.flight.as_ref().is_some_and(|f| {
                        f.from_element == input.from.element
                            && f.from_port == input.from.port
                            && f.to_element == input.at.element
                    })
            });
            let broken = |reason| PathBreak::new(&input.at.element, &input.at.port, reason);
            let (index, segment) = arrivals.next().ok_or_else(|| broken("input_not_reached"))?;
            if arrivals.next().is_some() {
                return Err(broken("ambiguous_input"));
            }
            if segment.status != "hit" {
                return Err(broken(&segment.status));
            }
            Ok(index)
        };
        let [a, b] = self.inputs.each_ref().map(arrival);
        measure(trace, [a?, b?], &self.inputs[1].at)
    }
}

#[derive(schemars::JsonSchema, Serialize)]
pub(super) struct AimResidual {
    branch: String,
    position_mm: Option<f64>,
    angle_mrad: Option<f64>,
    #[serde(rename = "break", skip_serializing_if = "Option::is_none")]
    interruption: Option<PathBreak>,
}

type Observations = Vec<Result<Overlap, PathBreak>>;

fn observe(
    scene: &Bench,
    system: &System,
    combiners: &[Combiner],
    nominal: bool,
) -> Result<Observations, String> {
    if let Some(element) = scene.elements.iter().find(|e| e.rotation.is_none()) {
        let location = PathBreak::new(&element.id, "in", "rotation_unresolved");
        return Ok(combiners.iter().map(|_| Err(location.clone())).collect());
    }
    let trace = scene.trace_alignment(system, nominal)?;
    Ok(combiners
        .iter()
        .map(|combiner| combiner.observe(&trace))
        .collect())
}

fn objective(observations: &Observations) -> Option<Vec<f64>> {
    let mut values = Vec::with_capacity(6 * observations.len());
    for observation in observations {
        let overlap = observation.as_ref().ok()?;
        values.extend(overlap.spot_delta);
        values.extend(overlap.direction_delta);
    }
    values.iter().all(|v| v.is_finite()).then_some(values)
}

enum Adjustment {
    Element(usize),
    Source(usize),
}

impl Adjustment {
    fn shift(&self, bench: &mut Bench, axis: usize, delta: f64) {
        match *self {
            Self::Element(index) => {
                let rotation = bench.elements[index].rotation.as_mut().unwrap();
                rotation[axis] = (rotation[axis] + delta) % 360.;
            }
            Self::Source(index) => {
                let source = &mut bench.sources[index];
                let mut rotation = normal_rotation(source.direction);
                rotation[axis] += delta;
                source.direction = bench::rotate([1., 0., 0.], rotation);
            }
        }
    }
}

fn adjustments(scene: &Bench, combiners: &[Combiner], adjustable: &[bool]) -> Vec<Adjustment> {
    let mut pending: Vec<_> = combiners
        .iter()
        .map(|c| scene.elements[c.index].id.as_str())
        .collect();
    let mut upstream = HashSet::new();
    while let Some(id) = pending.pop() {
        if upstream.insert(id) {
            pending.extend(
                scene
                    .connections
                    .iter()
                    .filter(|c| c.to == id)
                    .map(|c| c.from.as_str()),
            );
        }
    }
    let elements = scene.elements.iter().enumerate().filter_map(|(i, e)| {
        (adjustable[i] && upstream.contains(e.id.as_str())).then_some(Adjustment::Element(i))
    });
    let sources = scene.sources.iter().enumerate().filter_map(|(i, s)| {
        (s.aim_direction && upstream.contains(s.id.as_str())).then_some(Adjustment::Source(i))
    });
    elements.chain(sources).collect()
}

fn squared(values: &[f64]) -> f64 {
    values.iter().map(|v| v * v).sum()
}

fn least_squares(columns: &[Vec<f64>], residual: &[f64], damping: f64) -> Option<Vec<f64>> {
    let product = |a: &[f64], b: &[f64]| a.iter().zip(b).map(|(a, b)| a * b).sum::<f64>();
    let mut rows: Vec<Vec<_>> = columns
        .iter()
        .enumerate()
        .map(|(i, column)| {
            let mut row: Vec<_> = columns.iter().map(|other| product(column, other)).collect();
            row[i] += damping * row[i].max(1.);
            row.push(-product(column, residual));
            row
        })
        .collect();
    // Damped normal equations are positive definite. Eliminate the lower triangle in place.
    let n = rows.len();
    for i in 0..n {
        let (solved, remaining) = rows.split_at_mut(i + 1);
        let pivot = &solved[i];
        if !pivot[i].is_finite() || pivot[i] <= 0. {
            return None;
        }
        for row in remaining {
            let factor = row[i] / pivot[i];
            for (value, base) in row[i + 1..].iter_mut().zip(&pivot[i + 1..]) {
                *value -= factor * base;
            }
        }
    }
    let mut step = vec![0.; n];
    for (i, row) in rows.iter().enumerate().rev() {
        step[i] = (row[n] - product(&row[i + 1..n], &step[i + 1..])) / row[i];
    }
    step.iter().all(|v| v.is_finite()).then_some(step)
}

fn refine(
    scene: &mut Bench,
    changes: &[Adjustment],
    mut observations: Observations,
    measure_candidate: impl Fn(&Bench) -> Result<Observations, String>,
) -> Result<Observations, String> {
    const DERIVATIVE_STEP: f64 = 1e-4;
    let axes: Vec<_> = changes
        .iter()
        .flat_map(|change| [(change, 1), (change, 2)])
        .collect();
    let mut damping = 1e-4;
    for _ in 0..24 {
        if axes.is_empty()
            || observations
                .iter()
                .all(|value| value.as_ref().is_ok_and(Overlap::aligned))
        {
            break;
        }
        let Some(current) = objective(&observations) else {
            break;
        };
        let mut columns = Vec::with_capacity(axes.len());
        for (change, axis) in &axes {
            let mut offset = scene.clone();
            change.shift(&mut offset, *axis, DERIVATIVE_STEP);
            let values = objective(&measure_candidate(&offset)?);
            columns.push(values.map_or_else(
                || vec![0.; current.len()],
                |values| {
                    values
                        .iter()
                        .zip(&current)
                        .map(|(a, b)| (a - b) / DERIVATIVE_STEP)
                        .collect()
                },
            ));
        }
        let Some(step) = least_squares(&columns, &current, damping) else {
            break;
        };
        let largest = step.iter().fold(0_f64, |max, v| max.max(v.abs()));
        let scale = 1. / (largest / 10.).max(1.);
        let mut next = scene.clone();
        for ((change, axis), delta) in axes.iter().zip(step) {
            change.shift(&mut next, *axis, delta * scale);
        }
        let trial = measure_candidate(&next)?;
        if objective(&trial).is_some_and(|values| squared(&values) < squared(&current)) {
            *scene = next;
            observations = trial;
            damping = (damping / 4.).max(1e-10);
        } else {
            damping *= 10.;
        }
    }
    Ok(observations)
}

pub(super) fn align(
    scene: &mut Bench,
    original: &Bench,
    system: &System,
    combiners: &[Combiner],
    adjustable: &[bool],
) -> Result<(Vec<Port>, Vec<AimResidual>), String> {
    if combiners.is_empty() {
        return Ok((vec![], vec![]));
    }
    let mut supplied = scene.clone();
    for (element, previous) in supplied.elements.iter_mut().zip(&original.elements) {
        if previous.rotation.is_some() {
            element.rotation = previous.rotation;
        }
    }
    for (source, previous) in supplied.sources.iter_mut().zip(&original.sources) {
        source.direction = previous.direction;
    }
    let aligned = |observations: &Observations| {
        observations
            .iter()
            .all(|observation| observation.as_ref().is_ok_and(Overlap::aligned))
    };
    let supplied_physical = observe(&supplied, system, combiners, false)?;
    let changes = adjustments(scene, combiners, adjustable);
    let (physical, nominal) = if aligned(&supplied_physical) {
        *scene = supplied;
        (supplied_physical, None)
    } else if changes.is_empty() {
        let nominal = if supplied_physical.iter().any(Result::is_err) {
            Some(observe(&supplied, system, combiners, true)?)
        } else {
            None
        };
        (supplied_physical, nominal)
    } else {
        let seeded_physical = observe(scene, system, combiners, false)?;
        if aligned(&seeded_physical) {
            (seeded_physical, None)
        } else {
            let mut seed = observe(scene, system, combiners, true)?;
            let supplied_geometry = observe(&supplied, system, combiners, true)?;
            let cost =
                |values: &Observations| objective(values).map_or(f64::INFINITY, |v| squared(&v));
            let use_supplied = cost(&supplied_geometry) < cost(&seed);
            if use_supplied {
                seed = supplied_geometry;
            }
            if objective(&seed).is_some() {
                if use_supplied {
                    *scene = supplied;
                }
                let nominal = refine(scene, &changes, seed, |candidate| {
                    observe(candidate, system, combiners, true)
                })?;
                (observe(scene, system, combiners, false)?, Some(nominal))
            } else {
                // A retroreflected path can have physical arrivals without a directed launch route.
                let seed_physical = if cost(&supplied_physical) < cost(&seeded_physical) {
                    *scene = supplied;
                    supplied_physical
                } else {
                    seeded_physical
                };
                let physical = refine(scene, &changes, seed_physical, |candidate| {
                    observe(candidate, system, combiners, false)
                })?;
                (physical, Some(seed))
            }
        }
    };
    let mut conflicts = Vec::new();
    let mut residuals = Vec::new();
    for (i, (combiner, result)) in combiners.iter().zip(physical).enumerate() {
        if !result.as_ref().is_ok_and(Overlap::aligned) {
            for input in &combiner.inputs {
                let port = if input.at.port == "in" {
                    &input.from
                } else {
                    &input.at
                };
                conflicts.push(port.clone());
            }
        }
        let separation = result
            .as_ref()
            .ok()
            .or_else(|| nominal.as_ref()?.get(i)?.as_ref().ok());
        for (arm, input) in combiner.inputs.iter().enumerate() {
            residuals.push(AimResidual {
                branch: input.branch.as_ref().map_or_else(
                    || format!("{}/{}", input.from.element, input.from.port),
                    |(id, _)| id.clone(),
                ),
                position_mm: separation.map(|overlap| {
                    if arm == 0 {
                        0.
                    } else {
                        overlap.separation.position_mm
                    }
                }),
                angle_mrad: separation.map(|overlap| {
                    if arm == 0 {
                        0.
                    } else {
                        overlap.separation.angle_mrad
                    }
                }),
                interruption: result.as_ref().err().cloned(),
            });
        }
    }
    Ok((conflicts, residuals))
}
