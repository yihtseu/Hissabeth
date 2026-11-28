//! Nominal bench routes and their correspondence with actual propagation.
use super::{Bench, Segment};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(schemars::JsonSchema, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Port {
    pub element: String,
    pub port: String,
}

impl Port {
    pub(crate) fn new(element: &str, port: &str) -> Self {
        Self {
            element: element.into(),
            port: port.into(),
        }
    }
}

#[derive(schemars::JsonSchema, Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Branch {
    pub id: String,
    /// Output port at which measurement begins.
    pub from: Port,
    /// Named input at which measurement ends. "in" is the generic input.
    pub to: Port,
    /// Ordered output selections at intermediate elements with multiple nominal continuations.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(length(max = 128))]
    pub via: Vec<Port>,
    /// How the drawing shows this branch's beams; the shared palette colour and a solid line when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<BranchStyle>,
}

/// Drawing style of a named branch, shared by the 2D view, the blueprint, the legend
/// and the branch readings.
#[derive(schemars::JsonSchema, Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BranchStyle {
    /// Beam colour "#rrggbb".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(pattern(r"^#[0-9a-fA-F]{6}$"))]
    pub color: Option<String>,
    /// Draw the branch dotted. On an upper height layer the dots join that layer's dashes.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dash: bool,
}

#[derive(schemars::JsonSchema, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssertionKind {
    EqualOpl,
    Colinear,
    Dispersion,
    Parity,
    ImageRotation,
}

#[derive(schemars::JsonSchema, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DispersionQuantity {
    Phase,
    GroupDelay,
}

#[derive(schemars::JsonSchema, Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssertionUnit {
    Mm,
    Um,
    Mrad,
    Fs,
    Rad,
    Count,
}
impl AssertionUnit {
    pub(crate) fn convert_mm(self, value: f64) -> f64 {
        match self {
            Self::Um => value * 1000.,
            _ => value,
        }
    }

    pub(in crate::optics) fn symbol(self) -> &'static str {
        match self {
            Self::Mm => "mm",
            Self::Um => "µm",
            Self::Mrad => "mrad",
            Self::Fs => "fs",
            Self::Rad => "rad",
            Self::Count => "",
        }
    }
}

#[derive(schemars::JsonSchema, Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assertion {
    pub id: String,
    /// `parity` (unit `count`, target 0 even or 1 odd) checks the reflection
    /// count of a branch listed twice, or the parity difference of two branches;
    /// `image_rotation` is the first branch's signed image rotation relative to the
    /// second about their shared exit direction (rad or mrad).
    pub kind: AssertionKind,
    /// Two distinct branch IDs (the same ID twice for an absolute `parity`).
    /// Equal OPL takes the first branch minus the second, both measured from
    /// the last segment the two arms share (the source launch when they share
    /// none); arms of two sources need a shared coherence group and count from
    /// launches taken to be in phase.
    pub branches: [String; 2],
    pub target: f64,
    pub unit: AssertionUnit,
    #[schemars(range(min = 0.0))]
    pub tolerance: f64,
    /// Dispersion only: working band `[λmin, λmax]` in nm.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(inner(range(min = 100.0, max = 20000.0)))]
    pub band_nm: Option<[f64; 2]>,
    /// Dispersion only: compared quantity (`phase` in rad, `group_delay` in fs).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quantity: Option<DispersionQuantity>,
}

#[derive(schemars::JsonSchema, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelayCheckKind {
    /// Matrix element B (mm) of the object-to-image matrix: 0 for an imaging relay.
    Conjugate,
    /// Matrix element A: the transverse magnification of a conjugate relay.
    Magnification,
    /// Matrix element C (1/mm): 0 for an afocal relay.
    Afocal,
    /// Largest deviation (mm) of object→first = f1, first→second = f1 + f2 and
    /// second→image = f2 for exactly two thin powered elements.
    #[serde(rename = "strict_4f")]
    Strict4f,
}

#[derive(schemars::JsonSchema, Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelayCheck {
    pub kind: RelayCheckKind,
    /// Magnification only; the other checks always target 0.
    #[serde(default)]
    pub target: f64,
    #[schemars(range(min = 0.0))]
    pub tolerance: f64,
}

