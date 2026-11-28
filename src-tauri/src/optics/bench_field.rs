//! Transverse complex fields along solved bench branches.
//!
//! Each arm launches its source's sampled field (`Source.field`) and follows the
//! arm's traced axial segments from the source. The field is a scalar complex
//! envelope A(u, v) on a fixed grid in the arm's transverse frame, times the
//! traced unit Jones vector and the square root of the traced power ratio, so
//! polarization elements, splitters, coatings and ND filters act through the
//! trace. Reflections mirror the frame together with the field, so the array is
//! carried unchanged from flight to flight.
//!
//! Flights in air or glass use the band-limited angular spectrum method
//! (Matsushima & Shimobaba 2009) with the carrier removed:
//! H = exp(i·2πL(√(1/λ² − fx² − fy²) − 1/λ)), λ = λ0/n, keeping |f| below
//! 1/(λ·√((2Δf·L)² + 1)) on each axis and dropping evanescent components. The
//! flight piston 2π·OPL/λ0 and phase-plate phases are added separately. The
//! method holds at any distance while the beam stays inside the window
//! (boundaries are periodic, so escaping light wraps) and the grid resolves the
//! angular content (|θ| < λ/(2·pitch)); `sampling` reports both conditions.
//!
//! At each element the field meets the clear aperture (or the departing region
//! outline), the paraxial quadratic phase of a thin lens or curved mirror about
//! the chief-ray hit (the chief-ray deflection is already in the traced frame),
//! and the element's spatial action. Each sample reaches element-local plane
//! coordinates by projection along the incoming ray.
//!
//! Traced reflection fractions are real, so the field adds the phases of a
//! lossless symmetric splitter (π/2 on reflection) and a perfect-conductor
//! mirror (π). Arms are superposed as world-frame polarization vectors after
//! resampling onto the first arm's plane, weighted by √cos of their incidence
//! so that intensity is the flux normal to that plane.
//!
//! Oblique refraction stretches the beam across the plane of incidence by
//! cos θt / cos θi. The field keeps its sampling grid and carries the linear map
//! G from grid to physical transverse coordinates. Element actions read physical
//! coordinates, and propagation uses physical frequencies G⁻ᵀ·f.

use super::super::{
    System,
    bench::{
        self, Action, Bench, BranchPath, Element, FieldProfile, Kind, Port, Source, Trace, cross,
        local, rotate, slab_faces,
    },
    math::{V3, add, dot, norm, scale, sub, unit},
};
use super::relay::{Frame, frames_along};
use serde::Serialize;
use std::{
    collections::HashMap,
    f64::consts::{PI, TAU},
    ops::{Add, AddAssign, Mul, Sub},
};

#[path = "bench_readout.rs"]
pub(crate) mod readout;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct C {
    re: f64,
    im: f64,
}

impl C {
    const ZERO: Self = Self { re: 0., im: 0. };
    fn new(re: f64, im: f64) -> Self {
        Self { re, im }
    }
    fn cis(phase: f64) -> Self {
        let (s, c) = phase.sin_cos();
        Self { re: c, im: s }
    }
    fn conj(self) -> Self {
        Self {
            re: self.re,
            im: -self.im,
        }
    }
    fn norm2(self) -> f64 {
        self.re * self.re + self.im * self.im
    }
    fn scale(self, s: f64) -> Self {
        Self {
            re: self.re * s,
            im: self.im * s,
        }
    }
    fn arg(self) -> f64 {
        self.im.atan2(self.re)
    }
}

impl Add for C {
    type Output = Self;
    fn add(self, o: Self) -> Self {
        Self::new(self.re + o.re, self.im + o.im)
    }
}

impl Sub for C {
    type Output = Self;
    fn sub(self, o: Self) -> Self {
        Self::new(self.re - o.re, self.im - o.im)
    }
}

impl Mul for C {
    type Output = Self;
    fn mul(self, o: Self) -> Self {
        Self::new(
            self.re * o.re - self.im * o.im,
            self.re * o.im + self.im * o.re,
        )
    }
}

impl AddAssign for C {
    fn add_assign(&mut self, o: Self) {
        *self = *self + o;
    }
}

/// In-place radix-2 FFT; the inverse is normalised by 1/n.
fn fft(a: &mut [C], inverse: bool) {
    let n = a.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            a.swap(i, j);
        }
    }
    let sign = if inverse { 1. } else { -1. };
    let mut len = 2;
    while len <= n {
        let half = len / 2;
        let twiddles: Vec<C> = (0..half)
            .map(|k| C::cis(sign * TAU * k as f64 / len as f64))
            .collect();
        for start in (0..n).step_by(len) {
            for (k, w) in twiddles.iter().enumerate() {
                let u = a[start + k];
                let v = a[start + k + half] * *w;
                a[start + k] = u + v;
                a[start + k + half] = u - v;
            }
        }
        len <<= 1;
    }
    if inverse {
        let s = 1. / n as f64;
        a.iter_mut().for_each(|x| *x = x.scale(s));
    }
}

/// 2-D FFT of a row-major grid of `n[0]` columns and `n[1]` rows.
fn fft2(data: &mut [C], n: [usize; 2], inverse: bool) {
    for row in data.chunks_mut(n[0]) {
        fft(row, inverse);
    }
    let mut column = vec![C::ZERO; n[1]];
    for x in 0..n[0] {
        for (y, c) in column.iter_mut().enumerate() {
            *c = data[y * n[0] + x];
        }
        fft(&mut column, inverse);
        for (y, c) in column.iter().enumerate() {
            data[y * n[0] + x] = *c;
        }
    }
}

/// Signed frequency index of FFT bin `k` out of `n`.
fn signed(k: usize, n: usize) -> f64 {
    if k < n / 2 {
        k as f64
    } else {
        k as f64 - n as f64
    }
}

/// SplitMix64 generator: deterministic, seedable phase screens.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// Uniform in the open interval (0, 1).
    fn uniform(&mut self) -> f64 {
        ((self.next() >> 11) as f64 + 0.5) / (1u64 << 53) as f64
    }
    fn gaussian(&mut self) -> f64 {
        let (a, b) = (self.uniform(), self.uniform());
        (-2. * a.ln()).sqrt() * (TAU * b).cos()
    }
}

fn fnv(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
    })
}

/// Sampling grid: `n` [columns, rows] centred on the chief ray, `pitch` in mm.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Grid {
    n: [usize; 2],
    pitch: f64,
}

impl Grid {
    fn len(&self) -> usize {
        self.n[0] * self.n[1]
    }
    fn coords(&self, i: usize) -> [f64; 2] {
        [
            ((i % self.n[0]) as f64 - (self.n[0] / 2) as f64) * self.pitch,
            ((i / self.n[0]) as f64 - (self.n[1] / 2) as f64) * self.pitch,
        ]
    }
    /// Spatial frequencies (1/mm) of FFT bin `i`.
    fn frequency(&self, i: usize) -> [f64; 2] {
        [
            signed(i % self.n[0], self.n[0]) / (self.n[0] as f64 * self.pitch),
            signed(i / self.n[0], self.n[1]) / (self.n[1] as f64 * self.pitch),
        ]
    }
    /// Bilinear sample at `[x, y]` (mm); zero outside the grid, or periodic.
    fn sample<T: Copy + Default + Add<Output = T> + Mul<f64, Output = T>>(
        &self,
        data: &[T],
        [x, y]: [f64; 2],
        periodic: bool,
    ) -> T {
        let fx = x / self.pitch + (self.n[0] / 2) as f64;
        let fy = y / self.pitch + (self.n[1] / 2) as f64;
        let (x0, y0) = (fx.floor(), fy.floor());
        let (tx, ty) = (fx - x0, fy - y0);
        let at = |ix: f64, iy: f64| -> Option<T> {
            let (ix, iy) = (ix as i64, iy as i64);
            let [nx, ny] = self.n.map(|n| n as i64);
            let (ix, iy) = if periodic {
                (ix.rem_euclid(nx), iy.rem_euclid(ny))
            } else if (0..nx).contains(&ix) && (0..ny).contains(&iy) {
                (ix, iy)
            } else {
                return None;
            };
            Some(data[(iy * nx + ix) as usize])
        };
        let mut sum = T::default();
        for (ix, iy, w) in [
            (x0, y0, (1. - tx) * (1. - ty)),
            (x0 + 1., y0, tx * (1. - ty)),
            (x0, y0 + 1., (1. - tx) * ty),
            (x0 + 1., y0 + 1., tx * ty),
        ] {
            if w > 0.
                && let Some(v) = at(ix, iy)
            {
                sum = sum + v * w;
            }
        }
        sum
    }
}

impl Mul<f64> for C {
    type Output = Self;
    fn mul(self, s: f64) -> Self {
        self.scale(s)
    }
}

/// One rotating-ground-glass realization: a periodic Gaussian-correlated phase
/// screen in element-local plane coordinates.
struct Screen {
    grid: Grid,
    phase: Vec<f64>,
}

impl Screen {
    fn new(grid: Grid, correlation_mm: f64, rms_rad: f64, seed: u64) -> Self {
        let mut rng = Rng(seed);
        let mut data: Vec<C> = (0..grid.len())
            .map(|_| C::new(rng.gaussian(), 0.))
            .collect();
        fft2(&mut data, grid.n, false);
        // |G|² = exp(−π²c²f²) gives the correlation exp(−r²/c²).
        for (i, v) in data.iter_mut().enumerate() {
            let [fx, fy] = grid.frequency(i);
            *v = v.scale((-(PI * correlation_mm).powi(2) * (fx * fx + fy * fy) / 2.).exp());
        }
        fft2(&mut data, grid.n, true);
        let mean = data.iter().map(|c| c.re).sum::<f64>() / data.len() as f64;
        let rms =
            (data.iter().map(|c| (c.re - mean).powi(2)).sum::<f64>() / data.len() as f64).sqrt();
        let gain = if rms > 0. { rms_rad / rms } else { 0. };
        Self {
            grid,
            phase: data.iter().map(|c| (c.re - mean) * gain).collect(),
        }
    }
    fn at(&self, point: [f64; 2]) -> f64 {
        self.grid.sample(&self.phase, point, true)
    }
}

/// Element interaction on an arm, applied before the flight at `step`.
#[derive(Clone)]
struct Event<'a> {
    step: usize,
    element: &'a Element,
    /// Output port taken (the last port when leaving through a substrate).
    port: String,
    hit: V3,
    incoming: Frame,
    /// Index of the medium before the element.
    index: f64,
    /// Normal of the surface the field is sampled on, when it is not the
    /// element plane (a lens's wedged glass exit face).
    surface: Option<V3>,
    /// Only the clear aperture acts (a lens's glass entry face; its power is
    /// applied at the exit).
    clip_only: bool,
}

/// The index (0 front, 1 back) and slab face of an element's substrate nearest
/// `at`: the face the light crossed there.
fn slab_face(e: &Element, at: V3) -> Option<(usize, bench::SlabFace)> {
    let faces = slab_faces(e, e.substrate.as_ref()?);
    faces.into_iter().enumerate().min_by(|(_, x), (_, y)| {
        dot(sub(at, x.point), x.normal)
            .abs()
            .total_cmp(&dot(sub(at, y.point), y.normal).abs())
    })
}

fn lens_face(e: &Element, at: V3) -> Option<bench::SlabFace> {
    slab_face(e, at).map(|(_, face)| face)
}

#[derive(Clone)]
struct Arm<'a> {
    label: String,
    source: &'a Source,
    grid: Grid,
    chain: Vec<usize>,
    events: Vec<Event<'a>>,
    wavelength_nm: f64,
    power: f64,
    piston: f64,
    opl_mm: f64,
    /// End polarization in world coordinates.
    vector: [C; 3],
    /// (step, tangential axis, stretch) of each oblique refraction.
    refractions: Vec<(usize, [f64; 2], f64)>,
    end: Frame,
    end_point: V3,
    end_index: f64,
    /// Far-field deflection (mrad) predicted from tip-tilt actions in the end
    /// basis; None when a lens or curved mirror follows a tip-tilt.
    geometric_tilt: Option<[f64; 2]>,
}

