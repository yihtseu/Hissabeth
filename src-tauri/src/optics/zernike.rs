// SPDX-License-Identifier: GPL-3.0-or-later
// Noll-ordered Zernike decomposition of masked pupil wavefront maps.
use super::analysis::Wavefront;
use serde_json::Value;

pub const MAX_TERMS: usize = 45;

// Noll (1976) indexing: j -> (n, m); theta measured from the pupil x axis.
pub fn noll(j: usize) -> Result<(usize, i32), String> {
    if j == 0 || j > MAX_TERMS {
        return Err(format!("Zernike index must be 1–{MAX_TERMS}"));
    }
    let mut n = 0;
    while (n + 1) * (n + 2) / 2 < j {
        n += 1;
    }
    let k = j - n * (n + 1) / 2 - 1;
    let negative_first = n % 4 == 2 || n % 4 == 3;
    let m = if n % 2 == 0 {
        if k == 0 {
            0
        } else {
            let mag = 2 * k.div_ceil(2) as i32;
            let positive = (k % 2 == 1) != negative_first;
            if positive { mag } else { -mag }
        }
    } else {
        let mag = (2 * (k / 2) + 1) as i32;
        let positive = k.is_multiple_of(2) != negative_first;
        if positive { mag } else { -mag }
    };
    Ok((n, m))
}
fn radial(n: usize, m: i32, r: f64) -> f64 {
    let m = m.unsigned_abs() as usize;
    let factorial = |x: usize| (1..=x).map(|i| i as f64).product::<f64>();
    let mut sum = 0.;
    for k in 0..=(n - m) / 2 {
        let sign = if k % 2 == 0 { 1. } else { -1. };
        let coefficient = sign * factorial(n - k)
            / (factorial(k) * factorial((n + m) / 2 - k) * factorial((n - m) / 2 - k));
        sum += coefficient * r.powi((n - 2 * k) as i32);
    }
    sum
}
pub fn evaluate(j: usize, x: f64, y: f64) -> Result<f64, String> {
    let (n, m) = noll(j)?;
    let r = x.hypot(y);
    if r > 1. + 1e-12 {
        return Err("Zernike evaluation outside the unit pupil".into());
    }
    let radial = radial(n, m, r);
    Ok(if m == 0 {
        (n as f64 + 1.).sqrt() * radial
    } else {
        let theta = y.atan2(x);
        let harmonic = if m > 0 {
            (m as f64 * theta).cos()
        } else {
            (-m as f64 * theta).sin()
        };
        (2. * (n as f64 + 1.)).sqrt() * radial * harmonic
    })
}
fn name(j: usize) -> String {
    match j {
        1 => "piston".into(),
        2 => "tilt x".into(),
        3 => "tilt y".into(),
        4 => "defocus".into(),
        5 => "astigmatism 45".into(),
        6 => "astigmatism 0".into(),
        7 => "coma y".into(),
        8 => "coma x".into(),
        9 => "trefoil y".into(),
        10 => "trefoil x".into(),
        11 => "spherical".into(),
        _ => format!("Z{j}"),
    }
}
// Least squares over the valid wavefront samples; normal equations solved by
// Gaussian elimination with partial pivoting.