/// Paraxial ABCD relay along one branch, from an object plane before the
/// branch's starting element to an image plane after its end element.
#[derive(schemars::JsonSchema, Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Relay {
    pub id: String,
    pub branch: String,
    /// Object plane distance before the branch's starting element (mm, in air).
    #[serde(default)]
    #[schemars(range(min = -1e6, max = 1e6))]
    pub object_distance_mm: f64,
    /// Image plane distance after the branch's end element (mm, in air).
    #[serde(default)]
    #[schemars(range(min = -1e6, max = 1e6))]
    pub image_distance_mm: f64,
    /// Element (or source) on the branch whose plane is the aperture stop.
    /// Defaults to the first powered element.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pupil_stop: Option<String>,
    /// Element on the branch, at or after the stop, that the conjugate pupil is
    /// measured from. Defaults to the branch's end element.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pupil_target: Option<String>,
    /// Checks on the matrix. Without any, the relay only reports its readings.
    #[serde(default)]
    #[schemars(length(max = 8))]
    pub checks: Vec<RelayCheck>,
}

type Edge<'a> = (&'a str, &'a str);
type Connections<'a> = HashMap<&'a str, Vec<Edge<'a>>>;

pub(in crate::optics) struct NominalPaths<'a> {
    routes: Vec<(&'a Branch, Vec<Edge<'a>>)>,
    /// Fold routes: measured like branches, but their endpoints name no inputs.
    folds: Vec<(&'a Branch, Vec<Edge<'a>>)>,
    inputs: HashMap<Edge<'a>, &'a str>,
}

impl Branch {
    fn route<'a>(&'a self, edges: &Connections<'a>) -> Result<Vec<Edge<'a>>, String> {
        let error = |at: &str, reason: &str| format!("Branch {} at {at}: {reason}", self.id);
        let mut outputs = Vec::new();
        let mut visited = HashSet::new();
        let mut via = self.via.iter().peekable();
        let mut output = (self.from.element.as_str(), self.from.port.as_str());
        loop {
            if !visited.insert(output) {
                return Err(error(output.0, "cycle in nominal route"));
            }
            outputs.push(output);
            let target = edges
                .get(output.0)
                .and_then(|ports| ports.iter().find(|&&(port, _)| port == output.1))
                .map(|&(_, target)| target)
                .ok_or_else(|| {
                    error(output.0, &format!("no connection from output {}", output.1))
                })?;
            if via.peek().is_some_and(|p| p.element == target) {
                output = (target, via.next().unwrap().port.as_str());
            } else if target == self.to.element {
                if via.peek().is_some() {
                    return Err(error(target, "unused intermediate port selection"));
                }
                return Ok(outputs);
            } else {
                output = match edges.get(target).map(Vec::as_slice) {
                    Some([(port, _)]) => (target, port),
                    Some(_) => return Err(error(target, "select an intermediate output in via")),
                    None => return Err(error(target, "nominal route ends before the destination")),
                };
            }
        }
    }
}