/// bench_field target selection.
pub(crate) struct FieldRequest {
    pub branch: Option<String>,
    pub port: Option<Port>,
    pub wavelength: Option<f64>,
    pub realizations: Option<usize>,
    pub arrays: bool,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ArmReading {
    pub branch: String,
    pub source: String,
    pub power: f64,
    /// Near-field centroid [u, v] (mm) in the target basis.
    pub centroid_mm: [f64; 2],
    /// Phase of the arm's field projected on the reference arm's polarization.
    pub phase_rad: f64,
    pub opl_mm: f64,
    /// Deflection predicted by the tip-tilt actions [u, v] (mrad); null when a
    /// lens or curved mirror follows a tip-tilt.
    pub geometric_tilt_mrad: Option<[f64; 2]>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct PairReading {
    pub arms: [usize; 2],
    /// Source coherence: 1 for one source or a shared coherence group (times the
    /// coherence-length factor), 0 otherwise.
    pub coherence: f64,
    /// Normalised field cross-correlation |Σ⟨E_a, E_b⟩| / √(ΣI_a·ΣI_b) over the
    /// grid and all realizations.
    pub correlation: f64,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct Sampling {
    pub window_mm: [f64; 2],
    /// Largest angle the grid resolves, λ/(2·pitch) (mrad).
    pub max_angle_mrad: f64,
    /// Smallest band-limit angle over the flights (mrad).
    pub band_limit_mrad: f64,
    /// Power fraction in the outer sixteenth of the window on each side.
    pub edge_fraction: f64,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct FieldReading {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<Port>,
    pub wavelength_nm: f64,
    pub realizations: usize,
    pub grid: [usize; 2],
    pub pitch_um: f64,
    /// Chief-ray point, direction and transverse basis [u, v] of the target plane.
    pub center: V3,
    pub direction: V3,
    pub basis: [V3; 2],
    /// Realization-averaged power, in source power units.
    pub power: f64,
    pub centroid_mm: [f64; 2],
    /// Far-field intensity centroid [u, v] (mrad).
    pub far_field_centroid_mrad: [f64; 2],
    /// Phase of the summed field coherent with the first arm, projected on its polarization.
    pub mean_phase_rad: f64,
    /// σ/mean of the averaged intensity over lit samples; the speckle contrast
    /// under uniform illumination.
    pub contrast: f64,
    pub arms: Vec<ArmReading>,
    pub pairs: Vec<PairReading>,
    pub sampling: Sampling,
    /// Block-averaged intensity rows (v ascending) when `arrays` is set, at most 64 × 64.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intensity: Option<Vec<Vec<f64>>>,
    /// Phase of the block-summed coherent field, same layout.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase_rad: Option<Vec<Vec<f64>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub array_pitch_um: Option<f64>,
}

fn direction(trace: &Trace, index: usize) -> Option<V3> {
    let s = trace.segments.get(index)?;
    unit(sub(s.to, s.from)).ok()
}

fn element<'a>(bench: &'a Bench, id: &str) -> Option<&'a Element> {
    bench.elements.iter().find(|e| e.id == id)
}

/// Reflection phase of a lossless symmetric splitter (r = i·√R against a real
/// t) and of a perfect-conductor mirror (r = −√R). The traced fractions are real.
fn reflection_phase(e: &Element, port: &str) -> f64 {
    match e.kind {
        Kind::Mirror | Kind::CurvedMirror => PI,
        Kind::Splitter | Kind::Pbs if port == "reflected" => PI / 2.,
        _ => 0.,
    }
}

/// World-frame complex polarization: the traced Jones components lie along the
/// branch frame basis [u, v], which reflections mirror together with the field
/// (an ideal reflection maps E to −R·E, so the components are unchanged and the
/// sign is the mirror phase above).
fn polarization([[hr, hi], [vr, vi]]: [[f64; 2]; 2], [u, v]: [V3; 2]) -> [C; 3] {
    std::array::from_fn(|k| C::new(hr, hi).scale(u[k]) + C::new(vr, vi).scale(v[k]))
}

/// ⟨a, b⟩ = Σ a_k·conj(b_k).
fn inner(a: [C; 3], b: [C; 3]) -> C {
    (0..3).fold(C::ZERO, |s, k| s + a[k] * b[k].conj())
}

/// Normal-flux coupling of two arms crossing a plane with unit normal `n`,
/// relative to their √cos-weighted amplitudes. With H ∝ d × E, the cross term
/// of ½Re(E × H*)·n is Re(A_a·conj(A_b)·(K_ab + conj(K_ba))) with
/// K_ab = (n·d_b)(E_a·E_b*) − (n·E_b*)(E_a·d_b); a single arm gives cos θ·|A|².
fn flux((ea, da): ([C; 3], V3), (eb, db): ([C; 3], V3), n: V3) -> C {
    let dot_real = |e: [C; 3], d: V3| (0..3).fold(C::ZERO, |s, k| s + e[k].scale(d[k]));
    let half = |ea: [C; 3], eb: [C; 3], db: V3| {
        inner(ea, eb).scale(dot(n, db)) - dot_real(eb.map(C::conj), n) * dot_real(ea, db)
    };
    let cos = (dot(n, da) * dot(n, db)).sqrt();
    (half(ea, eb, db) + half(eb, ea, da).conj()).scale(0.5 / cos)
}

impl Arm<'_> {
    fn wave(&self) -> ([C; 3], V3) {
        (self.vector, self.end.direction)
    }
}

/// Phase advance per reference-grid sample of the carrier exp(ik·along) that
/// places `arm` on the reference plane.
fn carrier_step(arm: &Arm, reference: &Arm) -> f64 {
    let k = TAU * arm.end_index / (arm.wavelength_nm * 1e-6);
    reference
        .end
        .basis
        .map(|b| k * dot(b, arm.end.direction).abs() * reference.grid.pitch)
        .into_iter()
        .fold(0., f64::max)
}

/// Row-major 2×2 map from grid to physical transverse coordinates.
type M2 = [[f64; 2]; 2];
const IDENTITY: M2 = [[1., 0.], [0., 1.]];

fn apply(m: M2, [x, y]: [f64; 2]) -> [f64; 2] {
    [m[0][0] * x + m[0][1] * y, m[1][0] * x + m[1][1] * y]
}

fn product(a: M2, b: M2) -> M2 {
    std::array::from_fn(|i| std::array::from_fn(|j| a[i][0] * b[0][j] + a[i][1] * b[1][j]))
}

fn inverse(m: M2) -> M2 {
    let det = m[0][0] * m[1][1] - m[0][1] * m[1][0];
    [
        [m[1][1] / det, -m[0][1] / det],
        [-m[1][0] / det, m[0][0] / det],
    ]
}

fn transpose(m: M2) -> M2 {
    [[m[0][0], m[1][0]], [m[0][1], m[1][1]]]
}

/// Anamorphic step of a refraction from index `n1` along `d1` to `n2` along
/// `d2`: the tangential axis in the outgoing basis and the stretch
/// cos θt / cos θi. Snell's law in vector form gives the normal ∥ n2·d2 − n1·d1.
/// The outgoing basis is the incoming one carried by the rotation taking `d1`
/// to the final direction `after` (`d2`, or the direction after a thin element
/// that turns the ray as it leaves, as a lens leaving its glass does), so the
/// tangential axis is taken across `d1` and carried by that same rotation: the
/// stretch then lands on the grid axis the basis transport maps it to.
fn refraction(
    n1: f64,
    d1: V3,
    n2: f64,
    d2: V3,
    after: V3,
    [u, v]: [V3; 2],
) -> Option<([f64; 2], f64)> {
    if dot(d1, d2) > 1. - 1e-12 {
        return None;
    }
    let normal = unit(sub(scale(d2, n2), scale(d1, n1))).ok()?;
    let (cos_i, cos_t) = (dot(d1, normal).abs(), dot(d2, normal).abs());
    let t = unit(sub(normal, scale(d1, dot(normal, d1)))).ok()?;
    let axis = cross(d1, after);
    let t = match unit(axis) {
        Ok(k) => {
            let (sin, cos) = (norm(axis), dot(d1, after));
            add(
                add(scale(t, cos), scale(cross(k, t), sin)),
                scale(k, dot(k, t) * (1. - cos)),
            )
        }
        Err(_) => t,
    };
    (cos_i > 1e-9).then(|| ([dot(t, u), dot(t, v)], cos_t / cos_i))
}

fn arm<'a>(
    bench: &'a Bench,
    trace: &Trace,
    label: String,
    path: &BranchPath,
) -> Result<Arm<'a>, String> {
    let end = path
        .end_segment
        .ok_or_else(|| format!("Branch {label} has no end segment"))?;
    let mut chain = vec![end];
    while let Some(previous) = trace.segments[*chain.last().unwrap()]
        .flight
        .as_ref()
        .and_then(|f| f.previous_segment)
    {
        chain.push(previous);
    }
    chain.reverse();
    let flight = |i: usize| {
        trace.segments[chain[i]]
            .flight
            .as_ref()
            .expect("branch flights")
    };
    for i in 0..chain.len() {
        let f = flight(i);
        for id in [&f.from_element, &f.to_element] {
            if element(bench, id).is_some_and(|e| e.kind == Kind::LensGroup) {
                return Err(format!(
                    "Branch {label} crosses lens group {id}; bench_field propagates thin elements only"
                ));
            }
        }
    }
    let source = bench
        .sources
        .iter()
        .find(|s| s.id == path.source)
        .ok_or_else(|| format!("Unknown source {}", path.source))?;
    let sampling = source.field.as_ref().ok_or_else(|| {
        format!(
            "Source {} of branch {label} has no field sampling",
            source.id
        )
    })?;
    let grid = Grid {
        n: sampling.grid,
        pitch: sampling.pitch_um * 1e-3,
    };
    if sampling.profile == FieldProfile::Flat && sampling.waist_um < sampling.pitch_um {
        return Err(format!(
            "Source {} flat field radius is below its sampling pitch",
            source.id
        ));
    }
    let end_jones = trace.segments[end].jones.ok_or_else(|| {
        format!(
            "Branch {label} is unpolarized; bench_field needs a polarized source (field.polarization or polarization)"
        )
    })?;
    let first = direction(trace, chain[0]).ok_or("Zero-length launch flight")?;
    let frames = frames_along(bench, trace, first, &chain).ok_or("Degenerate branch frame")?;
    let launched = trace.segments[chain[0]].power;
    if launched <= 0. {
        return Err(format!("Branch {label} carries no traced power"));
    }
    let mut piston = 0.;
    let mut opl_mm = 0.;
    for i in 0..chain.len() {
        let f = flight(i);
        piston += TAU * f.opl_mm / (f.wavelength_nm * 1e-6) + f.phase_rad;
        opl_mm += f.opl_mm + f.plate_opl_mm();
        if let Some(e) = element(bench, &f.from_element) {
            piston += reflection_phase(e, &f.from_port);
        }
    }
    let mut refractions = Vec::new();
    for step in 1..chain.len() {
        let (f, before) = (flight(step), flight(step - 1));
        let (a, b) = (frames[step - 1], frames[step]);
        // Leaving a lens's glass, the exit face alone refracts; the lens power
        // that also turns the traced ray is the lens event's.
        let exit = element(bench, &f.from_element)
            .filter(|e| e.kind == Kind::Lens && f.from_element == before.from_element)
            .and_then(|e| lens_face(e, trace.segments[chain[step - 1]].to))
            .and_then(|face| bench::refract(a.direction, face.normal, before.index / f.index));
        if a.reflections == b.reflections
            && (f.index - before.index).abs() > 1e-12
            && let Some((axis, stretch)) = refraction(
                before.index,
                a.direction,
                f.index,
                exit.unwrap_or(b.direction),
                b.direction,
                b.basis,
            )
        {
            refractions.push((step, axis, stretch));
        }
    }
    let mut events = Vec::new();
    let mut geometric_tilt = Some([0.; 2]);
    let mut tilted = false;
    for step in 1..chain.len() {
        let (f, before) = (flight(step), flight(step - 1));
        if f.from_element == before.from_element {
            continue;
        }
        let Some(e) = element(bench, &f.from_element) else {
            continue;
        };
        let mut last = step;
        while last + 1 < chain.len() && flight(last + 1).from_element == f.from_element {
            last += 1;
        }
        let outgoing = frames[last];
        if matches!(e.kind, Kind::Lens | Kind::CurvedMirror) && tilted {
            geometric_tilt = None;
        }
        if let Some(Action::TipTilt { tilt_mrad }) = &e.action {
            tilted = true;
            let r = e.rotation();
            let n = rotate([1., 0., 0.], r);
            let t = add(
                scale(rotate([0., 1., 0.], r), tilt_mrad[0]),
                scale(rotate([0., 0., 1.], r), tilt_mrad[1]),
            );
            // Keep the in-plane phase gradient and stay transverse to the exit ray.
            let d = outgoing.direction;
            let delta = sub(t, scale(n, dot(t, d) / dot(n, d)));
            if let Some(g) = &mut geometric_tilt {
                g[0] += dot(delta, outgoing.basis[0]);
                g[1] += dot(delta, outgoing.basis[1]);
            }
        }
        // A lens with a substrate applies its power where the light leaves the
        // glass, in the exit medium's wavenumber, as the trace does.
        let event = if e.kind == Kind::Lens && last > step {
            // The entry face clips to the clear aperture before the glass, as
            // the trace checks the entry and exit hits.
            let hit = trace.segments[chain[step - 1]].to;
            events.push(Event {
                step,
                element: e,
                port: flight(last).from_port.clone(),
                hit,
                incoming: frames[step - 1],
                index: before.index,
                surface: lens_face(e, hit).map(|face| face.normal),
                clip_only: true,
            });
            Event {
                step: last,
                element: e,
                port: flight(last).from_port.clone(),
                hit: trace.segments[chain[last - 1]].to,
                incoming: frames[last - 1],
                index: flight(last).index,
                // Sampled on the exit face, which a wedge tilts off the element plane.
                surface: lens_face(e, trace.segments[chain[last - 1]].to).map(|face| face.normal),
                clip_only: false,
            }
        } else if !e.regions.is_empty()
            && last > step
            && slab_face(e, trace.segments[chain[step - 1]].to).is_some_and(|(i, _)| i == 1)
        {
            // Regions sit on the element plane, the substrate's front face: light
            // entering through the back face meets them where it leaves the glass.
            Event {
                step: last,
                element: e,
                port: flight(last).from_port.clone(),
                hit: trace.segments[chain[last - 1]].to,
                incoming: frames[last - 1],
                index: flight(last).index,
                surface: None,
                clip_only: false,
            }
        } else {
            Event {
                step,
                element: e,
                port: flight(last).from_port.clone(),
                hit: trace.segments[chain[step - 1]].to,
                incoming: frames[step - 1],
                index: before.index,
                surface: None,
                clip_only: false,
            }
        };
        events.push(event);
    }
    let last = flight(chain.len() - 1);
    Ok(Arm {
        label,
        source,
        grid,
        wavelength_nm: last.wavelength_nm,
        power: source.power * trace.segments[end].power / launched,
        piston,
        opl_mm,
        vector: polarization(end_jones, frames.last().ok_or("Empty branch")?.basis),
        end: *frames.last().ok_or("Empty branch")?,
        end_point: trace.segments[end].to,
        end_index: last.index,
        geometric_tilt,
        refractions,
        events,
        chain,
    })
}

fn launch(source: &Source, grid: Grid) -> Vec<C> {
    let sampling = source.field.as_ref().expect("checked field");
    let w = sampling.waist_um * 1e-3;
    let mut a: Vec<C> = (0..grid.len())
        .map(|i| {
            let [x, y] = grid.coords(i);
            let r2 = x * x + y * y;
            let amplitude = match sampling.profile {
                FieldProfile::Gaussian => (-r2 / (w * w)).exp(),
                FieldProfile::Flat => f64::from(u8::from(r2 <= w * w)),
            };
            C::new(amplitude, 0.)
        })
        .collect();
    let power = a.iter().map(|c| c.norm2()).sum::<f64>() * grid.pitch * grid.pitch;
    let gain = 1. / power.sqrt();
    a.iter_mut().for_each(|c| *c = c.scale(gain));
    a
}

/// Band-limited angular-spectrum propagation over `length` mm at the medium
/// wavelength `wavelength` mm. Returns sin of the band-limit angle and the
/// power fraction dropped by the band limit and evanescent cut.
fn propagate(a: &mut [C], grid: Grid, g: M2, length: f64, wavelength: f64) -> (f64, f64) {
    if length <= 0. {
        return (1., 0.);
    }
    fft2(a, grid.n, false);
    // Physical frequency G⁻ᵀ·f; the transfer phase −πλL·fᵀ(G⁻¹G⁻ᵀ)f has its
    // grid-frequency gradient bounded by the largest absolute row sum of G⁻¹G⁻ᵀ.
    let to_physical = transpose(inverse(g));
    let metric = product(inverse(g), to_physical);
    let gain = metric
        .map(|row| row[0].abs() + row[1].abs())
        .into_iter()
        .fold(0., f64::max);
    let limit = grid.n.map(|n| {
        let df = 1. / (n as f64 * grid.pitch);
        1. / (wavelength * ((2. * df * length * gain).powi(2) + 1.).sqrt())
    });
    let k = 1. / wavelength;
    let (mut kept, mut dropped) = (0., 0.);
    for (i, v) in a.iter_mut().enumerate() {
        let [fx, fy] = grid.frequency(i);
        let [px, py] = apply(to_physical, [fx, fy]);
        let f2 = px * px + py * py;
        let kz2 = k * k - f2;
        if kz2 <= 0. || fx.abs() > limit[0] || fy.abs() > limit[1] {
            dropped += v.norm2();
            *v = C::ZERO;
            continue;
        }
        kept += v.norm2();
        // √(k² − f²) − k without cancellation.
        *v = *v * C::cis(-TAU * length * f2 / (kz2.sqrt() + k));
    }
    fft2(a, grid.n, true);
    let total = kept + dropped;
    (
        (limit[0].min(limit[1]) * wavelength).min(1.),
        if total > 0. { dropped / total } else { 0. },
    )
}