#[derive(serde::Serialize, schemars::JsonSchema)]
struct ZernikeTerm {
    j: usize,
    n: usize,
    m: i32,
    name: String,
    coefficient: f64,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
pub(crate) struct ZernikeFit {
    terms: Vec<ZernikeTerm>,
    fit_residual_rms: f64,
    valid: usize,
    grid: usize,
    wavelength: f64,
    units: &'static str,
    ordering: &'static str,
    normalization: &'static str,
}

pub fn fit(wf: &Wavefront, terms: usize) -> Result<Value, String> {
    serde_json::to_value(fit_report(wf, terms)?).map_err(|e| e.to_string())
}
pub(crate) fn fit_report(wf: &Wavefront, terms: usize) -> Result<ZernikeFit, String> {
    if terms == 0 || terms > MAX_TERMS {
        return Err(format!("Zernike terms must be 1–{MAX_TERMS}"));
    }
    let samples: Vec<(f64, f64, f64)> = wf
        .values
        .iter()
        .zip(&wf.pupils)
        .filter_map(|(v, p)| v.map(|v| (p[0], p[1], v)))
        .collect();
    if samples.len() < terms {
        return Err("Fewer valid wavefront samples than Zernike terms".into());
    }
    let basis: Vec<Vec<f64>> = samples
        .iter()
        .map(|&(x, y, _)| {
            (1..=terms)
                .map(|j| evaluate(j, x, y))
                .collect::<Result<Vec<_>, _>>()
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut gram = vec![vec![0.; terms]; terms];
    let mut rhs = vec![0.; terms];
    for (row, (_, _, w)) in basis.iter().zip(&samples) {
        for i in 0..terms {
            rhs[i] += row[i] * w;
            for k in 0..terms {
                gram[i][k] += row[i] * row[k];
            }
        }
    }
    for column in 0..terms {
        let pivot = (column..terms)
            .max_by(|&a, &b| gram[a][column].abs().total_cmp(&gram[b][column].abs()))
            .unwrap();
        if gram[pivot][column].abs() < 1e-14 {
            return Err("Zernike fit is rank deficient; reduce terms".into());
        }
        gram.swap(column, pivot);
        rhs.swap(column, pivot);
        for row in column + 1..terms {
            let factor = gram[row][column] / gram[column][column];
            gram[row][column] = 0.;
            let (pivot_rows, rest) = gram.split_at_mut(row);
            for (x, p) in rest[0][column + 1..terms]
                .iter_mut()
                .zip(&pivot_rows[column][column + 1..terms])
            {
                *x -= factor * p;
            }
            rhs[row] -= factor * rhs[column];
        }
    }
    let mut coefficients = vec![0.; terms];
    for row in (0..terms).rev() {
        let tail: f64 = (row + 1..terms)
            .map(|k| gram[row][k] * coefficients[k])
            .sum();
        coefficients[row] = (rhs[row] - tail) / gram[row][row];
    }
    let mut residual = 0.;
    for (row, (_, _, w)) in basis.iter().zip(&samples) {
        let model: f64 = row.iter().zip(&coefficients).map(|(b, c)| b * c).sum();
        residual += (w - model).powi(2);
    }
    residual = (residual / samples.len() as f64).sqrt();
    let coefficients_by_term: Vec<_> = (1..=terms)
        .map(|j| {
            let (n, m) = noll(j).unwrap();
            ZernikeTerm {
                j,
                n,
                m,
                name: name(j),
                coefficient: coefficients[j - 1],
            }
        })
        .collect();
    Ok(ZernikeFit {
        terms: coefficients_by_term,
        fit_residual_rms: residual,
        valid: samples.len(),
        grid: wf.grid,
        wavelength: wf.wavelength,
        units: "waves",
        ordering: "noll",
        normalization: "unit RMS over the unit disk; theta from pupil x axis; least squares over valid masked samples",
    })
}

// Fringe (University of Arizona) ordering as used by ZEMAX surface sag:
// unnormalized R_n^m(r)·{cos mφ, sin mφ}, per n the m=0 term leads, then
// ascending |m| with the cosine term first. j ≤ 37.
pub fn fringe(j: usize) -> Result<(usize, i32, bool), String> {
    if j == 0 || j > 37 {
        return Err("Fringe Zernike index must be 1–37".into());
    }
    let mut n = 0;
    while (n + 1) * (n + 2) / 2 < j {
        n += 1;
    }
    let k = j - n * (n + 1) / 2;
    if n % 2 == 0 && k == 1 {
        return Ok((n, 0, true));
    }
    let idx = if n % 2 == 0 { k - 2 } else { k - 1 };
    let m = (2 * (idx / 2) + if n % 2 == 0 { 2 } else { 1 }) as i32;
    Ok((n, m, idx % 2 == 0))
}
// Z(u,v) = R_n^m(r)·{cos mφ | sin mφ} evaluated as the polynomial
// Σ_k a_k·(u²+v²)^p·{Re,Im}((u+iv)^m). Returns (z, du, dv, duu, duv, dvv).
pub fn fringe_derivatives(
    n: usize,
    m: i32,
    cos: bool,
    u: f64,
    v: f64,
) -> (f64, f64, f64, f64, f64, f64) {
    let m = m.unsigned_abs() as usize;
    // (P_k, Q_k) = (Re, Im)((u+iv)^k) by recurrence.
    let mut pq = [(1., 0.); 9];
    for k in 1..=m {
        let (pr, qi) = pq[k - 1];
        pq[k] = (u * pr - v * qi, v * pr + u * qi);
    }
    let (pm, qm) = pq[m];
    let (pm1, qm1) = if m >= 1 { pq[m - 1] } else { (0., 0.) };
    let (pm2, qm2) = if m >= 2 { pq[m - 2] } else { (0., 0.) };
    let t = u * u + v * v;
    let (mut z, mut du, mut dv, mut duu, mut duv, mut dvv) = (0., 0., 0., 0., 0., 0.);
    for k in 0..=(n - m) / 2 {
        let sign = if k % 2 == 0 { 1. } else { -1. };
        let factorial = |x: usize| (1..=x).map(|i| i as f64).product::<f64>();
        let a = sign * factorial(n - k)
            / (factorial(k) * factorial((n + m) / 2 - k) * factorial((n - m) / 2 - k));
        let p = (n - m - 2 * k) / 2;
        let tp = t.powi(p as i32);
        let tp1 = if p == 0 { 0. } else { t.powi(p as i32 - 1) };
        let tp2 = if p <= 1 { 0. } else { t.powi(p as i32 - 2) };
        let (m0, m1) = (m as f64, (m as f64 - 1.).max(0.));
        // Harmonic and its derivatives for the cos or sin choice.
        let (h, hu, hv, huu, huv, hvv) = if cos {
            (
                pm,
                m0 * pm1,
                -m0 * qm1,
                m0 * m1 * pm2,
                -m0 * m1 * qm2,
                -m0 * m1 * pm2,
            )
        } else {
            (
                qm,
                m0 * qm1,
                m0 * pm1,
                m0 * m1 * qm2,
                m0 * m1 * pm2,
                -m0 * m1 * qm2,
            )
        };
        let pf = p as f64;
        z += a * tp * h;
        du += a * (2. * pf * u * tp1 * h + tp * hu);
        dv += a * (2. * pf * v * tp1 * h + tp * hv);
        duu += a
            * (2. * pf * tp1 * h
                + 4. * pf * (pf - 1.) * u * u * tp2 * h
                + 4. * pf * u * tp1 * hu
                + tp * huu);
        duv += a
            * (4. * pf * (pf - 1.) * u * v * tp2 * h
                + 2. * pf * u * tp1 * hv
                + 2. * pf * v * tp1 * hu
                + tp * huv);
        dvv += a
            * (2. * pf * tp1 * h
                + 4. * pf * (pf - 1.) * v * v * tp2 * h
                + 4. * pf * v * tp1 * hv
                + tp * hvv);
    }
    (z, du, dv, duu, duv, dvv)
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use super::*;
    #[test]
    fn noll_sequence_matches_standard_table() {
        let table = [
            (0, 0),
            (1, 1),
            (1, -1),
            (2, 0),
            (2, -2),
            (2, 2),
            (3, -1),
            (3, 1),
            (3, -3),
            (3, 3),
            (4, 0),
            (4, 2),
            (4, -2),
            (4, 4),
            (4, -4),
            (5, 1),
            (5, -1),
            (5, 3),
            (5, -3),
            (5, 5),
            (5, -5),
            (6, 0),
            (6, -2),
        ];
        for (j, expected) in table.iter().enumerate() {
            assert_eq!(noll(j + 1).unwrap(), *expected, "j={}", j + 1);
        }
    }
    #[test]
    fn basis_recovers_known_coefficients() {
        let s = System::default();
        let wf = analysis::wavefront(&s, &Field::meridional(0.), s.reference(), 33).unwrap();
        let expected: Vec<f64> = (1..=15).map(|j| 0.05 * j as f64).collect();
        let mut values = vec![];
        for (p, v) in wf.pupils.iter().zip(&wf.values) {
            values.push(v.map(|_| {
                (1..=15)
                    .map(|j| expected[j - 1] * evaluate(j, p[0], p[1]).unwrap())
                    .sum::<f64>()
            }));
        }
        let synthetic = Wavefront {
            values,
            ..analysis::wavefront(&s, &Field::meridional(0.), s.reference(), 33).unwrap()
        };
        let r = fit(&synthetic, 15).unwrap();
        for j in 1..=15 {
            let c = r["terms"][j - 1]["coefficient"].as_f64().unwrap();
            assert!((c - expected[j - 1]).abs() < 1e-8, "j={j}: {c}");
        }
        assert!(r["fit_residual_rms"].as_f64().unwrap() < 1e-10);
    }
    #[test]
    fn defocus_is_captured_by_n2_m0_term() {
        let mut s = System::default();
        let w = s.reference();
        s.surfaces.last_mut().unwrap().thickness += 0.1;
        s.update().unwrap();
        let after = analysis::wavefront(&s, &Field::meridional(0.), w, 33).unwrap();
        let r = fit(&after, 8).unwrap();
        let defocus = r["terms"][3]["coefficient"].as_f64().unwrap();
        let dominant = (1..8)
            .filter(|&j| j != 3)
            .all(|j| r["terms"][j]["coefficient"].as_f64().unwrap().abs() < defocus.abs());
        assert!(defocus.abs() > 0.1 && dominant);
        let mean = after.values.iter().flatten().sum::<f64>() / after.valid as f64;
        let rms = (after
            .values
            .iter()
            .flatten()
            .map(|v| (v - mean).powi(2))
            .sum::<f64>()
            / after.valid as f64)
            .sqrt();
        let residual = r["fit_residual_rms"].as_f64().unwrap();
        assert!(residual < rms);
    }
    #[test]
    fn fringe_sequence_matches_zemax_table() {
        let table = [
            (0, 0, true),
            (1, 1, true),
            (1, 1, false),
            (2, 0, true),
            (2, 2, true),
            (2, 2, false),
            (3, 1, true),
            (3, 1, false),
            (3, 3, true),
            (3, 3, false),
            (4, 0, true),
            (4, 2, true),
            (4, 2, false),
            (4, 4, true),
            (4, 4, false),
            (5, 1, true),
            (5, 1, false),
            (5, 3, true),
            (5, 3, false),
            (5, 5, true),
            (5, 5, false),
            (6, 0, true),
            (6, 2, true),
            (6, 2, false),
            (6, 4, true),
            (6, 4, false),
            (6, 6, true),
            (6, 6, false),
            (7, 1, true),
            (7, 1, false),
            (7, 3, true),
            (7, 3, false),
            (7, 5, true),
            (7, 5, false),
            (7, 7, true),
            (7, 7, false),
            (8, 0, true),
        ];
        for (j, expected) in table.iter().enumerate() {
            assert_eq!(fringe(j + 1).unwrap(), *expected, "j={}", j + 1);
        }
    }
    #[test]
    fn fringe_derivatives_match_finite_differences() {
        for j in [2, 3, 4, 6, 9, 11, 14, 19, 22, 28, 35, 37] {
            let (n, m, cos) = fringe(j).unwrap();
            let at = |u: f64, v: f64| fringe_derivatives(n, m, cos, u, v);
            let (u, v) = (0.31, -0.42);
            let h = 1e-6;
            let (z, du, dv, duu, duv, dvv) = at(u, v);
            assert!(z.is_finite(), "j={j}");
            let checks = [
                ((at(u + h, v).0 - at(u - h, v).0) / (2. * h), du),
                ((at(u, v + h).0 - at(u, v - h).0) / (2. * h), dv),
                ((at(u + h, v).1 - at(u - h, v).1) / (2. * h), duu),
                ((at(u, v + h).1 - at(u, v - h).1) / (2. * h), duv),
                ((at(u, v + h).2 - at(u, v - h).2) / (2. * h), dvv),
            ];
            for (fd, exact) in checks {
                assert!((fd - exact).abs() < 1e-5, "j={j}: {fd} vs {exact}");
            }
        }
    }
}