impl Bench {
    pub(super) fn nominal_paths(&self) -> Result<NominalPaths<'_>, String> {
        if self.branches.len() > 128
            || self.assertions.len() > 128
            || self.folds.len() > 64
            || self.relays.len() > 32
        {
            return Err("Bench branch, fold or assertion budget exceeded".into());
        }
        let mut edges = Connections::new();
        for c in &self.connections {
            edges.entry(&c.from).or_default().push((&c.port, &c.to));
        }
        let mut by_id = HashMap::new();
        let mut routes = Vec::new();
        let mut inputs = HashMap::new();
        let mut named_inputs = HashMap::new();
        for branch in &self.branches {
            if !(1..=64).contains(&branch.id.len()) || by_id.insert(&branch.id, branch).is_some() {
                return Err(format!("Invalid or repeated branch ID: {}", branch.id));
            }
            if let Some(color) = branch.style.as_ref().and_then(|s| s.color.as_deref())
                && !(color.len() == 7
                    && color.starts_with('#')
                    && color[1..].chars().all(|c| c.is_ascii_hexdigit()))
            {
                return Err(format!(
                    "Invalid branch style: {}: color must be #rrggbb",
                    branch.id
                ));
            }
            if !(1..=64).contains(&branch.to.port.len()) || branch.via.len() > 128 {
                return Err(format!(
                    "Invalid endpoint or intermediate ports: {}",
                    branch.id
                ));
            }
            let route = branch.route(&edges)?;
            let last = *route.last().unwrap();
            let input = (branch.to.element.as_str(), branch.to.port.as_str());
            if inputs
                .insert(last, input.1)
                .is_some_and(|name| name != input.1)
                || (input.1 != "in"
                    && named_inputs
                        .insert(input, last)
                        .is_some_and(|edge| edge != last))
            {
                return Err(format!(
                    "Conflicting input port {} / {} in branch {}",
                    input.0, input.1, branch.id
                ));
            }
            routes.push((branch, route));
        }
        let mut fold_ids = HashSet::new();
        let mut folds = Vec::new();
        for fold in &self.folds {
            if !(1..=64).contains(&fold.id.len()) || !fold_ids.insert(&fold.id) {
                return Err(format!("Invalid or repeated fold ID: {}", fold.id));
            }
            if !(1..=64).contains(&fold.to.port.len()) || fold.via.len() > 128 {
                return Err(format!(
                    "Invalid endpoint or intermediate ports: {}",
                    fold.id
                ));
            }
            let route = fold.route(&edges)?;
            // A fold names the input its route actually arrives at: a branch-declared
            // input, otherwise the generic "in".
            let arrival = inputs.get(route.last().unwrap()).copied().unwrap_or("in");
            if fold.to.port != arrival {
                return Err(format!(
                    "Fold {} arrives at input {} / {arrival}, not {}",
                    fold.id, fold.to.element, fold.to.port
                ));
            }
            folds.push((fold, route));
        }
        let mut assertions = HashSet::new();
        for assertion in &self.assertions {
            let [a, b] = assertion
                .branches
                .each_ref()
                .map(|id| by_id.get(id).copied());
            let (Some(a), Some(b)) = (a, b) else {
                return Err(format!(
                    "Unknown branch reference in assertion {}",
                    assertion.id
                ));
            };
            if !(1..=64).contains(&assertion.id.len())
                || !assertions.insert(&assertion.id)
                || (a.id == b.id && assertion.kind != AssertionKind::Parity)
                || !assertion.target.is_finite()
                || !assertion.tolerance.is_finite()
                || assertion.tolerance < 0.
            {
                return Err(format!("Invalid assertion {}", assertion.id));
            }
            if assertion.kind == AssertionKind::Parity
                && !(matches!(assertion.unit, AssertionUnit::Count)
                    && (assertion.target == 0. || assertion.target == 1.))
            {
                return Err(format!(
                    "Parity assertion {} needs unit count and target 0 or 1",
                    assertion.id
                ));
            }
            let dispersion = assertion.kind == AssertionKind::Dispersion;
            if dispersion != assertion.band_nm.is_some()
                || dispersion != assertion.quantity.is_some()
            {
                return Err(format!(
                    "Assertion {} needs band_nm and quantity exactly when it is a dispersion assertion",
                    assertion.id
                ));
            }
            let valid = match assertion.kind {
                AssertionKind::Dispersion => {
                    let band_ok = assertion.band_nm.is_some_and(|[lo, hi]| {
                        (100. ..=20000.).contains(&lo) && (100. ..=20000.).contains(&hi) && lo < hi
                    });
                    band_ok
                        && matches!(
                            (assertion.quantity, assertion.unit),
                            (Some(DispersionQuantity::Phase), AssertionUnit::Rad)
                                | (Some(DispersionQuantity::GroupDelay), AssertionUnit::Fs)
                        )
                }
                AssertionKind::EqualOpl => {
                    matches!(assertion.unit, AssertionUnit::Mm | AssertionUnit::Um)
                }
                AssertionKind::Colinear => {
                    matches!(
                        assertion.unit,
                        AssertionUnit::Mm | AssertionUnit::Um | AssertionUnit::Mrad
                    )
                }
                AssertionKind::Parity => true,
                AssertionKind::ImageRotation => {
                    matches!(assertion.unit, AssertionUnit::Rad | AssertionUnit::Mrad)
                }
            };
            if !valid {
                return Err(format!(
                    "Invalid unit or comparison endpoints in assertion {}",
                    assertion.id
                ));
            }
        }
        let mut relays = HashSet::new();
        for relay in &self.relays {
            let finite = |x: f64| x.is_finite() && x.abs() <= 1e6;
            if !(1..=64).contains(&relay.id.len())
                || !relays.insert(&relay.id)
                || !by_id.contains_key(&relay.branch)
                || !finite(relay.object_distance_mm)
                || !finite(relay.image_distance_mm)
                || relay.checks.len() > 8
                || relay.checks.iter().any(|c| {
                    !c.target.is_finite()
                        || (c.kind != RelayCheckKind::Magnification && c.target != 0.)
                        || !c.tolerance.is_finite()
                        || c.tolerance < 0.
                })
                || [&relay.pupil_stop, &relay.pupil_target]
                    .into_iter()
                    .flatten()
                    .any(|id| {
                        !self.elements.iter().any(|e| &e.id == id)
                            && !self.sources.iter().any(|s| &s.id == id)
                    })
            {
                return Err(format!("Invalid relay {}", relay.id));
            }
        }
        Ok(NominalPaths {
            routes,
            folds,
            inputs,
        })
    }
}