type Screens = HashMap<(String, usize), Screen>;

/// Smallest band limit (sine) and largest power fraction one flight dropped.
struct Band {
    sin: f64,
    dropped: f64,
}

fn screen<'s>(
    screens: &'s mut Screens,
    grid: Grid,
    e: &Element,
    realization: usize,
) -> Option<&'s Screen> {
    let Some(Action::Rgg {
        correlation_um,
        roughness_waves,
        seed,
    }) = &e.action
    else {
        return None;
    };
    Some(
        screens
            .entry((e.id.clone(), realization))
            .or_insert_with(|| {
                let seed = fnv(&e.id) ^ (u64::from(*seed) << 32) ^ realization as u64;
                Screen::new(
                    grid,
                    correlation_um * 1e-3,
                    TAU * roughness_waves,
                    Rng(seed).next(),
                )
            }),
    )
}

fn interact(
    a: &mut [C],
    grid: Grid,
    g: M2,
    event: &Event,
    k: f64,
    screen: Option<&Screen>,
) -> Result<(), String> {
    let e = event.element;
    let r = e.rotation();
    let normal = event.surface.unwrap_or_else(|| rotate([1., 0., 0.], r));
    let d = event.incoming.direction;
    let [bu, bv] = event.incoming.basis;
    let cos = dot(d, normal);
    if cos.abs() < 1e-9 {
        return Err(format!("Field meets {} at grazing incidence", e.id));
    }
    let hit = local(sub(event.hit, e.position), r);
    let region = (!e.regions.is_empty())
        .then(|| e.regions.iter().find(|x| x.port == event.port))
        .flatten();
    let focal = matches!(e.kind, Kind::Lens | Kind::CurvedMirror)
        .then_some(e.focal_length)
        .filter(|f| *f != 0. && !event.clip_only);
    let to_local = |[x, y]: [f64; 2]| {
        let offset = add(scale(bu, x), scale(bv, y));
        let point = sub(add(event.hit, offset), scale(d, dot(offset, normal) / cos));
        let l = local(sub(point, e.position), r);
        [l[1], l[2]]
    };
    // Smooth phase ramps must advance less than π per sample, or the grid would
    // alias them into a different direction. Check them before sampling.
    let origin = to_local(apply(g, [0., 0.]));
    let steps = [[grid.pitch, 0.], [0., grid.pitch]].map(|s| {
        let p = to_local(apply(g, s));
        [p[0] - origin[0], p[1] - origin[1]]
    });
    // Local-gradient shift (cycles per sample along each grid axis).
    let shift = |[gy, gz]: [f64; 2]| steps.map(|[sy, sz]| (gy * sy + gz * sz) / TAU);
    let mut shifts = Vec::new();
    if let Some(f) = focal {
        // The lens gradient k·r/f spans ± its clear-aperture edge value.
        let edge = k * e.aperture / f.abs();
        for g in [[edge, 0.], [-edge, 0.], [0., edge], [0., -edge]] {
            shifts.push(shift(g));
        }
    }
    match e.action.as_ref().filter(|_| !event.clip_only) {
        Some(Action::TipTilt {
            tilt_mrad: [ty, tz],
        }) => shifts.push(shift([k * 1e-3 * ty, k * 1e-3 * tz])),
        Some(Action::Slm { tilts_mrad, .. }) => {
            shifts.extend(
                tilts_mrad
                    .iter()
                    .map(|[ty, tz]| shift([k * 1e-3 * ty, k * 1e-3 * tz])),
            );
        }
        _ => {}
    }
    if !shifts.is_empty() {
        unaliased(a, grid, &shifts).map_err(|cycles| {
            format!(
                "{} phase ramp moves the field to {cycles:.2} cycles per sample on the {:.1} µm grid and would alias; refine pitch_um",
                e.id,
                grid.pitch * 1e3
            )
        })?;
    }
    for (i, v) in a.iter_mut().enumerate() {
        if *v == C::ZERO {
            continue;
        }
        let [ly, lz] = to_local(apply(g, grid.coords(i)));
        let open = match region {
            Some(region) => region.shape.contains(ly, lz),
            None => ly.hypot(lz) <= e.aperture,
        };
        if !open {
            *v = C::ZERO;
            continue;
        }
        if event.clip_only {
            continue;
        }
        let mut transmittance = 1.;
        let mut phase = 0.;
        if let Some(f) = focal {
            phase -= k * ((ly - hit[1]).powi(2) + (lz - hit[2]).powi(2)) / (2. * f);
        }
        match &e.action {
            Some(Action::Mask {
                shape: Some(shape),
                inside,
                outside,
                ..
            }) => {
                transmittance = if shape.contains(ly, lz) {
                    *inside
                } else {
                    *outside
                };
            }
            Some(Action::Mask {
                matrix: Some(rows),
                pitch_um: Some(pitch),
                outside,
                ..
            }) => {
                let pitch = pitch * 1e-3;
                let col = (ly / pitch + rows[0].len() as f64 / 2.).floor();
                let row = (-lz / pitch + rows.len() as f64 / 2.).floor();
                transmittance = if (0. ..rows[0].len() as f64).contains(&col)
                    && (0. ..rows.len() as f64).contains(&row)
                {
                    rows[row as usize][col as usize]
                } else {
                    *outside
                };
            }
            Some(Action::Slm {
                segments: [columns, rows],
                size_mm: [w, h],
                phases_rad,
                tilts_mrad,
            }) => {
                if ly.abs() <= w / 2. && lz.abs() <= h / 2. {
                    let col = (((ly + w / 2.) / w * *columns as f64) as usize).min(columns - 1);
                    let row = (((h / 2. - lz) / h * *rows as f64) as usize).min(rows - 1);
                    let index = row * columns + col;
                    phase += phases_rad[index];
                    if let Some([ty, tz]) = tilts_mrad.get(index) {
                        let yc = -w / 2. + (col as f64 + 0.5) * w / *columns as f64;
                        let zc = h / 2. - (row as f64 + 0.5) * h / *rows as f64;
                        phase += k * 1e-3 * (ty * (ly - yc) + tz * (lz - zc));
                    }
                }
            }
            Some(Action::TipTilt {
                tilt_mrad: [ty, tz],
            }) => phase += k * 1e-3 * (ty * ly + tz * lz),
            Some(Action::Rgg { .. }) => phase += screen.map_or(0., |s| s.at([ly, lz])),
            Some(Action::Nd { .. } | Action::Mask { .. }) | None => {}
        }
        *v = *v * C::cis(phase).scale(transmittance.sqrt());
    }
    Ok(())
}

/// Spatial-frequency band of `a` per grid axis (cycles per sample), trimming
/// 10⁻⁴ of the power from each tail.
fn band(a: &[C], grid: Grid) -> [[f64; 2]; 2] {
    let mut spectrum = a.to_vec();
    fft2(&mut spectrum, grid.n, false);
    std::array::from_fn(|axis| {
        let n = grid.n[axis];
        let mut marginal = vec![0.; n];
        for (i, v) in spectrum.iter().enumerate() {
            let bin = if axis == 0 {
                i % grid.n[0]
            } else {
                i / grid.n[0]
            };
            marginal[bin] += v.norm2();
        }
        // Order bins from −n/2 to n/2 − 1.
        let ordered: Vec<(f64, f64)> = (0..n)
            .map(|j| (j + n - n / 2) % n)
            .map(|bin| (signed(bin, n) / n as f64, marginal[bin]))
            .collect();
        let total: f64 = marginal.iter().sum();
        fn edge<'i>(items: impl Iterator<Item = &'i (f64, f64)>, threshold: f64) -> f64 {
            let mut sum = 0.;
            items
                .into_iter()
                .find(|(_, p)| {
                    sum += p;
                    sum > threshold
                })
                .map_or(0., |(f, _)| *f)
        }
        [
            edge(ordered.iter(), 1e-4 * total),
            edge(ordered.iter().rev(), 1e-4 * total),
        ]
    })
}

/// Whether every shift (cycles per sample) keeps the field's band within
/// Nyquist; otherwise the offending frequency.
fn unaliased(a: &[C], grid: Grid, shifts: &[[f64; 2]]) -> Result<(), f64> {
    let limits = band(a, grid);
    for s in shifts {
        for axis in 0..2 {
            let [lo, hi] = limits[axis];
            let worst = if s[axis] >= 0. {
                hi + s[axis]
            } else {
                lo + s[axis]
            };
            if worst.abs() >= 0.5 {
                return Err(worst);
            }
        }
    }
    Ok(())
}

/// The arm's end-plane field for one realization and its grid-to-physical map,
/// before mapping to the target basis.
fn compute(
    trace: &Trace,
    arm: &Arm,
    realization: usize,
    screens: &mut Screens,
    band: &mut Band,
) -> Result<(Vec<C>, M2), String> {
    let mut a = launch(arm.source, arm.grid);
    let mut g = IDENTITY;
    let mut events = arm.events.iter().peekable();
    let mut refractions = arm.refractions.iter().peekable();
    for (step, &index) in arm.chain.iter().enumerate() {
        while let Some(event) = events.next_if(|e| e.step == step) {
            let k = TAU * event.index / (arm.wavelength_nm * 1e-6);
            let s = screen(screens, arm.grid, event.element, realization);
            interact(&mut a, arm.grid, g, event, k, s)?;
        }
        while let Some(&(_, [tu, tv], m)) = refractions.next_if(|r| r.0 == step) {
            // Stretch the tangential axis by m; the amplitude keeps the power.
            let stretch = [
                [1. + (m - 1.) * tu * tu, (m - 1.) * tu * tv],
                [(m - 1.) * tu * tv, 1. + (m - 1.) * tv * tv],
            ];
            g = product(stretch, g);
            let gain = 1. / m.sqrt();
            a.iter_mut().for_each(|c| *c = c.scale(gain));
        }
        let f = trace.segments[index]
            .flight
            .as_ref()
            .expect("branch flights");
        let (sin, dropped) = propagate(
            &mut a,
            arm.grid,
            g,
            f.length_mm,
            f.wavelength_nm * 1e-6 / f.index,
        );
        band.sin = band.sin.min(sin);
        band.dropped = band.dropped.max(dropped);
    }
    let gain = C::cis(arm.piston).scale(arm.power.sqrt());
    a.iter_mut().for_each(|c| *c = *c * gain);
    Ok((a, g))
}

/// Resample an arm's end field onto the reference arm's plane and basis. The
/// √cos obliquity makes |A|² the flux through that plane, so an arm crossing
/// it at θ still integrates to its own power; `flux` supplies the cross terms.
/// Without `carrier` the arm keeps its own phase front: only valid for an arm
/// that couples to no other, whose intensity ignores the carrier.
fn map(arm: &Arm, reference: &Arm, (a, g): (Vec<C>, M2), carrier: bool) -> Result<Vec<C>, String> {
    let same = arm.end_point == reference.end_point && arm.end.basis == reference.end.basis;
    if same && g == IDENTITY {
        return Ok(a);
    }
    let d = arm.end.direction;
    let cos = dot(d, reference.end.direction);
    if cos < 0.05 {
        return Err(format!(
            "Branch {} crosses the plane of branch {} at grazing or reversed incidence",
            arm.label, reference.label
        ));
    }
    let obliquity = cos.sqrt();
    let k = TAU * arm.end_index / (arm.wavelength_nm * 1e-6);
    let k = if carrier { k } else { 0. };
    let [u0, v0] = reference.end.basis;
    let [u, v] = arm.end.basis;
    let to_grid = inverse(g);
    // Reference grid coordinates reach the arm grid through L = G⁻¹·P, with P
    // projecting along d onto the arm basis; frequencies map back through Lᵀ.
    let linear = product(
        to_grid,
        [[dot(u0, u), dot(v0, u)], [dot(u0, v), dot(v0, v)]],
    );
    if k != 0. {
        let carrier = [u0, v0].map(|b| k * dot(b, d) * reference.grid.pitch / TAU);
        let scale_pitch = arm.grid.pitch / reference.grid.pitch;
        let [[xlo, xhi], [ylo, yhi]] = band(&a, arm.grid);
        for corner in [[xlo, ylo], [xlo, yhi], [xhi, ylo], [xhi, yhi]] {
            let f = apply(transpose(linear), corner.map(|c| c / scale_pitch));
            let total = [f[0] + carrier[0], f[1] + carrier[1]];
            if let Some(cycles) = total.into_iter().find(|c| c.abs() >= 0.5) {
                return Err(format!(
                    "Branch {} reaches the plane of branch {} at {cycles:.2} cycles per sample on the {:.1} µm grid; refine pitch_um",
                    arm.label,
                    reference.label,
                    reference.grid.pitch * 1e3
                ));
            }
        }
    }
    // Reference coordinates c land at arm-grid position L·c + offset (u, v ⊥ d).
    let origin = sub(reference.end_point, arm.end_point);
    let offset = apply(to_grid, [dot(origin, u), dot(origin, v)]);
    let mapped = resample(&a, arm.grid, linear, offset);
    Ok(mapped
        .into_iter()
        .enumerate()
        .map(|(i, value)| {
            let [x, y] = reference.grid.coords(i);
            let along = dot(add(origin, add(scale(u0, x), scale(v0, y))), d);
            value * C::cis(k * along).scale(obliquity)
        })
        .collect())
}

