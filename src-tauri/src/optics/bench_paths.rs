//! Nominal bench routes and their correspondence with actual propagation.
use super::{Bench, Segment, ports};
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
}

#[derive(schemars::JsonSchema, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssertionKind {
    EqualOpl,
    Colinear,
}

#[derive(schemars::JsonSchema, Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssertionUnit {
    Mm,
    Um,
    Mrad,
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
        }
    }
}

#[derive(schemars::JsonSchema, Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assertion {
    pub id: String,
    pub kind: AssertionKind,
    /// Two distinct branch IDs. Equal OPL takes the first branch minus the second.
    pub branches: [String; 2],
    pub target: f64,
    pub unit: AssertionUnit,
    #[schemars(range(min = 0.0))]
    pub tolerance: f64,
}

type Edge<'a> = (&'a str, &'a str);
type Connections<'a> = HashMap<&'a str, Vec<Edge<'a>>>;

pub(in crate::optics) struct NominalPaths<'a> {
    routes: Vec<(&'a Branch, Vec<Edge<'a>>)>,
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
        if self.branches.len() > 128 || self.assertions.len() > 128 {
            return Err("Bench branch or assertion budget exceeded".into());
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
                || a.id == b.id
                || !assertion.target.is_finite()
                || !assertion.tolerance.is_finite()
                || assertion.tolerance < 0.
            {
                return Err(format!("Invalid assertion {}", assertion.id));
            }
            let valid = match assertion.kind {
                AssertionKind::EqualOpl => {
                    matches!(assertion.unit, AssertionUnit::Mm | AssertionUnit::Um)
                        && a.from.element == b.from.element
                        && a.from.port != b.from.port
                        && a.to.element == b.to.element
                        && self
                            .elements
                            .iter()
                            .any(|e| e.id == a.from.element && ports(e.kind).len() > 1)
                }
                AssertionKind::Colinear => {
                    matches!(
                        assertion.unit,
                        AssertionUnit::Mm | AssertionUnit::Um | AssertionUnit::Mrad
                    )
                }
            };
            if !valid {
                return Err(format!(
                    "Invalid unit or comparison endpoints in assertion {}",
                    assertion.id
                ));
            }
        }
        Ok(NominalPaths { routes, inputs })
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
}

#[derive(Clone)]
pub(super) struct Departure {
    pub port: Port,
    /// Physical predecessor and bench-level arrival differ when leaving a lens group.
    pub previous: Option<usize>,
    pub arrival: Option<usize>,
    pub nominal: bool,
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
        }
    }
}

#[derive(schemars::JsonSchema, Clone, Debug, Serialize)]
pub(crate) struct PathBreak {
    pub element: String,
    pub port: String,
    pub reason: String,
}

impl PathBreak {
    pub(crate) fn new(element: &str, port: &str, reason: &str) -> Self {
        Self {
            element: element.into(),
            port: port.into(),
            reason: reason.into(),
        }
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

#[derive(schemars::JsonSchema, Debug, Serialize)]
pub struct BranchPath {
    pub source: String,
    pub wavelength_nm: f64,
    /// Incoming segment at the split, shared by sibling arms. Null at a source output.
    pub start_segment: Option<usize>,
    pub length_mm: f64,
    pub opl_mm: f64,
    pub ports: Vec<Port>,
    pub nominal_match: bool,
    #[serde(rename = "break", skip_serializing_if = "Option::is_none")]
    pub(crate) interruption: Option<PathBreak>,
    #[serde(skip)]
    pub(crate) end_segment: Option<usize>,
    #[serde(skip)]
    pub(in crate::optics) segment_indices: Vec<usize>,
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
        let stage_by_member: HashMap<_, _> = bench.stages.iter().flat_map(|stage| {
            stage.members.iter().map(|member| (member.as_str(), stage.id.as_str()))
        }).collect();
        nominal
            .routes
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
                let interruption = paths.is_empty().then(|| {
                    PathBreak::new(&branch.from.element, &branch.from.port, "start_not_reached")
                });
                let stage_ids: HashSet<_> = route.iter().map(|&(element, _)| element)
                    .chain([branch.to.element.as_str()])
                    .filter_map(|element| stage_by_member.get(element).copied())
                    .collect();
                BranchReport {
                    id: branch.id.clone(),
                    paths,
                    interruption,
                    stage_ids: bench.stages.iter()
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
                    flight.previous_segment
                } else {
                    arrival
                };
                result.append_flights(segments, start, *at);
                let expected = route
                    .get(i + 1)
                    .map_or(branch.to.element.as_str(), |edge| edge.0);
                if hit.status != "hit" || flight.to_element != expected {
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
            self.push_port(Port::new(&flight.from_element, &flight.from_port));
            self.push_port(Port::new(&flight.to_element, &flight.to_port));
        }
    }
}