impl NominalPaths<'_> {
    pub(crate) fn input(&self, from: &Port) -> &str {
        self.inputs
            .get(&(from.element.as_str(), from.port.as_str()))
            .copied()
            .unwrap_or("in")
    }

    pub(crate) fn branch_ending_at(&self, from: &Port, to: &str) -> Option<&Branch> {
        self.routes.iter().find_map(|(branch, route)| {
            (branch.to.element == to
                && route.last() == Some(&(from.element.as_str(), from.port.as_str())))
            .then_some(*branch)
        })
    }
}

/// Free-space or prescription flight. Indices refer to the full trace's segments array.
#[derive(schemars::JsonSchema, Clone, Debug, Serialize)]
pub struct Flight {
    pub from_element: String,
    pub from_port: String,
    pub to_element: String,
    pub to_port: String,
    pub length_mm: f64,
    pub medium: String,
    pub index: f64,
    pub opl_mm: f64,
    pub wavelength_nm: f64,
    pub previous_segment: Option<usize>,
    /// Scalar phase picked up when leaving `from_port` (phase plates), radians.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub phase_rad: f64,
}

fn is_zero(value: &f64) -> bool {
    *value == 0.
}

#[derive(Clone)]
pub(super) struct Departure {
    pub port: Port,
    /// Physical predecessor and bench-level arrival differ when leaving a lens group.
    pub previous: Option<usize>,
    pub arrival: Option<usize>,
    pub nominal: bool,
    /// Phase-plate phase added on leaving `port` (radians).
    pub phase: f64,
}

impl Departure {
    pub(super) fn flight(
        &self,
        to: Port,
        length_mm: f64,
        medium: &str,
        index: f64,
        wavelength_nm: f64,
    ) -> Flight {
        Flight {
            from_element: self.port.element.clone(),
            from_port: self.port.port.clone(),
            to_element: to.element,
            to_port: to.port,
            length_mm,
            medium: medium.into(),
            index,
            opl_mm: length_mm * index,
            wavelength_nm,
            previous_segment: self.previous,
            phase_rad: self.phase,
        }
    }
}

#[derive(schemars::JsonSchema, Clone, Debug, Serialize)]
pub(crate) struct PathBreak {
    pub element: String,
    pub port: String,
    pub reason: String,
    /// Human-readable cause, for example the failing material and wavelength.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl PathBreak {
    pub(crate) fn new(element: &str, port: &str, reason: &str) -> Self {
        Self {
            element: element.into(),
            port: port.into(),
            reason: reason.into(),
            detail: None,
        }
    }
    pub(crate) fn with_detail(mut self, detail: String) -> Self {
        self.detail = Some(detail);
        self
    }
}