/// Band-limited resampling of a complex field: output sample c (grid
/// coordinates, mm) reads `a` at `linear·c + offset`, zero outside the grid.
/// A pure translation is an exact Fourier shift. Any other map runs two exact
/// 1-D passes of the trigonometric interpolant: rows sheared along x, then
/// columns along y, after a 2× spectral upsampling in y that keeps the sheared
/// columns inside Nyquist. Kernel interpolation (bilinear, Lanczos) would scale
/// a frequency q by less than one at fractional offsets.
fn resample(a: &[C], grid: Grid, linear: M2, offset: [f64; 2]) -> Vec<C> {
    let [nx, ny] = grid.n;
    let steps = offset.map(|o| o / grid.pitch);
    let identity = (0..2).all(|r| (0..2).all(|c| (linear[r][c] - IDENTITY[r][c]).abs() < 1e-12));
    if identity {
        let whole = steps.map(f64::round);
        let fraction = [steps[0] - whole[0], steps[1] - whole[1]];
        let mut shifted = a.to_vec();
        if fraction != [0.; 2] {
            fft2(&mut shifted, grid.n, false);
            for (i, value) in shifted.iter_mut().enumerate() {
                let f = [
                    signed(i % nx, nx) / nx as f64,
                    signed(i / nx, ny) / ny as f64,
                ];
                *value = *value * C::cis(TAU * (f[0] * fraction[0] + f[1] * fraction[1]));
            }
            fft2(&mut shifted, grid.n, true);
        }
        let [wx, wy] = whole.map(|w| w as i64);
        return (0..grid.len())
            .map(|i| {
                let (x, y) = ((i % nx) as i64 + wx, (i / nx) as i64 + wy);
                if (0..nx as i64).contains(&x) && (0..ny as i64).contains(&y) {
                    shifted[y as usize * nx + x as usize]
                } else {
                    C::ZERO
                }
            })
            .collect();
    }
    // Source index s = L·c + o about the grid centre. Transpose the source when
    // that makes |b| ≤ |d|, so the row pass shears by at most one sample per row.
    let [[a0, b0], [c0, d0]] = linear;
    let (source, [sx, sy], [[la, lb], [lc, ld]], [o1, o2]) = if b0.abs() > d0.abs() {
        let transposed: Vec<C> = (0..grid.len()).map(|i| a[(i % ny) * nx + i / ny]).collect();
        (
            transposed,
            [ny, nx],
            [[c0, d0], [a0, b0]],
            [steps[1], steps[0]],
        )
    } else {
        (a.to_vec(), [nx, ny], linear, steps)
    };
    let (hx, hy) = ((sx / 2) as f64, (sy / 2) as f64);
    let (ox, oy) = ((nx / 2) as f64, (ny / 2) as f64);
    // 2× in y by zero-padding each column's spectrum; fine row r is y = r/2.
    let fine = 2 * sy;
    let mut upsampled = vec![C::ZERO; sx * fine];
    let mut column = vec![C::ZERO; sy];
    let mut padded = vec![C::ZERO; fine];
    for x in 0..sx {
        column
            .iter_mut()
            .enumerate()
            .for_each(|(y, c)| *c = source[y * sx + x]);
        fft(&mut column, false);
        padded.fill(C::ZERO);
        for (bin, value) in column.iter().enumerate() {
            let k = signed(bin, sy);
            padded[if k < 0. {
                (fine as f64 + k) as usize
            } else {
                bin
            }] = value.scale(2.);
        }
        fft(&mut padded, true);
        padded
            .iter()
            .enumerate()
            .for_each(|(r, c)| upsampled[r * sx + x] = *c);
    }
    // Row pass: B(X, y) = A(αX + βy + γ, y), with Y eliminated through s₂ = y.
    let det = la * ld - lb * lc;
    let (alpha, beta, gamma) = (det / ld, lb / ld, o1 - lb * o2 / ld);
    let mut sheared = vec![C::ZERO; nx * fine];
    let mut row = vec![C::ZERO; sx];
    for r in 0..fine {
        row.copy_from_slice(&upsampled[r * sx..(r + 1) * sx]);
        fft(&mut row, false);
        let y = r as f64 / 2. - hy;
        for x in 0..nx {
            let position = alpha * (x as f64 - ox) + beta * y + gamma + hx;
            sheared[r * nx + x] = interpolant(&row, position);
        }
    }
    // Column pass: out(X, Y) = B(X, cX + dY + o₂), in fine rows.
    let mut out = vec![C::ZERO; grid.len()];
    let mut column = vec![C::ZERO; fine];
    for x in 0..nx {
        column
            .iter_mut()
            .enumerate()
            .for_each(|(r, c)| *c = sheared[r * nx + x]);
        fft(&mut column, false);
        let cx = lc * (x as f64 - ox) + o2 + hy;
        for y in 0..ny {
            let position = cx + ld * (y as f64 - oy);
            // Outside the source rows the field is zero, not periodic.
            if (-0.5..=sy as f64 - 0.5).contains(&position) {
                out[y * nx + x] = interpolant(&column, 2. * position);
            }
        }
    }
    out
}

/// The trigonometric interpolant of a length-m sequence, from its forward
/// spectrum, at fractional index `position`; zero beyond half a sample outside.
fn interpolant(spectrum: &[C], position: f64) -> C {
    let m = spectrum.len();
    if !(-0.5..=m as f64 - 0.5).contains(&position) {
        return C::ZERO;
    }
    let step = C::cis(TAU * position / m as f64);
    let half = m / 2;
    // Bins 0..m/2 at e^{2πi k s/m}, then m/2..m at negative k.
    let mut phase = C::cis(-TAU * position * half as f64 / m as f64);
    let mut sum = C::ZERO;
    for k in 0..m {
        let bin = (k + m - half) % m;
        sum += spectrum[bin] * phase;
        phase = phase * step;
    }
    sum.scale(1. / m as f64)
}

fn coherence(a: &Arm, b: &Arm) -> f64 {
    if (a.wavelength_nm - b.wavelength_nm).abs() > 1e-9 {
        return 0.;
    }
    fn group(s: &Source) -> Option<&str> {
        s.coherence.as_ref().map(|c| c.group.as_str())
    }
    if a.source.id != b.source.id
        && (group(a.source).is_none() || group(a.source) != group(b.source))
    {
        return 0.;
    }
    // Validation gives every source of a group one length, so γ is a Gaussian
    // kernel of OPL within each group: positive semidefinite, and intensities stay ≥ 0.
    match a.source.coherence.as_ref().and_then(|c| c.length_mm) {
        Some(length) => (-((a.opl_mm - b.opl_mm) / length).powi(2)).exp(),
        None => 1.,
    }
}

/// Intensity of coherent and incoherent arms: Σ|A_a|² + Σ_{a<b} 2γ·Re(A_a·conj(A_b)·⟨J_a, J_b⟩).
fn intensity(fields: &[Vec<C>], weights: &[Vec<C>], out: &mut [f64]) {
    for (i, o) in out.iter_mut().enumerate() {
        let mut sum = 0.;
        for (a, fa) in fields.iter().enumerate() {
            sum += fa[i].norm2();
            for (b, fb) in fields.iter().enumerate().skip(a + 1) {
                sum += 2. * (fa[i] * fb[i].conj() * weights[a][b]).re;
            }
        }
        *o += sum;
    }
}

fn select<'t>(
    bench: &Bench,
    trace: &'t Trace,
    request: &FieldRequest,
) -> Result<Vec<(String, &'t BranchPath)>, String> {
    let ids: Vec<&str> = match (&request.branch, &request.port) {
        (Some(id), None) => vec![id.as_str()],
        (None, Some(port)) => bench
            .branches
            .iter()
            .filter(|b| b.to == *port)
            .map(|b| b.id.as_str())
            .collect(),
        _ => return Err("Give exactly one of branch or port".into()),
    };
    if ids.is_empty() {
        return Err("No named branch ends at the requested port".into());
    }
    let mut paths: Vec<(String, &BranchPath)> = Vec::new();
    for id in ids {
        let report = trace
            .branches
            .iter()
            .find(|r| r.id == id)
            .ok_or_else(|| format!("Unknown branch {id}"))?;
        if let Some(b) = report
            .interruption
            .as_ref()
            .or_else(|| report.paths.iter().find_map(|p| p.interruption.as_ref()))
        {
            return Err(format!(
                "Branch {id} is interrupted at {}/{}: {}",
                b.element, b.port, b.reason
            ));
        }
        let many = report.paths.len() > 1;
        for (i, path) in report.paths.iter().enumerate() {
            if request
                .wavelength
                .is_some_and(|w| (path.wavelength_nm - w).abs() > 1e-6)
                || paths.iter().any(|(_, p)| p.end_segment == path.end_segment)
            {
                continue;
            }
            paths.push((if many { format!("{id}#{i}") } else { id.into() }, path));
        }
    }
    if paths.is_empty() {
        return Err("No traced path matches the requested target and wavelength".into());
    }
    if paths
        .iter()
        .any(|(_, p)| (p.wavelength_nm - paths[0].1.wavelength_nm).abs() > 1e-6)
    {
        return Err(
            "Arms at the target carry several wavelengths; choose one with wavelength".into(),
        );
    }
    Ok(paths)
}

fn downsample(grid: Grid, values: impl Fn(usize) -> C, block: usize) -> Vec<Vec<C>> {
    let [nx, ny] = grid.n.map(|n| n / block);
    (0..ny)
        .map(|by| {
            (0..nx)
                .map(|bx| {
                    let mut sum = C::ZERO;
                    for y in by * block..(by + 1) * block {
                        for x in bx * block..(bx + 1) * block {
                            sum += values(y * grid.n[0] + x);
                        }
                    }
                    sum
                })
                .collect()
        })
        .collect()
}

/// Coherence, flux cross-weights and carrier choice of arms superposed on the
/// first arm's plane.
struct Coupling {
    gamma: Vec<Vec<f64>>,
    weights: Vec<Vec<C>>,
    carriers: Vec<bool>,
    uncarried: Vec<String>,
}

fn couple(arms: &[Arm]) -> Result<Coupling, String> {
    couple_on(arms, &arms[0])
}

/// `couple` on the plane and grid of `reference`, which need not be an arm of
/// the group (a readout ROI on the detector face).
fn couple_on(arms: &[Arm], reference: &Arm) -> Result<Coupling, String> {
    let grid = reference.grid;
    if arms.iter().any(|a| a.grid != grid) {
        return Err("Arms at the target need sources with the same field grid and pitch".into());
    }
    let n = arms.len();
    let gamma: Vec<Vec<f64>> = (0..n)
        .map(|a| (0..n).map(|b| coherence(&arms[a], &arms[b])).collect())
        .collect();
    let weights: Vec<Vec<C>> = (0..n)
        .map(|a| {
            (0..n)
                .map(|b| {
                    flux(arms[a].wave(), arms[b].wave(), reference.end.direction).scale(gamma[a][b])
                })
                .collect()
        })
        .collect();
    // The mapping carrier must advance less than π per sample. An arm that
    // couples to no other may drop it: its intensity does not depend on it.
    let mut carriers = vec![true; n];
    let mut uncarried = Vec::new();
    for (a, arm) in arms.iter().enumerate() {
        let step = carrier_step(arm, reference);
        if step < PI {
            continue;
        }
        if (0..n).any(|b| b != a && weights[a][b].norm2() > 1e-24) {
            return Err(format!(
                "Branch {} interferes with others but crosses the plane of branch {} too steeply for the {:.1} µm grid (carrier {:.2}π per sample); refine pitch_um",
                arm.label,
                reference.label,
                grid.pitch * 1e3,
                step / PI
            ));
        }
        carriers[a] = false;
        uncarried.push(arm.label.clone());
    }
    Ok(Coupling {
        gamma,
        weights,
        carriers,
        uncarried,
    })
}

/// Every arm's field for one realization, resampled onto the first arm's plane.
fn realize(
    trace: &Trace,
    arms: &[Arm],
    carriers: &[bool],
    realization: usize,
    screens: &mut Screens,
    band: &mut Band,
) -> Result<Vec<Vec<C>>, String> {
    arms.iter()
        .zip(carriers)
        .map(|(arm, &carrier)| {
            map(
                arm,
                &arms[0],
                compute(trace, arm, realization, screens, band)?,
                carrier,
            )
        })
        .collect()
}

