//! Modal projection and balanced readout on a detector (`bench_readout`).
//!
//! Every named branch ending at the detector's `in` port is projected along
//! its direction onto the detector face. A port reads every arm that puts more
//! than 1e-12 of its power inside the port's ROI, so a spot straddling two ROIs
//! feeds both. The arms of one port are mapped onto the detector face itself,
//! on a grid of the source pitch centred on the ROI centre with axes along the
//! detector-local y and z, and superposed coherently exactly as `bench_field`
//! superposes arms at a port (same Jones projection, coherence factor, flux
//! weights through the face and obliquity). Every arm keeps its spatial carrier,
//! so a tilted beam reads with its transverse phase and stretched footprint; a
//! carrier the grid cannot sample is an error asking for a finer pitch_um.
//! Samples outside the ROI or outside the detector's circular aperture are
//! discarded; an ROI narrower than two samples of any arm's grid is an error.
//!
//! Each port then projects the masked field on the mode basis, centred on the
//! ROI centre with axes along the detector-local y and z. Modes narrower than
//! two samples per feature (waist / √(2N+1) at order N) on the grid of the port
//! that reads them are rejected. For arm amplitudes c_a = ∫u*·A_a
//! the mode reading is Σ_a |c_a|² + Σ_{a<b} 2·Re(c_a·conj(c_b)·W_ab), the same
//! weighted sum that gives the port intensity, averaged over ground-glass
//! realizations. `fraction` divides the unit-power mode reading by the ROI power.
//!
//! A balanced pair reads plus minus minus, both for the ROI powers and for each
//! mode; pairs sharing a `signal` add into its real or imaginary component.