#[derive(schemars::JsonSchema, Debug, Serialize)]
pub struct BranchReport {
    pub id: String,
    /// One path per on-axis specular arrival at the starting element.
    pub paths: Vec<BranchPath>,
    #[serde(rename = "break", skip_serializing_if = "Option::is_none")]
    pub(crate) interruption: Option<PathBreak>,
    #[serde(skip)]
    pub(in crate::optics) stage_ids: Vec<String>,
}

#[derive(schemars::JsonSchema, Clone, Debug, Serialize)]
pub struct BranchPath {
    pub source: String,
    pub wavelength_nm: f64,
    /// Incoming segment at the split, shared by sibling arms. Null at a source output.
    pub start_segment: Option<usize>,
    pub length_mm: f64,
    pub opl_mm: f64,
    /// 2π·OPL/λ plus phase-plate phases along the path (radians).
    pub phase_rad: f64,
    /// Substrate glass crossed along the path, in order.
    pub glass: Vec<GlassPass>,
    pub ports: Vec<Port>,
    pub nominal_match: bool,
    #[serde(rename = "break", skip_serializing_if = "Option::is_none")]
    pub(crate) interruption: Option<PathBreak>,
    #[serde(skip)]
    pub(crate) end_segment: Option<usize>,
    #[serde(skip)]
    pub(in crate::optics) segment_indices: Vec<usize>,
}

/// One crossing of an element substrate (an internal reflection is one pass).
#[derive(schemars::JsonSchema, Clone, Debug, Serialize)]
pub struct GlassPass {
    pub element: String,
    pub material: String,
    pub thickness_mm: f64,
    /// Geometric length inside the glass (mm).
    pub length_mm: f64,
}

enum Step {
    Flight(usize),
    Stopped(Option<usize>, PathBreak),
}

#[derive(Default)]
pub(super) struct PathIndex {
    departures: HashMap<(Option<usize>, Port), Step>,
    arrivals: HashMap<String, Vec<usize>>,
    group_stops: HashMap<usize, (usize, PathBreak)>,
    /// Arrivals on a region plate whose representative sample was masked or clipped
    /// but which anchor the plate's region arms; paths continue through them and
    /// whether an arm is lit is judged from its sampled port power.
    region_anchors: HashSet<usize>,
}

impl PathIndex {
    pub(super) fn hit(
        &mut self,
        departure: &Departure,
        segment: usize,
        element: &str,
        accepted: bool,
    ) {
        if departure.nominal {
            self.departures.insert(
                (departure.arrival, departure.port.clone()),
                Step::Flight(segment),
            );
            if accepted {
                self.arrivals
                    .entry(element.into())
                    .or_default()
                    .push(segment);
            }
        }
    }

    pub(super) fn anchor_regions(&mut self, segment: usize) {
        self.region_anchors.insert(segment);
    }

    pub(super) fn stop(&mut self, departure: &Departure, location: PathBreak) {
        if departure.nominal {
            self.departures.insert(
                (departure.arrival, departure.port.clone()),
                Step::Stopped(departure.previous, location),
            );
        }
    }

    pub(super) fn group_stop(
        &mut self,
        nominal: bool,
        arrival: usize,
        last: usize,
        location: PathBreak,
    ) {
        if nominal {
            self.group_stops.insert(arrival, (last, location));
        }
    }

    pub(super) fn reports(
        &self,
        bench: &Bench,
        nominal: &NominalPaths<'_>,
        segments: &[Segment],
    ) -> Vec<BranchReport> {
        self.route_reports(bench, &nominal.routes, segments)
    }

    pub(super) fn fold_reports(
        &self,
        bench: &Bench,
        nominal: &NominalPaths<'_>,
        segments: &[Segment],
    ) -> Vec<BranchReport> {
        self.route_reports(bench, &nominal.folds, segments)
    }