pub(crate) fn field(
    bench: &Bench,
    system: &System,
    request: &FieldRequest,
) -> Result<FieldReading, String> {
    let scene = bench.resolved_stages()?;
    let bench = scene.as_ref();
    bench.validate()?;
    let trace = bench.trace_resolved(system)?;
    let realizations = request.realizations.unwrap_or(1);
    if !(1..=256).contains(&realizations) {
        return Err("realizations must be 1..256".into());
    }
    let arms = select(bench, &trace, request)?
        .into_iter()
        .map(|(label, path)| arm(bench, &trace, label, path))
        .collect::<Result<Vec<_>, _>>()?;
    let reference = &arms[0];
    let grid = reference.grid;
    let random = arms.iter().any(|a| {
        a.events
            .iter()
            .any(|e| matches!(e.element.action, Some(Action::Rgg { .. })))
    });
    let runs = if random { realizations } else { 1 };
    let n = arms.len();
    let Coupling {
        gamma,
        weights,
        carriers,
        uncarried,
    } = couple(&arms)?;
    let projection: Vec<C> = arms
        .iter()
        .map(|a| inner(a.vector, reference.vector))
        .collect();
    let mut near = vec![0.; grid.len()];
    let mut far = vec![0.; grid.len()];
    let mut cross = vec![vec![C::ZERO; n]; n];
    let mut coherent = vec![C::ZERO; grid.len()];
    let mut arm_near = vec![vec![0.; grid.len()]; n];
    let mut arm_phase = vec![C::ZERO; n];
    let mut screens = Screens::new();
    let mut band = Band {
        sin: 1.,
        dropped: 0.,
    };
    for realization in 0..runs {
        let fields = realize(
            &trace,
            &arms,
            &carriers,
            realization,
            &mut screens,
            &mut band,
        )?;
        intensity(&fields, &weights, &mut near);
        let spectra: Vec<Vec<C>> = fields
            .iter()
            .map(|f| {
                let mut s = f.clone();
                fft2(&mut s, grid.n, false);
                s
            })
            .collect();
        intensity(&spectra, &weights, &mut far);
        for a in 0..n {
            for b in 0..n {
                cross[a][b] += fields[a]
                    .iter()
                    .zip(&fields[b])
                    .fold(C::ZERO, |s, (x, y)| s + *x * y.conj())
                    * flux(arms[a].wave(), arms[b].wave(), reference.end.direction);
            }
        }
        for (a, f) in fields.iter().enumerate() {
            for (i, v) in f.iter().enumerate() {
                arm_near[a][i] += v.norm2();
                // Intensity-weighted phase: Σ A·|A|.
                arm_phase[a] += (*v * projection[a]).scale(v.norm2().sqrt());
            }
        }
        for (i, c) in coherent.iter_mut().enumerate() {
            for (a, f) in fields.iter().enumerate() {
                if gamma[0][a] > 0. {
                    *c += f[i] * projection[a];
                }
            }
        }
        // Each realization draws fresh screens; drop the finished ones.
        screens.retain(|(_, r), _| *r > realization);
    }
    let runs_f = runs as f64;
    near.iter_mut().for_each(|x| *x /= runs_f);
    let area = grid.pitch * grid.pitch;
    let total: f64 = near.iter().sum();
    let centroid = |values: &[f64]| -> [f64; 2] {
        let sum: f64 = values.iter().sum();
        if sum <= 0. {
            return [0.; 2];
        }
        let mut c = [0.; 2];
        for (i, v) in values.iter().enumerate() {
            let p = grid.coords(i);
            c[0] += p[0] * v;
            c[1] += p[1] * v;
        }
        c.map(|x| x / sum)
    };
    let wavelength_medium = reference.wavelength_nm * 1e-6 / reference.end_index;
    let far_centroid = {
        let sum: f64 = far.iter().sum();
        let mut c = [0.; 2];
        if sum > 0. {
            for (i, v) in far.iter().enumerate() {
                let f = grid.frequency(i);
                c[0] += f[0] * v;
                c[1] += f[1] * v;
            }
        }
        c.map(|x| {
            if sum > 0. {
                x / sum * wavelength_medium * 1e3
            } else {
                0.
            }
        })
    };
    let peak = near.iter().copied().fold(0., f64::max);
    let lit: Vec<f64> = near.iter().copied().filter(|v| *v > 1e-6 * peak).collect();
    let contrast = if lit.is_empty() {
        0.
    } else {
        let mean = lit.iter().sum::<f64>() / lit.len() as f64;
        (lit.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / lit.len() as f64).sqrt() / mean
    };
    let border = (grid.n[0].min(grid.n[1]) / 16).max(1);
    let edge: f64 = near
        .iter()
        .enumerate()
        .filter(|(i, _)| {
            let (x, y) = (i % grid.n[0], i / grid.n[0]);
            x < border || y < border || x >= grid.n[0] - border || y >= grid.n[1] - border
        })
        .map(|(_, v)| v)
        .sum();
    let edge_fraction = if total > 0. { edge / total } else { 0. };
    let max_angle = (wavelength_medium / (2. * grid.pitch)).min(1.).asin();
    let mut warnings: Vec<String> = uncarried
        .iter()
        .map(|label| format!("Branch {label} crosses the reference plane too steeply to sample its carrier; it adds incoherently and its far field is centred on its own axis"))
        .collect();
    if edge_fraction > 1e-3 {
        warnings.push(format!(
            "{:.2}% of the power reaches the window edge, where the periodic angular spectrum wraps it; enlarge the grid or pitch",
            100. * edge_fraction
        ));
    }
    if band.dropped > 1e-3 {
        warnings.push(format!(
            "A flight dropped {:.2}% of its power beyond the band limit of {:.3} mrad; enlarge the window",
            100. * band.dropped,
            band.sin.asin() * 1e3
        ));
    }
    let arms_out = arms
        .iter()
        .enumerate()
        .map(|(a, arm)| ArmReading {
            branch: arm.label.clone(),
            source: arm.source.id.clone(),
            power: cross[a][a].re / runs_f * area,
            centroid_mm: centroid(&arm_near[a]),
            phase_rad: arm_phase[a].arg(),
            opl_mm: arm.opl_mm,
            geometric_tilt_mrad: arm.geometric_tilt.map(|[gu, gv]| {
                let world = add(scale(arm.end.basis[0], gu), scale(arm.end.basis[1], gv));
                [
                    dot(world, reference.end.basis[0]),
                    dot(world, reference.end.basis[1]),
                ]
            }),
        })
        .collect();
    let pairs = (0..n)
        .flat_map(|a| (a + 1..n).map(move |b| (a, b)))
        .map(|(a, b)| {
            let norm = (cross[a][a].re * cross[b][b].re).sqrt();
            PairReading {
                arms: [a, b],
                coherence: gamma[a][b],
                correlation: if norm > 0. {
                    cross[a][b].norm2().sqrt() / norm
                } else {
                    0.
                },
            }
        })
        .collect();
    let mean_phase = coherent
        .iter()
        .fold(C::ZERO, |s, c| s + c.scale(c.norm2().sqrt()))
        .arg();
    let block = grid.n[0].max(grid.n[1]).div_ceil(64).max(1);
    let (intensity_rows, phase_rows) = if request.arrays {
        let rows = downsample(grid, |i| C::new(near[i], 0.), block);
        let phases = downsample(grid, |i| coherent[i], block);
        let cells = (block * block) as f64;
        (
            Some(
                rows.iter()
                    .map(|r| r.iter().map(|c| c.re / cells).collect())
                    .collect(),
            ),
            Some(
                phases
                    .iter()
                    .map(|r| r.iter().map(|c| c.arg()).collect())
                    .collect(),
            ),
        )
    } else {
        (None, None)
    };
    Ok(FieldReading {
        branch: request.branch.clone(),
        port: request.port.clone(),
        wavelength_nm: reference.wavelength_nm,
        realizations: runs,
        grid: grid.n,
        pitch_um: grid.pitch * 1e3,
        center: reference.end_point,
        direction: reference.end.direction,
        basis: reference.end.basis,
        power: total * area,
        centroid_mm: centroid(&near),
        far_field_centroid_mrad: far_centroid,
        mean_phase_rad: mean_phase,
        contrast,
        arms: arms_out,
        pairs,
        sampling: Sampling {
            window_mm: grid.n.map(|n| n as f64 * grid.pitch),
            max_angle_mrad: max_angle * 1e3,
            band_limit_mrad: band.sin.asin() * 1e3,
            edge_fraction,
            warnings,
        },
        intensity: intensity_rows,
        phase_rad: phase_rows,
        array_pitch_um: request.arrays.then_some(grid.pitch * 1e3 * block as f64),
    })
}

#[cfg(test)]
mod tests {
    use super::super::{DETECTOR, LENS, MASK, MIRROR, SPLITTER, mount_element, template};
    use super::*;
    use crate::optics::bench::{Branch, Connection, FieldSampling, PhasePlate, Substrate};

    fn sampling(grid: usize, pitch_um: f64, profile: FieldProfile, waist_um: f64) -> FieldSampling {
        FieldSampling {
            grid: [grid; 2],
            pitch_um,
            profile,
            waist_um,
            polarization: Some([[1., 0.], [0., 0.]]),
        }
    }

    fn request(branch: Option<&str>, port: Option<Port>, realizations: usize) -> FieldRequest {
        FieldRequest {
            branch: branch.map(Into::into),
            port,
            wavelength: None,
            realizations: Some(realizations),
            arrays: false,
        }
    }

    /// Source S1 at (100, 200) heading +x, a fold mirror M at x = 200 sending
    /// the beam +y, and detector D at `length` mm beyond it.
    fn fold(action: Option<Action>, length: f64, field: FieldSampling) -> Bench {
        let mut bench = template("mach_zehnder").unwrap();
        bench.assertions.clear();
        bench.sources[0].field = Some(field);
        let mut mirror = mount_element("M", MIRROR, [200., 200., 75.], [-1., 1., 0.]);
        mirror.action = action;
        bench.elements = vec![
            mirror,
            mount_element("D", DETECTOR, [200., 200. + length, 75.], [0., 1., 0.]),
        ];
        bench.connections = [("S1", "out", "M"), ("M", "reflected", "D")]
            .map(|(from, port, to)| Connection {
                from: from.into(),
                port: port.into(),
                to: to.into(),
            })
            .into();
        bench.branches = vec![Branch {
            id: "beam".into(),
            from: Port::new("S1", "out"),
            to: Port::new("D", "in"),
            via: vec![],
            style: None,
        }];
        bench
    }

    /// The Mach–Zehnder template closed on `detector` (D1 or D2) through both BS2
    /// outputs, with an optional aperture G before BS1.
    fn interferometer(field: FieldSampling, glass: Option<Action>, detector: &str) -> Bench {
        let mut bench = template("mach_zehnder").unwrap();
        bench.assertions.clear();
        bench.sources[0].field = Some(field);
        for port in ["transmitted", "reflected"] {
            bench.connections.push(Connection {
                from: "BS2".into(),
                port: port.into(),
                to: detector.into(),
            });
        }
        if glass.is_some() {
            let mut g = mount_element("G", MASK, [175., 200., 75.], [1., 0., 0.]);
            g.action = glass;
            bench.elements.push(g);
            bench.connections[0].to = "G".into();
            bench.connections.push(Connection {
                from: "G".into(),
                port: "out".into(),
                to: "BS1".into(),
            });
        }
        // At D1 arm a leaves BS2 transmitted and arm b reflected; at D2 the reverse.
        let ends = if detector == "D1" {
            ["transmitted", "reflected"]
        } else {
            ["reflected", "transmitted"]
        };
        bench.branches = [("a", "reflected", ends[0]), ("b", "transmitted", ends[1])]
            .map(|(id, start, end)| Branch {
                id: id.into(),
                from: Port::new("BS1", start),
                to: Port::new(detector, "in"),
                via: vec![Port::new("BS2", end)],
                style: None,
            })
            .into();
        bench
    }

    #[test]
    fn fft_round_trip_and_screen_statistics() {
        let grid = Grid {
            n: [16, 8],
            pitch: 0.01,
        };
        let data: Vec<C> = (0..grid.len())
            .map(|i| C::new((i as f64).sin(), (i as f64 * 0.3).cos()))
            .collect();
        let mut work = data.clone();
        fft2(&mut work, grid.n, false);
        fft2(&mut work, grid.n, true);
        assert!(
            data.iter()
                .zip(&work)
                .all(|(a, b)| (*a - *b).norm2() < 1e-20)
        );
        let screen = Screen::new(
            Grid {
                n: [64; 2],
                pitch: 0.01,
            },
            0.04,
            2.,
            7,
        );
        let rms =
            (screen.phase.iter().map(|p| p * p).sum::<f64>() / screen.phase.len() as f64).sqrt();
        assert!((rms - 2.).abs() < 1e-9);
    }

    #[test]
    fn nd_scales_traced_and_field_power() {
        let system = System::default();
        let field = sampling(64, 40., FieldProfile::Gaussian, 500.);
        let plain = fold(None, 100., field.clone());
        let dimmed = fold(Some(Action::Nd { od: 1. }), 100., field);
        let power = |bench: &Bench| {
            let trace = bench.trace(&system).unwrap();
            let traced = trace.segments[trace.branches[0].paths[0].end_segment.unwrap()].power;
            (
                traced,
                super::field(bench, &system, &request(Some("beam"), None, 1))
                    .unwrap()
                    .power,
            )
        };
        let ((t0, f0), (t1, f1)) = (power(&plain), power(&dimmed));
        assert!((t1 / t0 - 0.1).abs() < 1e-12, "{t0} {t1}");
        assert!((f1 / f0 - 0.1).abs() < 1e-9, "{f0} {f1}");
        assert!(
            dimmed.elements[0]
                .action
                .as_ref()
                .is_some_and(Action::valid)
        );
        let mut bad = plain.clone();
        bad.elements[1].action = Some(Action::Nd { od: 1. });
        assert!(bad.validate().unwrap_err().contains("Invalid action on D"));
    }

    #[test]
    fn a_region_behind_a_substrate_masks_the_field_where_it_leaves_the_glass() {
        // S1 heads +x into a mask turned 30° whose 20 mm substrate faces the
        // source. The beam enters the back face 20·tan 30° off the mask axis and
        // the glass carries it back by the refracted run to a hole X on the front
        // face, which must pass the field whole.
        let system = System::default();
        let mut bench = template("mach_zehnder").unwrap();
        bench.assertions.clear();
        bench.sources[0].field = Some(sampling(64, 20., FieldProfile::Gaussian, 200.));
        let (s, c) = 30f64.to_radians().sin_cos();
        let mut mask = mount_element("AP", MASK, [200., 200., 75.], [c, s, 0.]);
        mask.action = None;
        mask.aperture = 25.;
        mask.substrate = Some(Substrate {
            material: "N-BK7_SCHOTT".into(),
            thickness_mm: 20.,
            wedge_deg: None,
            coated_face: None,
        });
        let n = system
            .material_index("N-BK7_SCHOTT", bench.sources[0].wavelength)
            .unwrap();
        let run = 20. * (s / n) / (1. - (s / n).powi(2)).sqrt();
        let y = 20. * s / c - run;
        mask.regions = vec![bench::Region {
            port: "X".into(),
            shape: bench::RegionShape::Circle {
                center: [y, 0.],
                radius: 2.,
            },
            transmittance: 1.,
            normal: None,
        }];
        bench.elements = vec![
            mask,
            mount_element("D", DETECTOR, [300., 200. + c * y, 75.], [1., 0., 0.]),
        ];
        bench.connections = [("S1", "out", "AP"), ("AP", "X", "D")]
            .map(|(from, port, to)| Connection {
                from: from.into(),
                port: port.into(),
                to: to.into(),
            })
            .into();
        bench.branches = vec![Branch {
            id: "X".into(),
            from: Port::new("AP", "X"),
            to: Port::new("D", "in"),
            via: vec![],
            style: None,
        }];
        bench.validate().unwrap();
        let reading = super::field(&bench, &system, &request(Some("X"), None, 1)).unwrap();
        let launched = bench.sources[0].power;
        assert!(
            (reading.power - launched).abs() < 1e-3 * launched,
            "{} {launched}",
            reading.power
        );
    }