use super::super::super::{
    System,
    bench::{Action, Bench, Element, Kind, ModeBasis, ModeFamily, ModeNormalization, Port, Quadrature, Readout, local, rotate},
    math::{V3, add, dot, scale, sub, unit},
};
use super::{Arm, Band, C, Coupling, FieldRequest, M2, Screens, apply, arm, carrier_step, compute, couple_on, intensity, map, select};
use serde::Serialize;
use std::f64::consts::PI;

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ModeReading {
    pub index: String,
    /// |∫u*·E|² with the requested mode normalization, in source power units.
    pub intensity: f64,
    /// Unit-power mode reading divided by the ROI power.
    pub fraction: f64,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct PortReading {
    pub id: String,
    /// Arms whose chief ray lands in the ROI.
    pub arms: Vec<String>,
    /// Power inside the ROI.
    pub power: f64,
    pub modes: Vec<ModeReading>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ModeSignal {
    pub index: String,
    pub real: f64,
    pub imag: f64,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct SignalReading {
    pub pair: String,
    /// Balanced ROI power differences.
    pub real: f64,
    pub imag: f64,
    /// Balanced differences of the mode intensities.
    pub modes: Vec<ModeSignal>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ReadoutReading {
    pub detector: String,
    pub wavelength_nm: f64,
    pub realizations: usize,
    pub normalization: ModeNormalization,
    pub ports: Vec<PortReading>,
    pub signals: Vec<SignalReading>,
    pub warnings: Vec<String>,
}

/// Orthonormal Hermite functions h_n(t)·e^{−t²/2}, h_n = H_n/√(2ⁿn!), for n ≤ order.
fn hermite(order: usize, t: f64) -> Vec<f64> {
    let mut h = vec![0.; order + 1];
    h[0] = 1.;
    if order > 0 {
        h[1] = 2f64.sqrt() * t;
    }
    for n in 1..order {
        let k = n as f64;
        h[n + 1] = (2. / (k + 1.)).sqrt() * t * h[n] - (k / (k + 1.)).sqrt() * h[n - 1];
    }
    let gauss = (-t * t / 2.).exp();
    h.iter_mut().for_each(|v| *v *= gauss);
    h
}

/// Generalized Laguerre polynomial L_p^a(x).
fn laguerre(p: usize, a: f64, x: f64) -> f64 {
    let (mut previous, mut current) = (1., 1. + a - x);
    if p == 0 {
        return previous;
    }
    for k in 1..p {
        let k = k as f64;
        let next = ((2. * k + 1. + a - x) * current - (k + a) * previous) / (k + 1.);
        previous = current;
        current = next;
    }
    current
}

fn factorial(n: usize) -> f64 {
    (1..=n).map(|k| k as f64).product()
}

/// Unit-power mode functions u(X, Y) (mm⁻¹) of the basis in mode coordinates.
struct Modes<'a> {
    basis: &'a ModeBasis,
    w: f64,
    order: usize,
    /// Custom modes as unit-norm coefficients of distinct HG indices.
    custom: Vec<Vec<([u32; 2], C)>>,
}

impl<'a> Modes<'a> {
    fn new(basis: &'a ModeBasis) -> Self {
        let order = match basis.basis {
            ModeFamily::Custom => basis.custom.iter().flat_map(|c| &c.terms).flat_map(|t| t.hg).max().unwrap_or(0) as usize,
            _ => basis.indices.iter().flatten().map(|i| i.unsigned_abs()).max().unwrap_or(0) as usize,
        };
        let custom = basis
            .custom
            .iter()
            .map(|mode| {
                // Scale by the largest coefficient first so tiny or huge
                // coefficients neither underflow nor overflow when squared.
                let terms = mode.merged();
                let largest = terms.iter().map(|(_, c)| c[0].hypot(c[1])).fold(0., f64::max);
                let total = terms.iter().map(|(_, c)| (c[0] / largest).powi(2) + (c[1] / largest).powi(2)).sum::<f64>().sqrt();
                terms.into_iter().map(|(hg, c)| (hg, C::new(c[0] / largest / total, c[1] / largest / total))).collect()
            })
            .collect();
        Self { basis, w: basis.waist_um * 1e-3, order, custom }
    }

    fn count(&self) -> usize {
        match self.basis.basis {
            ModeFamily::Custom => self.basis.custom.len(),
            _ => self.basis.indices.len(),
        }
    }

    /// Every mode at (X, Y).
    fn eval(&self, [x, y]: [f64; 2]) -> Vec<C> {
        let w = self.w;
        match self.basis.basis {
            ModeFamily::HermiteGauss | ModeFamily::Custom => {
                // ψ_m(x) = (2/π)^¼ w^−½ h_m(√2x/w) e^{−x²/w²}
                let norm = (2. / PI).sqrt() / w;
                let hx = hermite(self.order, 2f64.sqrt() * x / w);
                let hy = hermite(self.order, 2f64.sqrt() * y / w);
                if self.basis.basis == ModeFamily::HermiteGauss {
                    return self.basis.indices.iter().map(|[m, n]| C::new(norm * hx[*m as usize] * hy[*n as usize], 0.)).collect();
                }
                self.custom
                    .iter()
                    .map(|terms| terms.iter().fold(C::ZERO, |s, (hg, c)| s + c.scale(norm * hx[hg[0] as usize] * hy[hg[1] as usize])))
                    .collect()
            }
            ModeFamily::LaguerreGauss => {
                let r2 = (x * x + y * y) / (w * w);
                let phi = y.atan2(x);
                self.basis
                    .indices
                    .iter()
                    .map(|&[p, l]| {
                        let (p, a) = (p as usize, l.unsigned_abs() as usize);
                        let norm = (2. * factorial(p) / (PI * factorial(p + a))).sqrt() / w;
                        let radial = norm * (2. * r2).sqrt().powi(a as i32) * laguerre(p, a as f64, 2. * r2) * (-r2).exp();
                        C::cis(l as f64 * phi).scale(radial)
                    })
                    .collect()
            }
        }
    }

    /// Peak |u| of every mode: a scan over the mode's extent, refined three
    /// times by zooming twentyfold around the best sample.
    fn peaks(&self) -> Vec<f64> {
        let reach = ((2. * self.order as f64 + 1.).sqrt() + 3.) * self.w;
        (0..self.count())
            .map(|m| {
                let value = |p: [f64; 2]| self.eval(p)[m].norm2().sqrt();
                let (mut center, mut half, mut best) = ([0.; 2], reach, 0.);
                for _ in 0..4 {
                    const N: i32 = 120;
                    let mut found = center;
                    for i in -N..=N {
                        for j in -N..=N {
                            let p = [center[0] + half * i as f64 / N as f64, center[1] + half * j as f64 / N as f64];
                            let v = value(p);
                            if v > best {
                                best = v;
                                found = p;
                            }
                        }
                    }
                    center = found;
                    half /= 20.;
                }
                best
            })
            .collect()
    }
}

/// Read the modal projections and balanced signals of `detector`.
pub(crate) fn readout(
    bench: &Bench,
    system: &System,
    detector: &str,
    realizations: Option<usize>,
) -> Result<ReadoutReading, String> {
    let scene = bench.resolved_stages()?;
    let bench = scene.as_ref();
    bench.validate()?;
    let element = bench
        .elements
        .iter()
        .find(|e| e.id == detector)
        .ok_or_else(|| format!("Unknown detector {detector}"))?;
    let config = match (&element.kind, &element.readout) {
        (Kind::Detector, Some(r)) => r,
        _ => return Err(format!("{detector} is not a detector with a readout (ports, modes, balanced_pairs)")),
    };
    let realizations = realizations.unwrap_or(1);
    if !(1..=256).contains(&realizations) {
        return Err("realizations must be 1..256".into());
    }
    let trace = bench.trace_resolved(system)?;
    let request = FieldRequest {
        branch: None,
        port: Some(Port::new(detector, "in")),
        wavelength: None,
        realizations: Some(realizations),
        arrays: false,
    };
    let arms = select(bench, &trace, &request)?
        .into_iter()
        .map(|(label, path)| arm(bench, &trace, label, path))
        .collect::<Result<Vec<_>, _>>()?;
    let wavelength_nm = arms[0].wavelength_nm;
    let mut warnings = Vec::new();
    let random = arms.iter().any(|a| a.events.iter().any(|e| matches!(e.element.action, Some(Action::Rgg { .. }))));
    let runs = if random { realizations } else { 1 };
    // Share of every arm's power inside every ROI, from its first realization.
    let (footprints, spacings): (Vec<_>, Vec<_>) = {
        let (mut screens, mut band) = (Screens::new(), Band { sin: 1., dropped: 0. });
        arms.iter()
            .map(|a| Ok(footprint(element, a, &compute(&trace, a, 0, &mut screens, &mut band)?, &config.ports)))
            .collect::<Result<Vec<_>, String>>()?
            .into_iter()
            .unzip()
    };
    for (a, shares) in arms.iter().zip(&footprints) {
        if shares.iter().all(|s| *s <= LIT) {
            warnings.push(format!("Branch {} puts no power inside any readout ROI on {detector}; it is not read", a.label));
        }
    }
    let order = config.modes.sampling_order();
    let resolves = |pitch: f64| -> Result<(), String> {
        let spread = ((2 * order + 1) as f64).sqrt();
        if config.modes.waist_um * 1e-3 / spread < 2. * pitch {
            return Err(format!(
                "Readout modes on {detector} (waist {} µm, order {order}) need at least two samples per feature; the {:.1} µm field grid resolves waist_um ≥ {:.1} at this order — raise waist_um or refine pitch_um",
                config.modes.waist_um,
                pitch * 1e3,
                2. * pitch * 1e3 * spread
            ));
        }
        Ok(())
    };
    // Each ROI must span enough samples of every grid that could read it, as
    // projected onto the face (an oblique beam stretches its grid there), so
    // membership never hinges on whether a sample happens to fall inside.
    let coarsest = spacings.iter().copied().fold(0., f64::max);
    // The effective region is the ROI clipped to the aperture. A convex region
    // free of lattice points has lattice width at most 1 + 2/√3 ≈ 2.15 samples
    // (Hurkens 1990), so a minimum width of WIDE samples guarantees that every
    // grid, however offset, puts samples inside it and membership never hinges
    // on where they happen to fall.
    const WIDE: f64 = 2.5;
    for port in &config.ports {
        let (area, width) = effective(port.roi, element.aperture);
        if area > 0. && width < WIDE * coarsest {
            return Err(format!(
                "Readout port {} reads a region {:.3} mm wide inside the {:.3} mm aperture radius of {detector}, narrower than {WIDE} samples of the field grid as projected onto the face ({:.1} µm, stretched by oblique incidence); enlarge the ROI or aperture, or refine pitch_um",
                port.id,
                width,
                element.aperture,
                coarsest * 1e3
            ));
        }
        if area == 0. {
            warnings.push(format!("Readout port {} lies outside the aperture of {detector}; it reads nothing", port.id));
            continue;
        }
        let [cy, cz, w, h] = port.roi;
        if [[-1., -1.], [-1., 1.], [1., -1.], [1., 1.]].iter().any(|[a, b]| (cy + a * w / 2.).hypot(cz + b * h / 2.) > element.aperture) {
            warnings.push(format!(
                "Readout port {} extends beyond the {:.2} mm aperture radius of {detector}; only the part inside the aperture is read",
                port.id, element.aperture
            ));
        }
    }
    let modes = Modes::new(&config.modes);
    let labels = config.modes.labels();
    let peaks = match config.modes.normalization {
        ModeNormalization::Power => vec![1.; labels.len()],
        ModeNormalization::Amplitude => modes.peaks(),
    };
    // Per port: member arm indices (brightest first), coupling, the reference
    // frame through the ROI centre, and the ROI plane.
    let plans = config
        .ports
        .iter()
        .enumerate()
        .map(|(p, port)| {
            let mut members: Vec<usize> = (0..arms.len()).filter(|&a| footprints[a][p] > LIT).collect();
            if members.is_empty() {
                warnings.push(format!("Readout port {} receives no named branch", port.id));
                return Ok(None);
            }
            members.sort_by(|&a, &b| footprints[b][p].total_cmp(&footprints[a][p]));
            let group: Vec<Arm> = members.iter().map(|&a| arms[a].clone()).collect();
            // The ROI on the detector face: centre, face normal facing along
            // the light, and detector-local y, z axes.
            let r = element.rotation();
            let (normal, ny, nz) = (rotate([1., 0., 0.], r), rotate([0., 1., 0.], r), rotate([0., 0., 1.], r));
            let mut reference = group[0].clone();
            reference.label = format!("{detector} port {}", port.id);
            reference.end_point = add(element.position, add(scale(ny, port.roi[0]), scale(nz, port.roi[1])));
            reference.end.direction = scale(normal, dot(group[0].end.direction, normal).signum());
            reference.end.basis = [ny, nz];
            for arm in &group {
                let step = carrier_step(arm, &reference);
                if step >= PI {
                    return Err(format!(
                        "Branch {} reaches readout port {} too obliquely for the {:.1} µm grid (carrier {:.2}π per sample); refine pitch_um",
                        arm.label,
                        port.id,
                        reference.grid.pitch * 1e3,
                        step / PI
                    ));
                }
            }
            resolves(reference.grid.pitch)?;
            let coupling = couple_on(&group, &reference)?;
            let window = reference.grid.n.map(|n| n as f64 * reference.grid.pitch);
            if port.roi[2] > window[0] || port.roi[3] > window[1] {
                warnings.push(format!(
                    "Readout port {} is larger than the {:.2} × {:.2} mm field window; its outer part is not sampled",
                    port.id, window[0], window[1]
                ));
            }
            let plane = Plane::new(element, &reference, port.roi, &modes);
            // The sampled mask must reproduce the effective area, or the grid
            // does not resolve the region's shape.
            let (area, _) = effective(port.roi, element.aperture);
            let sampled = plane.mask.iter().filter(|m| **m).count() as f64 * reference.grid.pitch.powi(2);
            if (sampled - area).abs() > 0.25 * area {
                return Err(format!(
                    "Readout port {} samples {sampled:.4} mm² of its {area:.4} mm² effective region on the {:.1} µm grid; enlarge the ROI or aperture, or refine pitch_um",
                    port.id,
                    reference.grid.pitch * 1e3
                ));
            }
            Ok(Some((members, coupling, reference, plane)))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let mut ports = Vec::new();
    let mut screens = Screens::new();
    let mut band = Band { sin: 1., dropped: 0. };
    let mut power = vec![0.; plans.len()];
    let mut readings = vec![vec![0.; labels.len()]; plans.len()];
    for realization in 0..runs {
        let mut computed: Vec<Option<(Vec<C>, M2)>> = vec![None; arms.len()];
        for (p, plan) in plans.iter().enumerate() {
            let Some((members, Coupling { weights, carriers, .. }, reference, plane)) = plan else { continue };
            let mut fields = Vec::with_capacity(members.len());
            for (&a, &carrier) in members.iter().zip(carriers) {
                if computed[a].is_none() {
                    computed[a] = Some(compute(&trace, &arms[a], realization, &mut screens, &mut band)?);
                }
                fields.push(map(&arms[a], reference, computed[a].clone().expect("computed above"), carrier)?);
            }
            let grid = reference.grid;
            let area = grid.pitch * grid.pitch;
            let mut near = vec![0.; grid.len()];
            intensity(&fields, weights, &mut near);
            power[p] += near.iter().zip(&plane.mask).filter(|(_, m)| **m).map(|(v, _)| v).sum::<f64>() * area;
            for (m, reading) in readings[p].iter_mut().enumerate() {
                let c: Vec<C> = fields
                    .iter()
                    .map(|f| {
                        f.iter()
                            .zip(&plane.modes)
                            .zip(&plane.mask)
                            .filter(|(_, inside)| **inside)
                            .fold(C::ZERO, |s, ((a, u), _)| s + *a * u[m].conj())
                            .scale(area)
                    })
                    .collect();
                let mut sum = 0.;
                for a in 0..c.len() {
                    sum += c[a].norm2();
                    for b in a + 1..c.len() {
                        sum += 2. * (c[a] * c[b].conj() * weights[a][b]).re;
                    }
                }
                *reading += sum;
            }
        }
        screens.retain(|(_, r), _| *r > realization);
    }
    let runs_f = runs as f64;
    for (p, port) in config.ports.iter().enumerate() {
        let total = power[p] / runs_f;
        ports.push(PortReading {
            id: port.id.clone(),
            arms: plans[p].as_ref().map_or(vec![], |(members, ..)| {
                let mut order = members.clone();
                order.sort_unstable();
                order.into_iter().map(|a| arms[a].label.clone()).collect()
            }),
            power: total,
            modes: labels
                .iter()
                .enumerate()
                .map(|(m, index)| {
                    let unit_power = readings[p][m] / runs_f;
                    ModeReading {
                        index: index.clone(),
                        intensity: unit_power / (peaks[m] * peaks[m]),
                        fraction: if total > 0. { unit_power / total } else { 0. },
                    }
                })
                .collect(),
        });
    }
    if band.dropped > 1e-3 {
        warnings.push(format!(
            "A flight dropped {:.2}% of its power beyond the band limit of {:.3} mrad; enlarge the window",
            100. * band.dropped,
            band.sin.asin() * 1e3
        ));
    }
    let signals = signals(config, &ports);
    Ok(ReadoutReading {
        detector: detector.into(),
        wavelength_nm,
        realizations: runs,
        normalization: config.modes.normalization,
        ports,
        signals,
        warnings,
    })
}

/// Share of an arm's power above which it counts as lighting a ROI.
const LIT: f64 = 1e-12;

/// Fraction of the arm's end-plane power whose projection along the arm onto
/// the detector face lands inside each ROI.
fn footprint(detector: &Element, arm: &Arm, (field, g): &(Vec<C>, M2), rois: &[crate::optics::bench::ReadoutPort]) -> (Vec<f64>, f64) {
    let r = detector.rotation();
    let normal = rotate([1., 0., 0.], r);
    let d = arm.end.direction;
    let [b0, b1] = arm.end.basis;
    let cos = dot(d, normal);
    // The sample lattice projected along d onto the face: its columns are the
    // images of one pitch step along each grid axis; the largest singular value
    // is the coarsest spacing it can have in any direction on the face.
    let column = |step: [f64; 2]| {
        let [x, y] = apply(*g, step);
        let v = add(scale(b0, x), scale(b1, y));
        let [_, ly, lz] = local(sub(v, scale(d, dot(v, normal) / cos)), r);
        [ly, lz]
    };
    let (a, b) = (column([arm.grid.pitch, 0.]), column([0., arm.grid.pitch]));
    let (p, q, t) = (a[0] * a[0] + a[1] * a[1], b[0] * b[0] + b[1] * b[1], a[0] * b[0] + a[1] * b[1]);
    let spacing = ((p + q) / 2. + (((p - q) / 2.).powi(2) + t * t).sqrt()).sqrt();
    let spacing = if spacing.is_finite() { spacing } else { f64::INFINITY };
    let mut shares = vec![0.; rois.len()];
    let mut total = 0.;
    for (i, v) in field.iter().enumerate() {
        let weight = v.norm2();
        total += weight;
        let [x, y] = apply(*g, arm.grid.coords(i));
        let point = add(arm.end_point, add(scale(b0, x), scale(b1, y)));
        let hit = add(point, scale(d, dot(sub(detector.position, point), normal) / cos));
        let [_, ly, lz] = local(sub(hit, detector.position), r);
        for (share, port) in shares.iter_mut().zip(rois) {
            if inside(port.roi, [ly, lz]) && ly.hypot(lz) <= detector.aperture {
                *share += weight;
            }
        }
    }
    shares.iter_mut().for_each(|s| *s /= total.max(f64::MIN_POSITIVE));
    (shares, spacing)
}

/// Area (mm²) and minimum width (mm) of an ROI clipped to a circular
/// aperture of radius `a` centred on the detector. The region is convex, so it
/// is clipped as a polygon and its minimum width found over edge normals.
fn effective([cy, cz, w, h]: [f64; 4], a: f64) -> (f64, f64) {
    const SIDES: usize = 1024;
    let mut poly: Vec<[f64; 2]> = (0..SIDES)
        .map(|k| {
            let t = 2. * PI * k as f64 / SIDES as f64;
            [a * t.cos(), a * t.sin()]
        })
        .collect();
    // Keep the side where `n · p <= limit`, for each ROI edge.
    for (n, limit) in [([1., 0.], cy + w / 2.), ([-1., 0.], w / 2. - cy), ([0., 1.], cz + h / 2.), ([0., -1.], h / 2. - cz)] {
        let side = |p: [f64; 2]| n[0] * p[0] + n[1] * p[1] - limit;
        let mut kept = Vec::with_capacity(poly.len() + 2);
        for (k, &p) in poly.iter().enumerate() {
            let q = poly[(k + 1) % poly.len()];
            let (sp, sq) = (side(p), side(q));
            if sp <= 0. {
                kept.push(p);
            }
            if (sp <= 0.) != (sq <= 0.) {
                let t = sp / (sp - sq);
                kept.push([p[0] + t * (q[0] - p[0]), p[1] + t * (q[1] - p[1])]);
            }
        }
        poly = kept;
        if poly.len() < 3 {
            return (0., 0.);
        }
    }
    let n = poly.len();
    let area = (0..n).map(|k| {
        let (p, q) = (poly[k], poly[(k + 1) % n]);
        p[0] * q[1] - q[0] * p[1]
    });
    let area = area.sum::<f64>().abs() / 2.;
    let width = (0..n)
        .filter_map(|k| {
            let (p, q) = (poly[k], poly[(k + 1) % n]);
            let len = (q[0] - p[0]).hypot(q[1] - p[1]);
            (len > 1e-12).then(|| {
                let normal = [(p[1] - q[1]) / len, (q[0] - p[0]) / len];
                let proj = poly.iter().map(|v| normal[0] * v[0] + normal[1] * v[1]);
                let (lo, hi) = proj.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), x| (lo.min(x), hi.max(x)));
                hi - lo
            })
        })
        .fold(f64::INFINITY, f64::min);
    (area, width)
}

/// Half-open ROI test, shifted by 1 pm so a sample on an edge shared by two
/// abutting ROIs counts once despite rounding.
fn inside([cy, cz, w, h]: [f64; 4], [y, z]: [f64; 2]) -> bool {
    let within = |v: f64, c: f64, size: f64| (c - size / 2. - 1e-9..c + size / 2. - 1e-9).contains(&v);
    within(y, cy, w) && within(z, cz, h)
}

/// ROI mask and sampled modes on a port's reference plane.
struct Plane {
    mask: Vec<bool>,
    modes: Vec<Vec<C>>,
}

impl Plane {
    fn new(detector: &Element, reference: &Arm, roi: [f64; 4], modes: &Modes) -> Self {
        let r = detector.rotation();
        let (normal, ny, nz) = (rotate([1., 0., 0.], r), rotate([0., 1., 0.], r), rotate([0., 0., 1.], r));
        let d = reference.end.direction;
        let [b0, b1] = reference.end.basis;
        let o = reference.end_point;
        let across = |v: V3| sub(v, scale(d, dot(v, d)));
        let ex = unit(across(ny)).unwrap_or(b0);
        let ey = unit(sub(across(nz), scale(ex, dot(nz, ex)))).unwrap_or(b1);
        let centre = sub(add(detector.position, add(scale(ny, roi[0]), scale(nz, roi[1]))), o);
        let [u0, v0] = [dot(centre, b0), dot(centre, b1)];
        let grid = reference.grid;
        let cos = dot(d, normal);
        let (mask, modes) = (0..grid.len())
            .map(|i| {
                let [x, y] = grid.coords(i);
                let point = add(o, add(scale(b0, x), scale(b1, y)));
                let hit = add(point, scale(d, dot(sub(detector.position, point), normal) / cos));
                let [_, ly, lz] = local(sub(hit, detector.position), r);
                let offset = add(scale(b0, x - u0), scale(b1, y - v0));
                (inside(roi, [ly, lz]) && ly.hypot(lz) <= detector.aperture, modes.eval([dot(offset, ex), dot(offset, ey)]))
            })
            .unzip();
        Self { mask, modes }
    }
}

fn signals(config: &Readout, ports: &[PortReading]) -> Vec<SignalReading> {
    let mut out: Vec<SignalReading> = Vec::new();
    let find = |id: &str| ports.iter().find(|p| p.id == id).expect("validated readout port");
    for pair in &config.balanced_pairs {
        let (plus, minus) = (find(&pair.plus), find(&pair.minus));
        let index = match out.iter().position(|s| s.pair == pair.signal) {
            Some(i) => i,
            None => {
                out.push(SignalReading {
                    pair: pair.signal.clone(),
                    real: 0.,
                    imag: 0.,
                    modes: plus.modes.iter().map(|m| ModeSignal { index: m.index.clone(), real: 0., imag: 0. }).collect(),
                });
                out.len() - 1
            }
        };
        let signal = &mut out[index];
        let component = |real: &mut f64, imag: &mut f64, value: f64| match pair.quadrature {
            Quadrature::Real => *real += value,
            Quadrature::Imag => *imag += value,
        };
        component(&mut signal.real, &mut signal.imag, plus.power - minus.power);
        for (m, mode) in signal.modes.iter_mut().enumerate() {
            component(&mut mode.real, &mut mode.imag, plus.modes[m].intensity - minus.modes[m].intensity);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::super::{DETECTOR, MIRROR, mount_element, template};
    use super::*;
    use crate::optics::bench::{
        BalancedPair, Branch, Connection, CustomMode, FieldProfile, FieldSampling, ModeBasis, ModeFamily, ModeNormalization,
        ModeTerm, PhasePlate, Port, Quadrature, Readout, ReadoutPort,
    };

    fn hybrid(phi: f64) -> Bench {
        let mut bench = template("modal_hybrid").unwrap();
        let at_wavelength_nm = bench.sources[0].wavelength;
        let bs0 = bench.elements.iter_mut().find(|e| e.id == "BS0").unwrap();
        bs0.phase = Some(PhasePlate { radians: Some(phi), waves: None, at_wavelength_nm });
        bench
    }

    fn basis(basis: ModeFamily, indices: &[[i32; 2]], normalization: ModeNormalization) -> ModeBasis {
        ModeBasis { basis, indices: indices.into(), custom: vec![], waist_um: 700., normalization }
    }

    /// A 700 um source waist folded by mirror M onto detector D 100 mm later,
    /// read in one centred ROI.
    fn fold(modes: ModeBasis) -> Bench {
        let mut bench = template("mach_zehnder").unwrap();
        bench.assertions.clear();
        bench.sources[0].field = Some(FieldSampling {
            grid: [128; 2],
            pitch_um: 40.,
            profile: FieldProfile::Gaussian,
            waist_um: 700.,
            polarization: Some([[1., 0.], [0., 0.]]),
        });
        let mut detector = mount_element("D", DETECTOR, [200., 300., 75.], [0., 1., 0.]);
        detector.readout = Some(Readout {
            ports: vec![ReadoutPort { id: "A".into(), roi: [0., 0., 4., 4.] }],
            modes,
            balanced_pairs: vec![],
        });
        bench.elements = vec![mount_element("M", MIRROR, [200., 200., 75.], [-1., 1., 0.]), detector];
        bench.connections = [("S1", "out", "M"), ("M", "reflected", "D")]
            .map(|(from, port, to)| Connection { from: from.into(), port: port.into(), to: to.into() })
            .into();
        bench.branches = vec![Branch { id: "beam".into(), from: Port::new("S1", "out"), to: Port::new("D", "in"), via: vec![], style: None }];
        bench
    }

    /// The source waist read head-on by detector D 1 mm downstream, where the
    /// residual curvature (R ≈ 6 km) is far below the tolerance.
    fn matched(modes: ModeBasis) -> Bench {
        let mut bench = fold(modes);
        let mut detector = mount_element("D", DETECTOR, add(bench.sources[0].position, [1., 0., 0.]), [-1., 0., 0.]);
        detector.readout = bench.elements.pop().unwrap().readout;
        bench.elements = vec![detector];
        bench.connections = vec![Connection { from: "S1".into(), port: "out".into(), to: "D".into() }];
        bench
    }

    /// Least-squares fit of `values` to c + a·cos φ + b·sin φ; returns
    /// (c, a, b, rms residual).
    fn fit(phis: &[f64], values: &[f64]) -> (f64, f64, f64, f64) {
        let rows: Vec<[f64; 3]> = phis.iter().map(|phi| [1., phi.cos(), phi.sin()]).collect();
        let mut m = [[0.; 4]; 3];
        for (row, &v) in rows.iter().zip(values) {
            for i in 0..3 {
                for j in 0..3 {
                    m[i][j] += row[i] * row[j];
                }
                m[i][3] += row[i] * v;
            }
        }
        for i in 0..3 {
            for k in 0..3 {
                if k != i {
                    let (f, pivot) = (m[k][i] / m[i][i], m[i]);
                    for (target, p) in m[k].iter_mut().zip(pivot) {
                        *target -= f * p;
                    }
                }
            }
        }
        let [c, a, b] = [0, 1, 2].map(|i| m[i][3] / m[i][i]);
        let residual = rows.iter().zip(values).map(|(r, v)| (v - c - a * r[1] - b * r[2]).powi(2)).sum::<f64>();
        (c, a, b, (residual / phis.len() as f64).sqrt())
    }

    #[test]
    fn matched_gaussian_has_no_power_in_any_other_mode() {
        let system = System::default();
        let hg: Vec<[i32; 2]> = (0..=3).flat_map(|n| (0..=3).map(move |m| [n, m])).collect();
        let reading = readout(&matched(basis(ModeFamily::HermiteGauss, &hg, ModeNormalization::Power)), &system, "D", None).unwrap();
        assert!(reading.warnings.is_empty(), "{:?}", reading.warnings);
        let port = &reading.ports[0];
        let modes = &port.modes;
        assert_eq!(modes.len(), 16);
        assert!((modes[0].fraction - 1.).abs() < 1e-6, "{modes:?}");
        assert!((modes[0].intensity - port.power).abs() < 1e-6 * port.power, "{modes:?} {}", port.power);
        assert!(modes[1..].iter().all(|m| m.fraction < 1e-6), "{modes:?}");
        let lg = [[0, 0], [0, 1], [0, -1], [0, 2], [0, -2], [1, 0], [1, 1], [2, 0]];
        let reading = readout(&matched(basis(ModeFamily::LaguerreGauss, &lg, ModeNormalization::Power)), &system, "D", None).unwrap();
        let modes = &reading.ports[0].modes;
        assert!((modes[0].fraction - 1.).abs() < 1e-6, "{modes:?}");
        assert!(modes[1..].iter().all(|m| m.fraction < 1e-6), "{modes:?}");
    }

    #[test]
    fn a_tilted_beam_reads_its_transverse_phase_on_the_detector_face() {
        let system = System::default();
        let tilted = |theta: f64| {
            let mut bench = matched(basis(ModeFamily::HermiteGauss, &[[0, 0]], ModeNormalization::Power));
            let readout = bench.elements[0].readout.take();
            let position = bench.elements[0].position;
            bench.elements[0] = mount_element("D", DETECTOR, position, [-theta.cos(), -theta.sin(), 0.]);
            bench.elements[0].readout = readout;
            bench
        };
        // Tilt θ about z: on the face the beam carries exp(i q y'), q = k sin θ,
        // and stretches by 1/cos θ; the HG0,0 overlap power is
        // 2c/(1+c²)·exp(−q²w²/(2(1+c²))) with c = cos θ.
        let theta: f64 = 2e-4;
        let bench = tilted(theta);
        let k = 2. * PI / (bench.sources[0].wavelength * 1e-6);
        let (c, q, w) = (theta.cos(), k * theta.sin(), 0.7);
        let expected = 2. * c / (1. + c * c) * (-(q * w).powi(2) / (2. * (1. + c * c))).exp();
        assert!(expected < 0.9 && expected > 0.1, "{expected}");
        let reading = readout(&bench, &system, "D", None).unwrap();
        let fraction = reading.ports[0].modes[0].fraction;
        assert!((fraction - expected).abs() < 1e-4 * expected, "{fraction} vs {expected}");
        // A carrier beyond the grid's Nyquist limit is an error, not a dropped phase.
        let error = readout(&tilted(0.02), &system, "D", None).unwrap_err();
        assert!(error.contains("pitch_um"), "{error}");
    }

    #[test]
    fn modes_finer_than_the_field_grid_are_rejected() {
        let system = System::default();
        let mut modes = basis(ModeFamily::HermiteGauss, &[[0, 0]], ModeNormalization::Power);
        modes.waist_um = 0.1;
        let error = readout(&matched(modes.clone()), &system, "D", None).unwrap_err();
        assert!(error.contains("waist_um") && error.contains("pitch_um"), "{error}");
        // Two samples per feature: 80 µm at order 0 on the 40 µm grid, but not at order 3.
        modes.waist_um = 80.;
        assert!(readout(&matched(modes.clone()), &system, "D", None).is_ok());
        modes.indices = vec![[3, 0]];
        assert!(readout(&matched(modes), &system, "D", None).is_err());
    }

    #[test]
    fn a_spot_straddling_two_rois_feeds_both() {
        let system = System::default();
        let mut bench = matched(basis(ModeFamily::HermiteGauss, &[[0, 0]], ModeNormalization::Power));
        let whole = readout(&bench, &system, "D", None).unwrap().ports[0].power;
        let config = bench.elements[0].readout.as_mut().unwrap();
        config.ports = vec![
            ReadoutPort { id: "L".into(), roi: [-1., 0., 2., 4.] },
            ReadoutPort { id: "R".into(), roi: [1., 0., 2., 4.] },
            ReadoutPort { id: "far".into(), roi: [0., 10., 1., 1.] },
        ];
        let reading = readout(&bench, &system, "D", None).unwrap();
        let [left, right, far] = &reading.ports[..] else { panic!() };
        assert_eq!(left.arms, ["beam"]);
        assert_eq!(right.arms, ["beam"]);
        assert!(far.arms.is_empty());
        assert!(reading.warnings.iter().any(|w| w.contains("far")), "{:?}", reading.warnings);
        // The sample column on the shared edge belongs to the right ROI only.
        assert!((left.power - right.power).abs() < 0.05 * whole, "{} {}", left.power, right.power);
        assert!((left.power + right.power - whole).abs() < 1e-9 * whole, "{} {whole}", left.power + right.power);
        // Each half reads the fundamental about its own centre, 1 mm off the beam.
        assert!(left.modes[0].fraction > 0.05 && left.modes[0].fraction < 0.5, "{:?}", left.modes);
    }

    #[test]
    fn the_detector_aperture_and_roi_size_bound_the_reading() {
        let system = System::default();
        let mut bench = matched(basis(ModeFamily::HermiteGauss, &[[0, 0]], ModeNormalization::Power));
        let whole = readout(&bench, &system, "D", None).unwrap().ports[0].power;
        // A 0.5 mm aperture radius inside the 4 mm ROI passes 1 − exp(−2a²/W²).
        bench.elements[0].aperture = 0.5;
        let reading = readout(&bench, &system, "D", None).unwrap();
        let expected = whole * (1. - (-2. * 0.25 / 0.49_f64).exp());
        let power = reading.ports[0].power;
        assert!((power - expected).abs() < 0.02 * expected, "{power} vs {expected}");
        assert!(reading.warnings.iter().any(|w| w.contains("aperture")), "{:?}", reading.warnings);
        // An ROI between source samples is an error, not an empty port.
        let mut tiny = matched(basis(ModeFamily::HermiteGauss, &[[0, 0]], ModeNormalization::Power));
        tiny.elements[0].readout.as_mut().unwrap().ports[0].roi = [0.02, 0.02, 0.01, 0.01];
        let error = readout(&tiny, &system, "D", None).unwrap_err();
        assert!(error.contains("narrower") && error.contains("pitch_um"), "{error}");
        // So is a large ROI clipped to an aperture smaller than the grid.
        tiny.elements[0].aperture = 0.01;
        tiny.elements[0].readout.as_mut().unwrap().ports[0].roi = [0.02, 0.02, 4., 4.];
        let error = readout(&tiny, &system, "D", None).unwrap_err();
        assert!(error.contains("narrower") && error.contains("aperture"), "{error}");
        // And a sliver at the aperture rim whose bounding box looks wide enough.
        (tiny.elements[0].aperture, tiny.elements[0].body_radius) = (10., 12.);
        tiny.elements[0].readout.as_mut().unwrap().ports[0].roi = [10., 3.5, 1., 1.];
        let error = readout(&tiny, &system, "D", None).unwrap_err();
        assert!(error.contains("narrower") && error.contains("pitch_um"), "{error}");
        // An oblique face stretches the 40 µm grid to 200 µm along y, so a
        // 0.12 mm ROI that a normal face would resolve is too narrow.
        tiny.elements[0].readout.as_mut().unwrap().ports[0].roi = [0.1, 0., 0.12, 0.12];
        assert!(readout(&tiny, &system, "D", None).is_ok());
        let mut oblique = tiny.clone();
        let (position, normal) = (oblique.elements[0].position, [-0.2, -0.96f64.sqrt(), 0.]);
        let mut remounted = mount_element("D", DETECTOR, position, normal);
        (remounted.aperture, remounted.body_radius, remounted.readout) = (10., 12., oblique.elements[0].readout.clone());
        oblique.elements[0] = remounted;
        let error = readout(&oblique, &system, "D", None).unwrap_err();
        assert!(error.contains("narrower") && error.contains("200.0 µm"), "{error}");
        // While the ROI covers the whole aperture, moving it leaves the power unchanged.
        let shifted = |roi: [f64; 4]| {
            let mut bench = matched(basis(ModeFamily::HermiteGauss, &[[0, 0]], ModeNormalization::Power));
            bench.elements[0].aperture = 0.5;
            bench.elements[0].readout.as_mut().unwrap().ports[0].roi = roi;
            readout(&bench, &system, "D", None).unwrap().ports[0].power
        };
        let (centred, moved) = (shifted([0., 0., 4., 4.]), shifted([0.02, 0.013, 4., 4.]));
        assert!((centred - moved).abs() < 0.03 * centred, "{centred} vs {moved}");
    }

    #[test]
    fn each_port_checks_the_modes_on_its_own_grid() {
        let system = System::default();
        let mut modes = basis(ModeFamily::HermiteGauss, &[[0, 0]], ModeNormalization::Power);
        modes.waist_um = 60.;
        let mut bench = matched(modes);
        // S1 on a 20 µm grid resolves a 60 µm waist; S2, 10 mm above on 40 µm, does not.
        bench.sources[0].field.as_mut().unwrap().pitch_um = 20.;
        let mut second = bench.sources[0].clone();
        second.id = "S2".into();
        second.position = add(second.position, [0., 0., 10.]);
        second.field.as_mut().unwrap().pitch_um = 40.;
        bench.sources.push(second);
        bench.connections.push(Connection { from: "S2".into(), port: "out".into(), to: "D".into() });
        bench.branches.push(Branch { id: "second".into(), from: Port::new("S2", "out"), to: Port::new("D", "in"), via: vec![], style: None });
        let detector = &mut bench.elements[0];
        (detector.aperture, detector.body_radius) = (20., 25.);
        let config = detector.readout.as_mut().unwrap();
        config.ports = [("A", 0.), ("up", 10.), ("down", -10.)]
            .map(|(id, z)| ReadoutPort { id: id.into(), roi: [0., z, 2., 2.] })
            .into();
        let error = readout(&bench, &system, "D", None).unwrap_err();
        assert!(error.contains("waist_um") && error.contains("40.0 µm"), "{error}");
        // Without the second branch the fine grid reads normally.
        bench.branches.pop();
        assert!(readout(&bench, &system, "D", None).is_ok());
    }

    #[test]
    fn repeated_custom_terms_add_before_normalizing() {
        let system = System::default();
        let term = ModeTerm { hg: [0, 0], coefficient: [1., 0.] };
        let mut custom = basis(ModeFamily::Custom, &[], ModeNormalization::Power);
        custom.custom = vec![CustomMode { label: "twice".into(), terms: vec![term.clone(), term.clone()] }];
        let reading = readout(&matched(custom.clone()), &system, "D", None).unwrap();
        let fraction = reading.ports[0].modes[0].fraction;
        assert!((fraction - 1.).abs() < 1e-6, "{fraction}");
        // Tiny coefficients normalize without underflow.
        let mut tiny = custom.clone();
        tiny.custom[0].terms = vec![ModeTerm { hg: [0, 0], coefficient: [1e-200, 0.] }];
        let reading = readout(&matched(tiny), &system, "D", None).unwrap();
        assert!((reading.ports[0].modes[0].fraction - 1.).abs() < 1e-6, "{:?}", reading.ports[0].modes);
        custom.custom[0].terms[1].coefficient = [-1., 0.];
        let error = matched(custom).validate().unwrap_err();
        assert!(error.contains("cancel"), "{error}");
    }

    #[test]
    fn pure_gaussian_reads_as_the_fundamental_mode() {
        let system = System::default();
        let hg = [[0, 0], [1, 0], [0, 1], [1, 1], [2, 0]];
        let reading = readout(&fold(basis(ModeFamily::HermiteGauss, &hg, ModeNormalization::Power)), &system, "D", None).unwrap();
        assert!(reading.warnings.is_empty(), "{:?}", reading.warnings);
        let port = &reading.ports[0];
        assert_eq!(port.arms, ["beam"]);
        assert!(port.modes[0].fraction > 0.99, "{:?}", port.modes);
        // Odd modes vanish by symmetry; HG2,0 only sees the small defocus.
        assert!(port.modes[1..4].iter().all(|m| m.fraction < 1e-6), "{:?}", port.modes);
        assert!(port.modes[4].fraction < 1e-2, "{:?}", port.modes);
        assert!((port.modes[0].intensity - port.modes[0].fraction * port.power).abs() < 1e-12);
        // Laguerre–Gauss and custom superpositions agree with the Hermite–Gauss reading.
        let lg = readout(
            &fold(basis(ModeFamily::LaguerreGauss, &[[0, 0], [0, 1], [0, -1], [1, 0]], ModeNormalization::Power)),
            &system,
            "D",
            None,
        )
        .unwrap();
        let modes = &lg.ports[0].modes;
        assert_eq!(modes[1].index, "LG0,1");
        assert!((modes[0].fraction - port.modes[0].fraction).abs() < 1e-9, "{modes:?}");
        assert!(modes[1..3].iter().all(|m| m.fraction < 1e-6), "{modes:?}");
        let mut custom = basis(ModeFamily::Custom, &[], ModeNormalization::Power);
        custom.custom = vec![CustomMode {
            label: "diagonal".into(),
            terms: vec![ModeTerm { hg: [0, 0], coefficient: [1., 0.] }, ModeTerm { hg: [1, 0], coefficient: [0., 1.] }],
        }];
        let diagonal = readout(&fold(custom), &system, "D", None).unwrap();
        assert_eq!(diagonal.ports[0].modes[0].index, "diagonal");
        // Exact up to the cross term with the ~1e-8 HG1,0 amplitude.
        assert!((diagonal.ports[0].modes[0].fraction - port.modes[0].fraction / 2.).abs() < 1e-6);
        // Amplitude normalization divides the unit-power overlap by the mode's peak
        // intensity, 2/(π w²) for HG0,0 in mm⁻².
        let amplitude =
            readout(&fold(basis(ModeFamily::HermiteGauss, &hg, ModeNormalization::Amplitude)), &system, "D", None).unwrap();
        let peak = 2. / (PI * 0.7 * 0.7);
        let expected = port.modes[0].intensity / peak;
        assert!((amplitude.ports[0].modes[0].intensity - expected).abs() < 1e-6 * expected, "{:?}", amplitude.ports[0].modes);
    }

    #[test]
    fn hybrid_balanced_pairs_follow_the_signal_phase_in_quadrature() {
        let system = System::default();
        let phis: Vec<f64> = (0..8).map(|k| 0.2 + 0.8 * k as f64).collect();
        let readings: Vec<_> = phis.iter().map(|&phi| readout(&hybrid(phi), &system, "CAM", None).unwrap()).collect();
        for reading in &readings {
            assert!(reading.warnings.is_empty(), "{:?}", reading.warnings);
            for port in &reading.ports {
                assert_eq!(port.arms, [format!("{} signal", port.id), format!("{} lo", port.id)]);
            }
            assert_eq!(reading.signals.len(), 1);
        }
        let real: Vec<f64> = readings.iter().map(|r| r.signals[0].real).collect();
        let imag: Vec<f64> = readings.iter().map(|r| r.signals[0].imag).collect();
        let (rc, ra, rb, r_res) = fit(&phis, &real);
        let (ic, ia, ib, i_res) = fit(&phis, &imag);
        let (r_amp, i_amp) = (ra.hypot(rb), ia.hypot(ib));
        assert!(r_amp > 0.1 && i_amp > 0.1, "{r_amp} {i_amp}");
        // Proportional: no offset, and no residual beyond the sinusoid.
        assert!(rc.abs() < 1e-6 * r_amp && ic.abs() < 1e-6 * i_amp, "{rc} {ic}");
        assert!(r_res < 1e-6 * r_amp && i_res < 1e-6 * i_amp, "{r_res} {i_res}");
        assert!((r_amp - i_amp).abs() < 1e-6 * r_amp, "{r_amp} {i_amp}");
        // real ∝ cos(φ + θ) and imag ∝ sin(φ + θ) with the same θ.
        let theta = (-rb).atan2(ra);
        let wrap = |a: f64| (a + PI).rem_euclid(2. * PI) - PI;
        assert!(wrap(theta - ia.atan2(ib)).abs() < 1e-6, "{theta} {}", ia.atan2(ib));
        // The HG0,0 signal tracks the same phase; its offset comes from the ports'
        // slightly different overlaps at their oblique camera incidence.
        let mode: Vec<f64> = readings.iter().map(|r| r.signals[0].modes[0].real).collect();
        let (_, ma, mb, m_res) = fit(&phis, &mode);
        assert!(m_res < 1e-6 * ma.hypot(mb) && wrap((-mb).atan2(ma) - theta).abs() < 1e-6, "{m_res} {ma} {mb}");
    }

    #[test]
    fn readout_validation_rejects_bad_configurations() {
        let mut bench = fold(basis(ModeFamily::HermiteGauss, &[[0, 0]], ModeNormalization::Power));
        assert!(bench.validate().is_ok());
        let config = bench.elements[1].readout.clone().unwrap();
        let reject = |bench: &Bench, needle: &str| {
            let error = bench.validate().unwrap_err();
            assert!(error.contains(needle), "{error}");
        };
        bench.elements[0].readout = Some(config.clone());
        reject(&bench, "Invalid readout on M");
        bench.elements[0].readout = None;
        let mut edit = |change: &dyn Fn(&mut Readout), needle: &str| {
            let mut broken = config.clone();
            change(&mut broken);
            bench.elements[1].readout = Some(broken);
            reject(&bench, needle);
        };
        edit(&|r| r.ports.push(r.ports[0].clone()), "Invalid readout on D");
        edit(&|r| r.ports[0].roi[2] = 0., "Invalid readout on D");
        edit(&|r| r.modes.indices = vec![[0, 0], [25, 0]], "Invalid readout on D");
        edit(&|r| r.modes.waist_um = 0., "Invalid readout on D");
        edit(
            &|r| {
                r.balanced_pairs = vec![BalancedPair {
                    signal: "x".into(),
                    plus: "A".into(),
                    minus: "B".into(),
                    quadrature: Quadrature::Real,
                }]
            },
            "Invalid readout on D",
        );
        edit(
            &|r| {
                r.modes.basis = ModeFamily::Custom;
                r.modes.custom = vec![CustomMode { label: "z".into(), terms: vec![ModeTerm { hg: [0, 0], coefficient: [0., 0.] }] }];
            },
            "Invalid readout on D",
        );
        edit(
            &|r| {
                r.modes.basis = ModeFamily::LaguerreGauss;
                r.modes.indices = vec![[0, i32::MIN]];
            },
            "Invalid readout on D",
        );
        let system = System::default();
        assert!(readout(&template("mach_zehnder").unwrap(), &system, "D1", None).unwrap_err().contains("readout"));
    }
}