    fn route_reports(
        &self,
        bench: &Bench,
        routes: &[(&Branch, Vec<Edge<'_>>)],
        segments: &[Segment],
    ) -> Vec<BranchReport> {
        let stage_by_member: HashMap<_, _> = bench
            .stages
            .iter()
            .flat_map(|stage| {
                stage
                    .members
                    .iter()
                    .map(|member| (member.as_str(), stage.id.as_str()))
            })
            .collect();
        routes
            .iter()
            .map(|(branch, route)| {
                let paths: Vec<_> = if let Some(source) =
                    bench.sources.iter().find(|s| s.id == branch.from.element)
                {
                    vec![self.follow(branch, route, segments, None, &source.id, source.wavelength)]
                } else {
                    self.arrivals
                        .get(&branch.from.element)
                        .into_iter()
                        .flatten()
                        .map(|&start| {
                            let hit = &segments[start];
                            self.follow(
                                branch,
                                route,
                                segments,
                                Some(start),
                                &hit.source,
                                hit.flight.as_ref().unwrap().wavelength_nm,
                            )
                        })
                        .collect()
                };
                let mut paths = paths;
                for path in &mut paths {
                    path.fill_thickness(bench);
                }
                let interruption = paths.is_empty().then(|| {
                    PathBreak::new(&branch.from.element, &branch.from.port, "start_not_reached")
                });
                let stage_ids: HashSet<_> = route
                    .iter()
                    .map(|&(element, _)| element)
                    .chain([branch.to.element.as_str()])
                    .filter_map(|element| stage_by_member.get(element).copied())
                    .collect();
                BranchReport {
                    id: branch.id.clone(),
                    paths,
                    interruption,
                    stage_ids: bench
                        .stages
                        .iter()
                        .filter(|stage| stage_ids.contains(stage.id.as_str()))
                        .map(|stage| stage.id.clone())
                        .collect(),
                }
            })
            .collect()
    }

    fn follow(
        &self,
        branch: &Branch,
        route: &[Edge<'_>],
        segments: &[Segment],
        start_segment: Option<usize>,
        source: &str,
        wavelength_nm: f64,
    ) -> BranchPath {
        let mut result = BranchPath {
            source: source.into(),
            wavelength_nm,
            start_segment,
            length_mm: 0.,
            opl_mm: 0.,
            phase_rad: 0.,
            glass: Vec::new(),
            ports: Vec::new(),
            nominal_match: false,
            interruption: None,
            end_segment: None,
            segment_indices: Vec::new(),
        };
        let mut arrival = start_segment;
        for (i, &(element, port)) in route.iter().enumerate() {
            let output = Port::new(element, port);
            let next = self.departures.get(&(arrival, output.clone()));
            if let Some(Step::Flight(at)) = next {
                let hit = &segments[*at];
                let flight = hit.flight.as_ref().unwrap();
                let start = if i == 0 {
                    // The starting element's own substrate belongs to this arm.
                    let mut start = flight.previous_segment;
                    while let Some(at) = start.filter(|&at| {
                        segments[at].status == "substrate"
                            && segments[at].element.as_deref() == Some(element)
                    }) {
                        start = segments[at].flight.as_ref().unwrap().previous_segment;
                    }
                    start
                } else {
                    arrival
                };
                result.append_flights(segments, start, *at);
                let expected = route
                    .get(i + 1)
                    .map_or(branch.to.element.as_str(), |edge| edge.0);
                let passable = hit.status == "hit" || self.region_anchors.contains(at);
                if !passable || flight.to_element != expected {
                    result.interruption = Some(PathBreak::new(
                        &flight.to_element,
                        &flight.to_port,
                        &hit.status,
                    ));
                    return result;
                }
                arrival = Some(*at);
            } else {
                let interruption = if let Some(Step::Stopped(last, location)) = next {
                    if i > 0
                        && let Some(last) = last
                    {
                        result.append_flights(segments, arrival, *last);
                    }
                    result.push_port(output);
                    location.clone()
                } else if let Some((last, location)) =
                    arrival.and_then(|at| self.group_stops.get(&at))
                {
                    if i > 0 {
                        result.append_flights(segments, arrival, *last);
                    }
                    location.clone()
                } else {
                    PathBreak::new(element, port, "no_output")
                };
                result.interruption = Some(interruption);
                return result;
            }
        }
        result.nominal_match = true;
        result.end_segment = arrival;
        result
    }
}

impl BranchPath {
    /// The segments `start` descends from, `start` first, back to the source launch.
    pub(in crate::optics) fn ancestry(segments: &[Segment], start: Option<usize>) -> Vec<usize> {
        std::iter::successors(start, |&at| {
            segments[at]
                .flight
                .as_ref()
                .and_then(|f| f.previous_segment)
        })
        .collect()
    }

    /// The last segment before the path's own first one: the arrival at the
    /// start element, or past it when the start element (a lens group) has
    /// internal propagation the path leaves out.
    pub(in crate::optics) fn attachment(&self, segments: &[Segment]) -> Option<usize> {
        match self.segment_indices.first() {
            Some(&first) => segments[first]
                .flight
                .as_ref()
                .and_then(|f| f.previous_segment),
            None => self.start_segment,
        }
    }

    fn fill_thickness(&mut self, bench: &Bench) {
        for pass in &mut self.glass {
            pass.thickness_mm = bench
                .elements
                .iter()
                .find(|e| e.id == pass.element)
                .and_then(|e| e.substrate.as_ref())
                .map_or(0., |s| s.thickness_mm);
        }
    }

    /// The same path measured from `origin`, a segment its attachment descends
    /// from (`None` is the source launch): the propagation in between is prepended.
    pub(in crate::optics) fn rooted_at(
        &self,
        bench: &Bench,
        segments: &[Segment],
        origin: Option<usize>,
    ) -> BranchPath {
        let mut rooted = BranchPath {
            start_segment: origin,
            length_mm: 0.,
            opl_mm: 0.,
            phase_rad: 0.,
            glass: Vec::new(),
            ports: Vec::new(),
            segment_indices: Vec::new(),
            ..self.clone()
        };
        if let Some(attachment) = self.attachment(segments).filter(|&at| Some(at) != origin) {
            rooted.append_flights(segments, origin, attachment);
            rooted.fill_thickness(bench);
        }
        rooted.length_mm += self.length_mm;
        rooted.opl_mm += self.opl_mm;
        rooted.phase_rad += self.phase_rad;
        rooted.glass.extend(self.glass.iter().cloned());
        for port in &self.ports {
            rooted.push_port(port.clone());
        }
        rooted.segment_indices.extend(&self.segment_indices);
        rooted
    }

    fn push_port(&mut self, port: Port) {
        if self.ports.last() != Some(&port) {
            self.ports.push(port);
        }
    }

    fn append_flights(&mut self, segments: &[Segment], start: Option<usize>, last: usize) {
        let mut between = Vec::new();
        let mut next = Some(last);
        while next != start {
            let at = next.expect("linked propagation");
            between.push(at);
            next = segments[at].flight.as_ref().unwrap().previous_segment;
        }
        for at in between.into_iter().rev() {
            let flight = segments[at].flight.as_ref().unwrap();
            self.segment_indices.push(at);
            self.length_mm += flight.length_mm;
            self.opl_mm += flight.opl_mm;
            self.phase_rad += std::f64::consts::TAU * flight.opl_mm / (flight.wavelength_nm * 1e-6)
                + flight.phase_rad;
            if segments[at].status == "substrate" {
                match self.glass.last_mut() {
                    Some(pass)
                        if pass.element == flight.to_element
                            && self.ports.last()
                                == Some(&Port::new(&flight.from_element, &flight.from_port)) =>
                    {
                        pass.length_mm += flight.length_mm;
                    }
                    _ => self.glass.push(GlassPass {
                        element: flight.to_element.clone(),
                        material: flight.medium.clone(),
                        thickness_mm: 0.,
                        length_mm: flight.length_mm,
                    }),
                }
            }
            self.push_port(Port::new(&flight.from_element, &flight.from_port));
            self.push_port(Port::new(&flight.to_element, &flight.to_port));
        }
    }
}