    #[test]
    fn a_lens_substrate_is_propagated_before_the_lens_power() {
        // S1 heads +x from x = 100 to a detector at x = 300 through an f = 150 lens
        // at x = 200. Glass 10 mm thick ahead of a thin lens must give the field a
        // lens with that substrate gives: the power acts where the light leaves the glass.
        let system = System::default();
        let substrate = || {
            Some(Substrate {
                material: "N-BK7_SCHOTT".into(),
                thickness_mm: 10.,
                wedge_deg: None,
                coated_face: None,
            })
        };
        let read = |plate: bool| {
            let mut bench = template("mach_zehnder").unwrap();
            bench.assertions.clear();
            bench.sources[0].field = Some(sampling(128, 8., FieldProfile::Gaussian, 200.));
            let mut lens = mount_element("L", LENS, [200., 200., 75.], [1., 0., 0.]);
            lens.focal_length = 150.;
            lens.aperture = 1.;
            let mut chain = vec![("S1", "out", "L")];
            bench.elements = vec![
                lens,
                mount_element("D", DETECTOR, [300., 200., 75.], [1., 0., 0.]),
            ];
            if plate {
                let mut p = mount_element("P", MASK, [195., 200., 75.], [1., 0., 0.]);
                p.action = None;
                p.substrate = substrate();
                bench.elements.push(p);
                chain = vec![("S1", "out", "P"), ("P", "out", "L")];
            } else {
                bench.elements[0].substrate = substrate();
            }
            chain.push(("L", "out", "D"));
            bench.connections = chain
                .into_iter()
                .map(|(from, port, to)| Connection {
                    from: from.into(),
                    port: port.into(),
                    to: to.into(),
                })
                .collect();
            bench.branches = vec![Branch {
                id: "beam".into(),
                from: Port::new("S1", "out"),
                to: Port::new("D", "in"),
                via: vec![],
                style: None,
            }];
            bench.validate().unwrap();
            let request = FieldRequest {
                arrays: true,
                ..request(Some("beam"), None, 1)
            };
            super::field(&bench, &system, &request).unwrap()
        };
        let (thick, separate) = (read(false), read(true));
        let (a, b) = (thick.intensity.unwrap(), separate.intensity.unwrap());
        let peak = b.iter().flatten().cloned().fold(0., f64::max);
        let worst = a
            .iter()
            .flatten()
            .zip(b.iter().flatten())
            .map(|(x, y)| (x - y).abs())
            .fold(0., f64::max);
        assert!(worst < 1e-3 * peak, "{worst} vs peak {peak}");
        assert!(
            (thick.power - separate.power).abs() < 1e-6,
            "{} {}",
            thick.power,
            separate.power
        );
    }

    #[test]
    fn leaving_a_thick_lens_off_axis_is_not_a_refraction() {
        // The chief ray crosses the slab 5 mm off the axis of an f = 100 lens:
        // the flat exit face keeps its direction, so the lens turn adds no stretch.
        let system = System::default();
        let mut bench = template("mach_zehnder").unwrap();
        bench.assertions.clear();
        bench.sources[0].field = Some(sampling(32, 20., FieldProfile::Gaussian, 100.));
        let mut lens = mount_element("L", LENS, [200., 205., 75.], [1., 0., 0.]);
        lens.substrate = Some(Substrate {
            material: "N-BK7_SCHOTT".into(),
            thickness_mm: 10.,
            wedge_deg: None,
            coated_face: None,
        });
        bench.elements = vec![
            lens,
            mount_element("D", DETECTOR, [280., 200., 75.], [1., 0., 0.]),
        ];
        bench.connections = [("S1", "out", "L"), ("L", "out", "D")]
            .map(|(from, port, to)| Connection {
                from: from.into(),
                port: port.into(),
                to: to.into(),
            })
            .into();
        bench.branches = vec![Branch {
            id: "beam".into(),
            from: Port::new("S1", "out"),
            to: Port::new("D", "in"),
            via: vec![],
            style: None,
        }];
        bench.validate().unwrap();
        let trace = bench.trace(&system).unwrap();
        let path = &trace.branches[0].paths[0];
        let arm = arm(&bench, &trace, "beam".into(), path).unwrap();
        assert!(
            arm.refractions.is_empty(),
            "{:?}",
            arm.refractions.iter().map(|r| r.2).collect::<Vec<_>>()
        );
        // The entry face clips; the exit applies the power.
        assert_eq!(
            arm.events.iter().map(|e| e.clip_only).collect::<Vec<_>>(),
            [true, false]
        );
    }

    #[test]
    fn a_thick_lens_clips_to_its_aperture_at_the_glass_entry() {
        // A 10 mm slab lens of 5 mm clear radius tilted 30°: the field is clipped
        // on the entry face before the glass (no power there), so a sample
        // entering at y = −6 mm is blocked even though its ray would leave the
        // glass inside the aperture.
        let system = System::default();
        let (c, s) = (30_f64.to_radians().cos(), 30_f64.to_radians().sin());
        let mut bench = template("mach_zehnder").unwrap();
        bench.assertions.clear();
        bench.sources[0].field = Some(sampling(32, 20., FieldProfile::Gaussian, 100.));
        // Shifted along its local y so the chief ray's glass path straddles the axis.
        let mut lens = mount_element("L", LENS, [200. - 4. * s, 200. + 4. * c, 75.], [c, s, 0.]);
        lens.aperture = 5.;
        lens.substrate = Some(Substrate {
            material: "N-BK7_SCHOTT".into(),
            thickness_mm: 10.,
            wedge_deg: None,
            coated_face: None,
        });
        let mut detector = mount_element("D", DETECTOR, [300., 200., 75.], [1., 0., 0.]);
        detector.aperture = 40.;
        detector.body_radius = 50.;
        bench.elements = vec![lens, detector];
        bench.connections = [("S1", "out", "L"), ("L", "out", "D")]
            .map(|(from, port, to)| Connection {
                from: from.into(),
                port: port.into(),
                to: to.into(),
            })
            .into();
        bench.branches = vec![Branch {
            id: "beam".into(),
            from: Port::new("S1", "out"),
            to: Port::new("D", "in"),
            via: vec![],
            style: None,
        }];
        bench.validate().unwrap();
        let trace = bench.trace(&system).unwrap();
        let path = &trace.branches[0].paths[0];
        assert!(path.nominal_match, "{:?}", path.interruption);
        let arm = arm(&bench, &trace, "beam".into(), path).unwrap();
        let lens_events: Vec<&Event> = arm.events.iter().filter(|e| e.element.id == "L").collect();
        assert_eq!(
            lens_events.iter().map(|e| e.clip_only).collect::<Vec<_>>(),
            [true, false]
        );
        let (entry, exit) = (lens_events[0], lens_events[1]);
        let e = entry.element;
        let [front, back] = slab_faces(e, e.substrate.as_ref().unwrap());
        let on = |face: &bench::SlabFace, p: V3| dot(sub(p, face.point), face.normal).abs() < 1e-9;
        assert!(
            on(&back, entry.hit) && on(&front, exit.hit),
            "light enters the back face and leaves the front"
        );
        let grid = Grid {
            n: [32, 32],
            pitch: 0.5,
        };
        let mut a = vec![C::new(1., 0.); grid.len()];
        let k = TAU * entry.index / (arm.wavelength_nm * 1e-6);
        interact(&mut a, grid, IDENTITY, entry, k, None).unwrap();
        let (d, glass) = (entry.incoming.direction, exit.incoming.direction);
        let mut blocked_but_exits_inside = false;
        for (i, v) in a.iter().enumerate() {
            let [x, y] = grid.coords(i);
            let start = add(
                add(entry.hit, scale(entry.incoming.basis[0], x)),
                scale(entry.incoming.basis[1], y),
            );
            let at_entry = sub(
                start,
                scale(
                    d,
                    dot(sub(start, back.point), back.normal) / dot(d, back.normal),
                ),
            );
            let l = local(sub(at_entry, e.position), e.rotation());
            let inside = l[1].hypot(l[2]) <= e.aperture;
            // Clip only: open samples keep their amplitude and phase.
            assert!(
                if inside {
                    (v.re - 1.).abs() < 1e-12 && v.im.abs() < 1e-12
                } else {
                    *v == C::ZERO
                },
                "sample {i} at {l:?}: {v:?}"
            );
            let at_exit = sub(
                at_entry,
                scale(
                    glass,
                    dot(sub(at_entry, front.point), front.normal) / dot(glass, front.normal),
                ),
            );
            let m = local(sub(at_exit, e.position), e.rotation());
            blocked_but_exits_inside |= !inside && m[1].hypot(m[2]) <= e.aperture;
        }
        assert!(blocked_but_exits_inside);
    }

    #[test]
    fn a_wedged_lens_samples_its_power_on_the_traced_exit_face() {
        // A 10 mm, 10° wedged slab lens tilted 30°: the lens power is applied
        // where the light leaves the glass, so each sample's lens height is where
        // its ray, along the glass direction, meets the traced (wedged) back face.
        let system = System::default();
        let (c, s) = (30_f64.to_radians().cos(), 30_f64.to_radians().sin());
        let mut bench = template("mach_zehnder").unwrap();
        bench.assertions.clear();
        bench.sources[0].field = Some(sampling(32, 20., FieldProfile::Gaussian, 100.));
        // Facing back along the beam, so the light leaves through the wedged back face.
        let mut lens = mount_element("L", LENS, [200., 200., 75.], [-c, -s, 0.]);
        lens.substrate = Some(Substrate {
            material: "N-BK7_SCHOTT".into(),
            thickness_mm: 10.,
            wedge_deg: Some(10.),
            coated_face: None,
        });
        let mut detector = mount_element("D", DETECTOR, [300., 200., 75.], [1., 0., 0.]);
        detector.aperture = 40.;
        detector.body_radius = 50.;
        bench.elements = vec![lens, detector];
        bench.connections = [("S1", "out", "L"), ("L", "out", "D")]
            .map(|(from, port, to)| Connection {
                from: from.into(),
                port: port.into(),
                to: to.into(),
            })
            .into();
        bench.branches = vec![Branch {
            id: "beam".into(),
            from: Port::new("S1", "out"),
            to: Port::new("D", "in"),
            via: vec![],
            style: None,
        }];
        bench.validate().unwrap();
        let trace = bench.trace(&system).unwrap();
        let path = &trace.branches[0].paths[0];
        assert!(path.nominal_match, "{:?}", path.interruption);
        let arm = arm(&bench, &trace, "beam".into(), path).unwrap();
        let event = arm
            .events
            .iter()
            .find(|e| e.element.id == "L" && !e.clip_only)
            .unwrap();
        let e = event.element;
        let back = &slab_faces(e, e.substrate.as_ref().unwrap())[1];
        assert!(
            dot(sub(event.hit, back.point), back.normal).abs() < 1e-9,
            "the lens event sits on the exit face"
        );
        let pitch = 1e-4;
        let grid = Grid { n: [4, 4], pitch };
        let mut a = vec![C::new(1., 0.); grid.len()];
        let k = TAU * event.index / (arm.wavelength_nm * 1e-6);
        interact(&mut a, grid, IDENTITY, event, k, None).unwrap();
        let d = event.incoming.direction;
        let hit = local(sub(event.hit, e.position), e.rotation());
        let mut wedge_matters = false;
        for (i, offset) in [(11, event.incoming.basis[0]), (14, event.incoming.basis[1])] {
            let start = add(event.hit, scale(offset, pitch));
            let on_face = sub(
                start,
                scale(
                    d,
                    dot(sub(start, back.point), back.normal) / dot(d, back.normal),
                ),
            );
            let l = local(sub(on_face, e.position), e.rotation());
            let expected =
                -k * ((l[1] - hit[1]).powi(2) + (l[2] - hit[2]).powi(2)) / (2. * e.focal_length);
            assert!(
                (a[i].arg() - expected).abs() < 1e-6 * expected.abs(),
                "sample {i}: {} vs {expected}",
                a[i].arg()
            );
            // On the element plane instead, the height would differ by several percent.
            let on_plane = sub(
                start,
                scale(
                    d,
                    dot(sub(start, event.hit), rotate([1., 0., 0.], e.rotation()))
                        / dot(d, rotate([1., 0., 0.], e.rotation())),
                ),
            );
            let p = local(sub(on_plane, e.position), e.rotation());
            let plane =
                -k * ((p[1] - hit[1]).powi(2) + (p[2] - hit[2]).powi(2)) / (2. * e.focal_length);
            wedge_matters |= (plane / expected - 1.).abs() > 0.05;
        }
        assert!(wedge_matters);
    }

    #[test]
    fn an_oblique_thick_lens_stretches_in_and_back_out_of_its_glass() {
        // A 10 mm slab lens tilted 30° about z, met off its axis in plane (y) and
        // out of plane (z, so the lens turns the ray out of the refraction plane):
        // the entry and exit stretches act on the same grid axis and cancel, so
        // their combined grid map is the identity.
        let system = System::default();
        let (c, s) = (30_f64.to_radians().cos(), 30_f64.to_radians().sin());
        for offset in [[5., 0.], [0., 6.]] {
            let mut bench = template("mach_zehnder").unwrap();
            bench.assertions.clear();
            bench.sources[0].field = Some(sampling(32, 20., FieldProfile::Gaussian, 100.));
            let mut lens = mount_element(
                "L",
                LENS,
                [200. + offset[0] * s, 200. - offset[0] * c, 75. - offset[1]],
                [c, s, 0.],
            );
            lens.substrate = Some(Substrate {
                material: "N-BK7_SCHOTT".into(),
                thickness_mm: 10.,
                wedge_deg: None,
                coated_face: None,
            });
            let mut detector = mount_element("D", DETECTOR, [300., 200., 75.], [1., 0., 0.]);
            detector.aperture = 40.;
            detector.body_radius = 50.;
            bench.elements = vec![lens, detector];
            bench.connections = [("S1", "out", "L"), ("L", "out", "D")]
                .map(|(from, port, to)| Connection {
                    from: from.into(),
                    port: port.into(),
                    to: to.into(),
                })
                .into();
            bench.branches = vec![Branch {
                id: "beam".into(),
                from: Port::new("S1", "out"),
                to: Port::new("D", "in"),
                via: vec![],
                style: None,
            }];
            bench.validate().unwrap();
            let trace = bench.trace(&system).unwrap();
            let path = &trace.branches[0].paths[0];
            assert!(path.nominal_match, "{offset:?}: {:?}", path.interruption);
            let arm = arm(&bench, &trace, "beam".into(), path).unwrap();
            assert_eq!(arm.refractions.len(), 2, "{offset:?}");
            let n = system
                .material_index("N-BK7_SCHOTT", arm.wavelength_nm)
                .unwrap();
            let entry = (s / n).asin().cos() / c;
            assert!(
                (arm.refractions[0].2 - entry).abs() < 1e-9,
                "{offset:?}: {} vs {entry}",
                arm.refractions[0].2
            );
            // Grid map of a stretch m along unit axis a: I + (m − 1)·a·aᵀ.
            let stretch = |&(_, [x, y], m): &(usize, [f64; 2], f64)| {
                [
                    [1. + (m - 1.) * x * x, (m - 1.) * x * y],
                    [(m - 1.) * x * y, 1. + (m - 1.) * y * y],
                ]
            };
            let (a, b) = (stretch(&arm.refractions[0]), stretch(&arm.refractions[1]));
            let g = [
                [
                    b[0][0] * a[0][0] + b[0][1] * a[1][0],
                    b[0][0] * a[0][1] + b[0][1] * a[1][1],
                ],
                [
                    b[1][0] * a[0][0] + b[1][1] * a[1][0],
                    b[1][0] * a[0][1] + b[1][1] * a[1][1],
                ],
            ];
            assert!(
                [g[0][0] - 1., g[0][1], g[1][0], g[1][1] - 1.]
                    .iter()
                    .all(|x| x.abs() < 1e-9),
                "{offset:?}: grid map {g:?}"
            );
        }
    }

    #[test]
    fn tip_tilt_moves_the_far_field_by_the_geometric_deflection() {
        let system = System::default();
        let [ty, tz] = [0.8, -0.5];
        let read = |action| {
            let bench = fold(
                action,
                100.,
                sampling(128, 20., FieldProfile::Gaussian, 300.),
            );
            super::field(&bench, &system, &request(Some("beam"), None, 1)).unwrap()
        };
        let (flat, reading) = (
            read(None),
            read(Some(Action::TipTilt {
                tilt_mrad: [ty, tz],
            })),
        );
        // M folds +x into +y with normal (−1, 1, 0)/√2, local y = (−1, −1, 0)/√2 and
        // local z = +z. The exit ray keeps the in-plane phase gradient t and stays
        // transverse: δ = t + n·ty = (−√2·ty, 0, tz).
        let world = [-std::f64::consts::SQRT_2 * ty, 0., tz];
        let expected = reading.basis.map(|b| dot(world, b));
        let geometric = reading.arms[0].geometric_tilt_mrad.unwrap();
        for axis in 0..2 {
            assert!(
                (geometric[axis] - expected[axis]).abs() < 1e-9,
                "{geometric:?} {expected:?}"
            );
            let moved = reading.far_field_centroid_mrad[axis] - flat.far_field_centroid_mrad[axis];
            assert!(
                (moved - expected[axis]).abs() < 0.01,
                "{:?} vs {expected:?}",
                reading.far_field_centroid_mrad
            );
            // The near-field spot walks by the deflection over the 100 mm flight.
            let walk = reading.centroid_mm[axis] - flat.centroid_mm[axis];
            assert!(
                (walk - expected[axis] * 0.1).abs() < 2e-3,
                "{walk} {expected:?}"
            );
        }
        assert!(
            reading.sampling.warnings.is_empty(),
            "{:?}",
            reading.sampling.warnings
        );
    }

    #[test]
    fn equal_arm_interferometer_conserves_power_between_complementary_outputs() {
        let system = System::default();
        let field = sampling(64, 40., FieldProfile::Gaussian, 500.);
        // Each arm meets one mirror; both splitters are lossless 50:50.
        let delivered = template("mach_zehnder").unwrap().sources[0].power * MIRROR.reflectance;
        let read = |plate: f64, detector: &str| {
            let mut bench = interferometer(field.clone(), None, detector);
            let wavelength = bench.sources[0].wavelength;
            bench.elements[0].phase = Some(PhasePlate {
                radians: Some(plate),
                waves: None,
                at_wavelength_nm: wavelength,
            });
            let reading = super::field(
                &bench,
                &system,
                &request(None, Some(Port::new(detector, "in")), 1),
            )
            .unwrap();
            assert_eq!(reading.arms.len(), 2);
            reading.power
        };
        // Equal arms: D1 collects r·t + t·r (phases π/2 + π on both), D2 collects
        // r·r and t·t (π/2 + π + π/2 against π), which cancel. A π plate on the
        // BS1 transmitted arm swaps the outputs.
        for (plate, bright, dark) in [(0., "D1", "D2"), (PI, "D2", "D1")] {
            let (b, d) = (read(plate, bright), read(plate, dark));
            assert!(
                (b - delivered).abs() < 1e-3 * delivered,
                "{plate}: {b} {delivered}"
            );
            assert!(d < 1e-3 * delivered, "{plate}: {d}");
        }
        // Quadrature splits the power evenly.
        let (q1, q2) = (read(PI / 2., "D1"), read(PI / 2., "D2"));
        assert!(
            (q1 - delivered / 2.).abs() < 1e-3 * delivered
                && (q2 - delivered / 2.).abs() < 1e-3 * delivered,
            "{q1} {q2}"
        );
    }

    #[test]
    fn oblique_arms_deliver_their_source_power_and_orthogonal_ones_do_not_interfere() {
        let system = System::default();
        let mut bench = template("mach_zehnder").unwrap();
        bench.assertions.clear();
        // S2 meets D at 60° to S1, vertically polarized: its world field (z) is
        // orthogonal to S1's horizontal one (y).
        let (c, s) = (0.5, 3f64.sqrt() / 2.);
        let mut second = bench.sources[0].clone();
        second.id = "S2".into();
        second.position = [200. - 100. * c, 200. - 100. * s, 75.];
        second.direction = [c, s, 0.];
        bench.sources.push(second);
        for (i, source) in bench.sources.iter_mut().enumerate() {
            source.coherence = Some(crate::optics::bench::Coherence {
                group: "laser".into(),
                length_mm: None,
            });
            let mut field = sampling(128, 20., FieldProfile::Gaussian, 200.);
            if i == 1 {
                field.polarization = Some([[0., 0.], [1., 0.]]);
            }
            source.field = Some(field);
        }
        let delivered: f64 = bench.sources.iter().map(|s| s.power).sum();
        bench.elements = vec![mount_element(
            "D",
            DETECTOR,
            [200., 200., 75.],
            [1. + c, s, 0.],
        )];
        bench.connections = [("S1", "out", "D"), ("S2", "out", "D")]
            .map(|(from, port, to)| Connection {
                from: from.into(),
                port: port.into(),
                to: to.into(),
            })
            .into();
        bench.branches = ["S1", "S2"]
            .map(|s| Branch {
                id: s.into(),
                from: Port::new(s, "out"),
                to: Port::new("D", "in"),
                via: vec![],
                style: None,
            })
            .into();
        let reading = super::field(
            &bench,
            &system,
            &request(None, Some(Port::new("D", "in")), 1),
        )
        .unwrap();
        assert_eq!(reading.pairs[0].coherence, 1.);
        assert!(reading.pairs[0].correlation < 1e-9);
        // S2's footprint on S1's plane is twice as long; without the obliquity it
        // would count double.
        assert!(
            (reading.power - delivered).abs() < 1e-3 * delivered,
            "{} {delivered}",
            reading.power
        );
        // Its 60° carrier cannot be sampled; with no coupling it is dropped, and said so.
        assert!(
            reading
                .sampling
                .warnings
                .iter()
                .any(|w| w.contains("Branch S2 crosses")),
            "{:?}",
            reading.sampling.warnings
        );
        // Same polarization would couple the arms, so the unsampled carrier is an error.
        bench.sources[1].field.as_mut().unwrap().polarization = Some([[1., 0.], [0., 0.]]);
        let err = super::field(
            &bench,
            &system,
            &request(None, Some(Port::new("D", "in")), 1),
        )
        .unwrap_err();
        assert!(err.contains("too steeply"), "{err}");
        // One group, one coherence length: otherwise γ need not be positive semidefinite.
        bench.sources[1].coherence.as_mut().unwrap().length_mm = Some(0.1);
        assert!(
            bench
                .validate()
                .unwrap_err()
                .contains("different length_mm")
        );
    }

    #[test]
    fn oblique_plate_propagates_tangential_and_sagittal_axes_differently() {
        let system = System::default();
        let (thickness, w0) = (20., 0.05);
        let mut bench = fold(
            None,
            100.,
            sampling(256, 6., FieldProfile::Gaussian, w0 * 1e3),
        );
        let wavelength = bench.sources[0].wavelength;
        let n = system.material_index("N-BK7_SCHOTT", wavelength).unwrap();
        // A plate at 45° in the table plane. Its front face holds the element
        // position and the beam enters the back face at its centre, x = 130.
        let h = std::f64::consts::FRAC_1_SQRT_2;
        let entry = 130.;
        let mut plate = mount_element(
            "P",
            MASK,
            [entry + thickness * h, 200. + thickness * h, 75.],
            [h, h, 0.],
        );
        plate.substrate = Some(crate::optics::bench::Substrate {
            material: "N-BK7_SCHOTT".into(),
            thickness_mm: thickness,
            wedge_deg: None,
            coated_face: None,
        });
        let sin_t = std::f64::consts::FRAC_1_SQRT_2 / n;
        let cos_t = (1. - sin_t * sin_t).sqrt();
        let inside = thickness / cos_t;
        // The refracted chief ray is cos θt·n + sin θt·(1, −1)/√2, then +x to D.
        let exit = [
            entry + inside * (cos_t + sin_t) * h,
            200. + inside * (cos_t - sin_t) * h,
        ];
        let detector_x = 190.;
        bench.elements = vec![
            plate,
            mount_element("D", DETECTOR, [detector_x, exit[1], 75.], [1., 0., 0.]),
        ];
        bench.connections = [("S1", "out", "P"), ("P", "out", "D")]
            .map(|(from, port, to)| Connection {
                from: from.into(),
                port: port.into(),
                to: to.into(),
            })
            .into();
        let mut req = request(Some("beam"), None, 1);
        req.arrays = true;
        let reading = super::field(&bench, &system, &req).unwrap();
        let air = (entry - bench.sources[0].position[0]) + (detector_x - exit[0]);
        // Paraxial B of the plate: L/n sagittally, (L/n)(cos θi / cos θt)² tangentially.
        let sagittal = air + inside / n;
        let tangential = air + inside / n * (0.5 / (cos_t * cos_t));
        let range = std::f64::consts::PI * w0 * w0 / (wavelength * 1e-6);
        let width = |z: f64| w0 * (1. + (z / range).powi(2)).sqrt();
        // Second-moment 1/e² radii along the basis axes.
        let rows = reading.intensity.as_ref().unwrap();
        let pitch = reading.array_pitch_um.unwrap() * 1e-3;
        let (ny, nx) = (rows.len(), rows[0].len());
        let mut m = [0.; 5];
        for (j, row) in rows.iter().enumerate() {
            for (i, &p) in row.iter().enumerate() {
                let (x, y) = (
                    (i as f64 - nx as f64 / 2.) * pitch,
                    (j as f64 - ny as f64 / 2.) * pitch,
                );
                m = [
                    m[0] + p,
                    m[1] + p * x,
                    m[2] + p * y,
                    m[3] + p * x * x,
                    m[4] + p * y * y,
                ];
            }
        }
        let radius = |s1: f64, s2: f64| 2. * (s2 / m[0] - (s1 / m[0]).powi(2)).sqrt();
        let (ru, rv) = (radius(m[1], m[3]), radius(m[2], m[4]));
        // u = z × d lies in the plane of incidence (table), v = d × u is vertical.
        assert!(
            reading.basis[0][2].abs() < 1e-9 && reading.basis[1][2].abs() > 1. - 1e-9,
            "{:?}",
            reading.basis
        );
        assert!(
            (ru - width(tangential)).abs() < 0.01 * width(tangential),
            "u {ru} vs {} (s {})",
            width(tangential),
            width(sagittal)
        );
        assert!(
            (rv - width(sagittal)).abs() < 0.01 * width(sagittal),
            "v {rv} vs {}",
            width(sagittal)
        );
        assert!(
            (reading.power
                - bench.sources[0].power * reading.arms[0].power / bench.sources[0].power)
                .abs()
                < 1e-3 * reading.power
        );
    }

    #[test]
    fn non_collinear_flux_follows_the_poynting_vector() {
        // Reference normal x; arm 1 along x, arm 2 at 60° in the xy plane.
        let (c, s) = (0.5, 3f64.sqrt() / 2.);
        let n = [1., 0., 0.];
        let real = |v: V3| v.map(|x| C::new(x, 0.));
        for (e1, e2) in [
            // p: both E in the plane of incidence, H along z.
            ([0., 1., 0.], [-s, c, 0.]),
            // s: both E along z.
            ([0., 0., 1.], [0., 0., 1.]),
        ] {
            let w = flux((real(e1), n), (real(e2), [c, s, 0.]), n);
            // Amplitudes 1 and √c (the √cos map) at relative phase φ:
            // I = 1 + c + 2√c·Re(w·e^{iφ}) must equal (1 + c)(1 + cos φ).
            for phi in [0., 1., PI] {
                let got = 1. + c + 2. * c.sqrt() * (w * C::cis(phi)).re;
                let want = (1. + c) * (1. + phi.cos());
                assert!((got - want).abs() < 1e-12, "{e1:?}: {got} vs {want}");
            }
        }
        let crossed = flux((real([0., 1., 0.]), n), (real([0., 0., 1.]), [c, s, 0.]), n);
        assert!(crossed.norm2() < 1e-24);
        // A single arm couples to itself with unit weight.
        let own = flux(
            (real([0., 0., 1.]), [c, s, 0.]),
            (real([0., 0., 1.]), [c, s, 0.]),
            n,
        );
        assert!((own.re - 1.).abs() < 1e-12 && own.im.abs() < 1e-12);
    }

    #[test]
    fn crossing_coherent_beams_form_fringes_at_the_crossing_period() {
        let system = System::default();
        let mut bench = template("mach_zehnder").unwrap();
        bench.assertions.clear();
        let angle = 3f64.to_radians();
        let mut second = bench.sources[0].clone();
        second.id = "S2".into();
        second.direction = [angle.cos(), angle.sin(), 0.];
        second.position = [200. - 5. * angle.cos(), 200. - 5. * angle.sin(), 75.];
        bench.sources[0].position = [195., 200., 75.];
        bench.sources.push(second);
        for source in &mut bench.sources {
            source.coherence = Some(crate::optics::bench::Coherence {
                group: "laser".into(),
                length_mm: None,
            });
            source.field = Some(sampling(64, 2., FieldProfile::Gaussian, 25.));
        }
        let wavelength = bench.sources[0].wavelength * 1e-6;
        bench.elements = vec![mount_element(
            "D",
            DETECTOR,
            [200., 200., 75.],
            [1., 0., 0.],
        )];
        bench.connections = [("S1", "out", "D"), ("S2", "out", "D")]
            .map(|(from, port, to)| Connection {
                from: from.into(),
                port: port.into(),
                to: to.into(),
            })
            .into();
        bench.branches = ["S1", "S2"]
            .map(|s| Branch {
                id: s.into(),
                from: Port::new(s, "out"),
                to: Port::new("D", "in"),
                via: vec![],
                style: None,
            })
            .into();
        let mut req = request(None, Some(Port::new("D", "in")), 1);
        req.arrays = true;
        let reading = super::field(&bench, &system, &req).unwrap();
        let rows = reading.intensity.as_ref().unwrap();
        let pitch = reading.array_pitch_um.unwrap() * 1e-3;
        // Fringes run across u, the table-plane axis, with period λ / sin θ.
        assert!(reading.basis[0][2].abs() < 1e-9);
        let row = &rows[rows.len() / 2];
        let component = |period: f64| {
            let total: f64 = row.iter().sum();
            let sum = row.iter().enumerate().fold(C::ZERO, |s, (i, &p)| {
                s + C::cis(TAU * i as f64 * pitch / period).scale(p)
            });
            2. * sum.norm2().sqrt() / total
        };
        let period = wavelength / angle.sin();
        let visibility = component(period);
        assert!(visibility > 0.95, "{visibility}");
        assert!(component(period * 1.3) < 0.3 && component(period / 1.3) < 0.3);
    }

    #[test]
    fn rejects_phase_ramps_the_grid_would_alias() {
        let system = System::default();
        // λ/p = 13.3 mrad at 40 µm: every sample would get the same phase.
        let bench = fold(
            Some(Action::TipTilt {
                tilt_mrad: [13.3, 0.],
            }),
            100.,
            sampling(64, 40., FieldProfile::Gaussian, 300.),
        );
        let err = super::field(&bench, &system, &request(Some("beam"), None, 1)).unwrap_err();
        assert!(err.contains("would alias"), "{err}");
        // Two apertures each tilting 0.3125 cycles per sample at normal incidence pass
        // alone, but together reach 0.625 > ½ and would read as −4.99 mrad.
        let tilt = 0.532 / 40. * 1e3 * 0.3125;
        let mut bench = fold(None, 1., sampling(64, 40., FieldProfile::Gaussian, 300.));
        let mut first = mount_element("T1", MASK, [101., 200., 75.], [1., 0., 0.]);
        first.action = Some(Action::TipTilt {
            tilt_mrad: [tilt, 0.],
        });
        let mut second = first.clone();
        second.id = "T2".into();
        second.position = [102., 200., 75.];
        bench.sources[0].position = [100., 200., 75.];
        bench.elements = vec![
            first,
            second,
            mount_element("D", DETECTOR, [103., 200., 75.], [1., 0., 0.]),
        ];
        bench.connections = [("S1", "out", "T1"), ("T1", "out", "T2"), ("T2", "out", "D")]
            .map(|(from, port, to)| Connection {
                from: from.into(),
                port: port.into(),
                to: to.into(),
            })
            .into();
        let err = super::field(&bench, &system, &request(Some("beam"), None, 1)).unwrap_err();
        assert!(err.contains("T2 phase ramp"), "{err}");
        // One of them alone is fine and deflects by its own tilt.
        bench.elements.remove(1);
        bench.connections = [("S1", "out", "T1"), ("T1", "out", "D")]
            .map(|(from, port, to)| Connection {
                from: from.into(),
                port: port.into(),
                to: to.into(),
            })
            .into();
        let reading = super::field(&bench, &system, &request(Some("beam"), None, 1)).unwrap();
        assert!(
            (reading.far_field_centroid_mrad[0].abs() - tilt).abs() < 0.05 * tilt,
            "{:?}",
            reading.far_field_centroid_mrad
        );
    }

    #[test]
    fn carrier_check_uses_the_mapped_frequency_of_a_mirrored_arm() {
        let system = System::default();
        let wavelength = 532e-6;
        let theta = (0.3125 * wavelength / 0.04f64).asin();
        let d_out = [theta.cos(), -theta.sin(), 0.];
        let n = unit([-1., 1., 0.]).unwrap();
        let d_in = sub(d_out, scale(n, 2. * dot(d_out, n)));
        let splitter = [190., 200., 75.];
        let tilt_at = sub(splitter, scale(d_in, 20.));
        let run = |sign: f64| {
            let mut bench = template("mach_zehnder").unwrap();
            bench.assertions.clear();
            bench.sources[0].position = [150., 200., 75.];
            let mut second = bench.sources[0].clone();
            second.id = "S2".into();
            second.position = sub(splitter, scale(d_in, 40.));
            second.direction = d_in;
            bench.sources.push(second);
            for source in &mut bench.sources {
                source.wavelength = wavelength * 1e6;
                source.coherence = Some(crate::optics::bench::Coherence {
                    group: "laser".into(),
                    length_mm: None,
                });
                source.field = Some(sampling(64, 40., FieldProfile::Gaussian, 300.));
            }
            let mut tilt = mount_element("T", MASK, tilt_at, d_in);
            // Equal to the crossing angle, so the arm's own frequency is ±q.
            tilt.action = Some(Action::TipTilt {
                tilt_mrad: [sign * theta * 1e3, 0.],
            });
            bench.elements = vec![
                mount_element("BS", SPLITTER, splitter, [-1., 1., 0.]),
                tilt,
                mount_element("D", DETECTOR, [200., 200., 75.], [1., 0., 0.]),
            ];
            bench.connections = [
                ("S1", "out", "BS"),
                ("S2", "out", "T"),
                ("T", "out", "BS"),
                ("BS", "transmitted", "D"),
                ("BS", "reflected", "D"),
            ]
            .map(|(from, port, to)| Connection {
                from: from.into(),
                port: port.into(),
                to: to.into(),
            })
            .into();
            bench.branches = [("S1", "transmitted"), ("S2", "reflected")]
                .map(|(s, port)| Branch {
                    id: s.into(),
                    from: Port::new(s, "out"),
                    to: Port::new("D", "in"),
                    via: vec![Port::new("BS", port)],
                    style: None,
                })
                .into();
            super::field(
                &bench,
                &system,
                &request(None, Some(Port::new("D", "in")), 1),
            )
        };
        let results = [run(1.), run(-1.)];
        // One sign adds the tilt to the carrier (0.625 cycles per sample, rejected),
        // the other cancels it; comparing axes without the mirror flip would pick wrongly.
        let (ok, err): (Vec<_>, Vec<_>) = results.into_iter().partition(Result::is_ok);
        assert_eq!((ok.len(), err.len()), (1, 1), "{err:?}");
        assert!(
            err[0].as_ref().unwrap_err().contains("cycles per sample"),
            "{err:?}"
        );
        let reading = ok.into_iter().next().unwrap().unwrap();
        // The accepted arm lands on the reference axis: both far fields sit at zero.
        assert!(
            reading.far_field_centroid_mrad[0].abs() < 0.2 * theta * 1e3,
            "{:?}",
            reading.far_field_centroid_mrad
        );
    }

    #[test]
    fn resampling_is_band_limited_for_tilted_and_broadband_fields() {
        // Kernel weights at a half-sample offset scale a frequency q by less than
        // one: bilinear keeps cos²(0.3125π) = 0.31 of a tilted beam, Lanczos-3
        // 0.58 of a ±0.375 cycles-per-sample pair.
        let grid = Grid {
            n: [64, 64],
            pitch: 0.04,
        };
        let w = 0.3;
        let envelope = |[x, y]: [f64; 2]| (-(x * x + y * y) / (w * w)).exp();
        let tilted =
            move |p: [f64; 2]| C::cis(TAU * (0.3125 * p[0] - 0.2 * p[1]) / 0.04).scale(envelope(p));
        let broadband = move |p: [f64; 2]| {
            C::new(
                envelope(p) * (1. + (TAU * 0.375 * p[0] / 0.04).cos()) / 2.,
                0.,
            )
        };
        let power = |f: &[C]| f.iter().map(|v| v.norm2()).sum::<f64>();
        let (c, s) = (0.3f64.cos(), 0.3f64.sin());
        let maps: [(M2, [f64; 2]); 5] = [
            (IDENTITY, [0.5, 2.5]),
            ([[-1., 0.], [0., 1.]], [0.5, 0.]),
            ([[0., 1.], [1., 0.]], [0.5, -1.5]),
            ([[c, -s], [s, c]], [0.25, 0.5]),
            // 0.375 × 1.1 stays inside Nyquist with the envelope's spectral tail.
            ([[1.1, 0.], [0., 1.]], [0.5, 0.]),
        ];
        for field in [&tilted as &dyn Fn([f64; 2]) -> C, &broadband] {
            let a: Vec<C> = (0..grid.len()).map(|i| field(grid.coords(i))).collect();
            for (linear, steps) in maps {
                let offset = steps.map(|s| s * grid.pitch);
                let mapped = resample(&a, grid, linear, offset);
                let worst = (0..grid.len())
                    .map(|i| {
                        let p = apply(linear, grid.coords(i));
                        (mapped[i] - field([p[0] + offset[0], p[1] + offset[1]]))
                            .norm2()
                            .sqrt()
                    })
                    .fold(0., f64::max);
                assert!(worst < 1e-3, "{linear:?} {worst}");
                // Area-preserving maps keep the power.
                let det = linear[0][0] * linear[1][1] - linear[0][1] * linear[1][0];
                let ratio = power(&mapped) * det.abs() / power(&a);
                assert!((ratio - 1.).abs() < 1e-3, "{linear:?} {ratio}");
            }
        }
    }

    #[test]
    fn half_sample_offset_arm_keeps_its_power_at_a_shared_port() {
        let system = System::default();
        // 0.3125 cycles per sample on both axes at 40 µm, 532 nm.
        let tilt = 0.532 / 40. * 1e3 * 0.3125;
        let mut bench = fold(None, 1., sampling(64, 40., FieldProfile::Gaussian, 300.));
        bench.sources[0].position = [100., 200., 75.];
        let mut second = bench.sources[0].clone();
        second.id = "S2".into();
        // Half a sample off S1 on both transverse axes.
        second.position = [100., 200.02, 75.02];
        bench.sources.push(second);
        let mut plate = mount_element("T", MASK, [101., 200.02, 75.02], [1., 0., 0.]);
        plate.action = Some(Action::TipTilt {
            tilt_mrad: [tilt, tilt],
        });
        bench.elements = vec![
            plate,
            mount_element("D", DETECTOR, [102., 200., 75.], [1., 0., 0.]),
        ];
        // Both beams cross the plate; S2's field is the one resampled onto S1's grid.
        bench.connections = [("S1", "out", "T"), ("S2", "out", "T"), ("T", "out", "D")]
            .map(|(from, port, to)| Connection {
                from: from.into(),
                port: port.into(),
                to: to.into(),
            })
            .into();
        bench.branches = ["S1", "S2"]
            .map(|s| Branch {
                id: s.into(),
                from: Port::new(s, "out"),
                to: Port::new("D", "in"),
                via: vec![Port::new("T", "out")],
                style: None,
            })
            .into();
        let alone = super::field(&bench, &system, &request(Some("S2"), None, 1)).unwrap();
        let joint = super::field(
            &bench,
            &system,
            &request(None, Some(Port::new("D", "in")), 1),
        )
        .unwrap();
        let mapped = joint.arms.iter().find(|a| a.branch == "S2").unwrap().power;
        assert!(
            (mapped / alone.power - 1.).abs() < 1e-3,
            "{mapped} {}",
            alone.power
        );
    }

    #[test]
    fn ground_glass_speckle_averages_down_and_stays_mutually_coherent() {
        let system = System::default();
        let glass = Action::Rgg {
            correlation_um: 40.,
            roughness_waves: 1.,
            seed: 3,
        };
        let mut bench = fold(None, 20., sampling(64, 20., FieldProfile::Flat, 5000.));
        bench
            .elements
            .insert(0, mount_element("G", MASK, [150., 200., 75.], [1., 0., 0.]));
        bench.elements[0].action = Some(glass.clone());
        bench.elements[1].position = [160., 200., 75.];
        bench.elements[2].position = [160., 220., 75.];
        bench.connections[0].to = "G".into();
        bench.connections.push(Connection {
            from: "G".into(),
            port: "out".into(),
            to: "M".into(),
        });
        let contrast = |runs| {
            super::field(&bench, &system, &request(Some("beam"), None, runs))
                .unwrap()
                .contrast
        };
        let (one, many) = (contrast(1), contrast(16));
        assert!(one > 0.7, "{one}");
        assert!((3. ..5.3).contains(&(one / many)), "{one} {many}");
        let shared = interferometer(
            sampling(32, 40., FieldProfile::Gaussian, 300.),
            Some(glass),
            "D1",
        );
        let reading = super::field(
            &shared,
            &system,
            &request(None, Some(Port::new("D1", "in")), 4),
        )
        .unwrap();
        assert_eq!(reading.realizations, 4);
        assert!(
            reading.pairs[0].correlation > 0.99,
            "{}",
            reading.pairs[0].correlation
        );
    }

    #[test]
    fn mask_and_slm_shape_the_field_without_moving_the_trace() {
        let system = System::default();
        let field = sampling(64, 40., FieldProfile::Gaussian, 500.);
        let read = |action: Option<Action>| {
            let bench = fold(action, 100., field.clone());
            let trace = bench.trace(&system).unwrap();
            let traced = trace.segments[trace.branches[0].paths[0].end_segment.unwrap()].power;
            let mut request = request(Some("beam"), None, 1);
            request.arrays = true;
            (traced, super::field(&bench, &system, &request).unwrap())
        };
        let (t0, plain) = read(None);
        let half = Action::Mask {
            shape: None,
            matrix: Some(vec![vec![1., 0.]]),
            pitch_um: Some(5000.),
            inside: 1.,
            outside: 0.,
        };
        let (t1, masked) = read(Some(half));
        assert_eq!(t0, t1);
        // The centre sample column lies on the mask edge and is blocked: 0.5 − pitch/(2·w·√(π/2)).
        let expected = 0.5 - 0.04 / (2. * 0.5 * (PI / 2.).sqrt());
        assert!(
            (masked.power / plain.power - expected).abs() < 0.005,
            "{} {}",
            masked.power,
            plain.power
        );
        // Opposite pistons on the two SLM halves cancel the axial field.
        let slm = Action::Slm {
            segments: [2, 1],
            size_mm: [10., 10.],
            phases_rad: vec![0., PI],
            tilts_mrad: vec![],
        };
        let (_, split) = read(Some(slm));
        assert!((split.power / plain.power - 1.).abs() < 1e-3);
        let centre = |r: &FieldReading| r.intensity.as_ref().unwrap()[32][32];
        assert!(
            centre(&split) < 0.2 * centre(&plain),
            "{} {}",
            centre(&split),
            centre(&plain)
        );
        assert_eq!(plain.array_pitch_um, Some(40.));
    }

    #[test]
    fn rejects_unsampled_and_ambiguous_requests() {
        let system = System::default();
        let mut bench = fold(None, 100., sampling(64, 40., FieldProfile::Gaussian, 500.));
        assert!(
            super::field(&bench, &system, &request(None, None, 1))
                .unwrap_err()
                .contains("exactly one")
        );
        assert!(
            super::field(&bench, &system, &request(Some("beam"), None, 0))
                .unwrap_err()
                .contains("realizations")
        );
        bench.sources[0].field = None;
        assert!(
            super::field(&bench, &system, &request(Some("beam"), None, 1))
                .unwrap_err()
                .contains("no field sampling")
        );
    }
}
