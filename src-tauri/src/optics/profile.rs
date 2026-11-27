// SPDX-License-Identifier: GPL-3.0-or-later
// Vertex-referenced profiles and signed line intersections.
use super::zernike::{fringe, fringe_derivatives};
use super::{SagZernike, Surface, Toroid, math::*};
use serde::{Deserialize, Serialize};

#[derive(schemars::JsonSchema, Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EndpointSurface {
    #[schemars(range(min = -1e15, max = 1e15))]
    pub radius: f64,
    #[schemars(range(min = -1e15, max = 1e15))]
    pub conic: f64,
    #[schemars(length(min = 0, max = 16), inner(range(min = -1e15, max = 1e15)))]
    pub even: Vec<f64>,
    #[schemars(length(min = 0, max = 20), inner(range(min = -1e15, max = 1e15)))]
    pub odd: Vec<f64>,
    #[schemars(extend("exclusiveMinimum" = 0.0))]
    pub aperture: f64,
    pub clear_aperture: bool,
    #[schemars(range(min = 0.0, max = 1e15))]
    pub semi_diameter: f64,
}
impl EndpointSurface {
    pub fn plane() -> Self {
        Self {
            radius: 0.,
            conic: 0.,
            even: vec![],
            odd: vec![],
            aperture: 1.,
            clear_aperture: false,
            semi_diameter: 0.,
        }
    }
    pub fn profile(&self) -> Profile<'_> {
        Profile {
            radius: self.radius,
            conic: self.conic,
            even: &self.even,
            odd: &self.odd,
            toroid: None,
            zernike: None,
        }
    }
    pub fn sag(&self, r: f64) -> Result<(f64, f64), String> {
        self.profile().sag(r)
    }
    pub fn validate(&self) -> Result<(), String> {
        self.profile().validate()?;
        if !self.aperture.is_finite()
            || self.aperture <= 0.
            || !self.semi_diameter.is_finite()
            || self.semi_diameter < 0.
        {
            return Err("Invalid endpoint aperture or drawing radius".into());
        }
        if self.clear_aperture {
            self.sag(self.aperture)?;
        }
        Ok(())
    }
}
impl Surface {
    pub fn profile(&self) -> Profile<'_> {
        Profile {
            radius: self.radius,
            conic: self.conic,
            even: &self.even,
            odd: &self.odd,
            toroid: self.toroid.as_ref(),
            zernike: self.zernike.as_ref(),
        }
    }
}
#[derive(Clone, Copy)]
pub struct Profile<'a> {
    pub radius: f64,
    pub conic: f64,
    pub even: &'a [f64],
    pub odd: &'a [f64],
    pub toroid: Option<&'a Toroid>,
    pub zernike: Option<&'a SagZernike>,
}
pub struct Intersection {
    pub point: V3,
    pub normal: V3,
    pub distance: f64,
}
impl Profile<'_> {
    pub fn is_plane(self) -> bool {
        self.radius == 0.
            && self.even.iter().chain(self.odd).all(|a| *a == 0.)
            && self.toroid.map_or(true, |t| {
                t.revolution == 0. && t.even_y.iter().all(|a| *a == 0.)
            })
            && self
                .zernike
                .map_or(true, |z| z.coefficients.iter().all(|a| *a == 0.))
    }
    /// Rotationally symmetric profiles keep the fast radial path. A toroid or
    /// any Fringe term with m != 0 breaks the symmetry.
    pub fn is_radial(self) -> bool {
        self.toroid.is_none()
            && self.zernike.map_or(true, |z| {
                z.coefficients
                    .iter()
                    .enumerate()
                    .all(|(i, a)| *a == 0. || fringe(i + 1).is_ok_and(|(_, m, _)| m == 0))
            })
    }
    pub fn validate(self) -> Result<(), String> {
        if !self.radius.is_finite()
            || !self.conic.is_finite()
            || self.even.len() > 16
            || self.odd.len() > 20
            || !self.even.iter().chain(self.odd).all(|a| a.is_finite())
        {
            return Err("Invalid radial profile parameters".into());
        }
        if let Some(t) = self.toroid {
            if !t.revolution.is_finite()
                || t.even_y.len() > 8
                || !t.even_y.iter().all(|a| a.is_finite())
            {
                return Err("Invalid toroidal parameters".into());
            }
            if !self.even.is_empty() || !self.odd.is_empty() {
                return Err("Toroidal profiles take no radial asphere coefficients".into());
            }
        }
        if let Some(z) = self.zernike {
            if !z.normalize.is_finite()
                || z.normalize <= 0.
                || z.coefficients.len() > 37
                || !z.coefficients.iter().all(|a| a.is_finite())
            {
                return Err("Invalid Zernike sag parameters".into());
            }
        }
        Ok(())
    }
    /// Sag, first, and second radial derivative of the rotationally symmetric
    /// base (conic + radial polynomials).
    fn radial_sag2(self, r: f64) -> Result<(f64, f64, f64), String> {
        if !r.is_finite() || r < 0. {
            return Err("Invalid radial height".into());
        }
        let c = if self.radius == 0. {
            0.
        } else {
            1. / self.radius
        };
        let q = 1. - (1. + self.conic) * c * c * r * r;
        if q < 0. {
            return Err("Surface sag outside real domain".into());
        }
        let sq = q.sqrt();
        let (mut z, mut dz, mut d2z) = (c * r * r / (1. + sq), c * r / sq, c / (sq * q));
        for (i, a) in self.even.iter().enumerate() {
            let power = 4 + 2 * i as i32;
            z += a * r.powi(power);
            dz += a * power as f64 * r.powi(power - 1);
            d2z += a * (power * (power - 1)) as f64 * r.powi(power - 2);
        }
        // Geopter's odd profile includes every radial power beginning with r^3.
        for (i, a) in self.odd.iter().enumerate() {
            let power = 3 + i as i32;
            z += a * r.powi(power);
            dz += a * power as f64 * r.powi(power - 1);
            d2z += a * (power * (power - 1)) as f64 * r.powi(power - 2);
        }
        if z.is_finite() && (dz.is_finite() || (q == 0. && !dz.is_nan())) {
            Ok((z, dz, d2z))
        } else {
            Err("Nonfinite surface sag".into())
        }
    }
    /// Toroidal sag and gradient: the YZ conic profile swept about an axis
    /// parallel to Y at distance `revolution`; the X section is a circle of
    /// radius revolution + z_y(y). revolution == 0 gives a flat X section.
    fn toroid_sag(self, t: &Toroid, x: f64, y: f64) -> Result<(f64, f64, f64), String> {
        let c = if self.radius == 0. {
            0.
        } else {
            1. / self.radius
        };
        let q = 1. - (1. + self.conic) * c * c * y * y;
        if q < 0. {
            return Err("Surface sag outside real domain".into());
        }
        let sq = q.sqrt();
        let (mut zy, mut dy) = (c * y * y / (1. + sq), c * y / sq);
        for (i, a) in t.even_y.iter().enumerate() {
            let power = 4 + 2 * i as i32;
            zy += a * y.powi(power);
            dy += a * power as f64 * y.powi(power - 1);
        }
        if t.revolution == 0. {
            return Ok((zy, 0., dy));
        }
        let ring = t.revolution + zy;
        if ring == 0. {
            return Err("Degenerate toroidal revolution radius".into());
        }
        let a = 1. / ring;
        let s = 1. - a * a * x * x;
        if s < 0. {
            return Err("Surface sag outside real domain".into());
        }
        let sqx = s.sqrt();
        let z = zy + a * x * x / (1. + sqx);
        let gx = a * x / sqx;
        // dA/dy = -A^2·z_y', and dh/dA = x²/(s(1+s)).
        let gy = dy - a * a * dy * x * x / (sqx * (1. + sqx));
        Ok((z, gx, gy))
    }
    /// Sag and gradient (dz/dx, dz/dy) at an arbitrary aperture point.
    pub fn sag2(self, x: f64, y: f64) -> Result<(f64, f64, f64), String> {
        if !x.is_finite() || !y.is_finite() {
            return Err("Invalid aperture coordinates".into());
        }
        let (mut z, mut gx, mut gy) = match self.toroid {
            Some(t) => self.toroid_sag(t, x, y)?,
            None => {
                let r = x.hypot(y);
                let (z, dz, _) = self.radial_sag2(r)?;
                let (gx, gy) = if r > 1e-15 {
                    (dz * x / r, dz * y / r)
                } else {
                    (0., 0.)
                };
                (z, gx, gy)
            }
        };
        if let Some(zn) = self.zernike {
            let inv = 1. / zn.normalize;
            for (i, c) in zn.coefficients.iter().enumerate() {
                if *c == 0. {
                    continue;
                }
                let (n, m, cos) = fringe(i + 1)?;
                let (zz, du, dv, _, _, _) = fringe_derivatives(n, m, cos, x * inv, y * inv);
                z += c * zz;
                gx += c * du * inv;
                gy += c * dv * inv;
            }
        }
        if z.is_finite() && gx.is_finite() && gy.is_finite() {
            Ok((z, gx, gy))
        } else {
            Err("Nonfinite surface sag".into())
        }
    }
    /// Hessian (d²z/dx², d²z/dxdy, d²z/dy²) at an arbitrary aperture point.
    pub fn hessian2(self, x: f64, y: f64) -> Result<(f64, f64, f64), String> {
        if !x.is_finite() || !y.is_finite() {
            return Err("Invalid aperture coordinates".into());
        }
        let (mut hxx, mut hxy, mut hyy) = match self.toroid {
            Some(t) => {
                let c = if self.radius == 0. {
                    0.
                } else {
                    1. / self.radius
                };
                let q = 1. - (1. + self.conic) * c * c * y * y;
                if q < 0. {
                    return Err("Surface sag outside real domain".into());
                }
                let sq = q.sqrt();
                let (mut zy, mut dy, mut d2y) = (c * y * y / (1. + sq), c * y / sq, c / (sq * q));
                for (i, a) in t.even_y.iter().enumerate() {
                    let power = 4 + 2 * i as i32;
                    zy += a * y.powi(power);
                    dy += a * power as f64 * y.powi(power - 1);
                    d2y += a * (power * (power - 1)) as f64 * y.powi(power - 2);
                }
                if t.revolution == 0. {
                    (0., 0., d2y)
                } else {
                    let ring = t.revolution + zy;
                    if ring == 0. {
                        return Err("Degenerate toroidal revolution radius".into());
                    }
                    let a = 1. / ring;
                    let s = 1. - a * a * x * x;
                    if s <= 0. {
                        return Err("Surface sag outside real domain".into());
                    }
                    let sqx = s.sqrt();
                    let ap = -a * a * dy;
                    let app = 2. * a * a * a * dy * dy - a * a * d2y;
                    let h_a = x * x / (sqx * (1. + sqx));
                    let h_aa = a * x.powi(4) * (1. + 2. * sqx) / (sqx * s * (1. + sqx).powi(2));
                    (
                        a / (sqx * s),
                        x * ap / (sqx * s),
                        d2y + h_aa * ap * ap + h_a * app,
                    )
                }
            }
            None => {
                let r = x.hypot(y);
                let (_, dz, d2z) = self.radial_sag2(r)?;
                if r > 1e-15 {
                    let (ux, uy) = (x / r, y / r);
                    let transverse = dz / r;
                    (
                        transverse + (d2z - transverse) * ux * ux,
                        (d2z - transverse) * ux * uy,
                        transverse + (d2z - transverse) * uy * uy,
                    )
                } else {
                    let c = if self.radius == 0. {
                        0.
                    } else {
                        1. / self.radius
                    };
                    (c, 0., c)
                }
            }
        };
        if let Some(zn) = self.zernike {
            let inv = 1. / zn.normalize;
            for (i, c) in zn.coefficients.iter().enumerate() {
                if *c == 0. {
                    continue;
                }
                let (n, m, cos) = fringe(i + 1)?;
                let (_, _, _, duu, duv, dvv) = fringe_derivatives(n, m, cos, x * inv, y * inv);
                hxx += c * duu * inv * inv;
                hxy += c * duv * inv * inv;
                hyy += c * dvv * inv * inv;
            }
        }
        if hxx.is_finite() && hxy.is_finite() && hyy.is_finite() {
            Ok((hxx, hxy, hyy))
        } else {
            Err("Nonfinite surface curvature".into())
        }
    }
    pub fn sag(self, r: f64) -> Result<(f64, f64), String> {
        if !self.is_radial() {
            return Err("Profile is not rotationally symmetric".into());
        }
        if self.is_plane() {
            return Ok((0., 0.));
        }
        let (mut z, mut dz, _) = self.radial_sag2(r)?;
        if let Some(zn) = self.zernike {
            let inv = 1. / zn.normalize;
            for (i, c) in zn.coefficients.iter().enumerate() {
                if *c == 0. {
                    continue;
                }
                // is_radial guarantees only m = 0 terms are present.
                let (n, m, cos) = fringe(i + 1)?;
                let (zz, du, _, _, _, _) = fringe_derivatives(n, m, cos, r * inv, 0.);
                z += c * zz;
                dz += c * du * inv;
            }
        }
        if z.is_finite() && !dz.is_nan() {
            Ok((z, dz))
        } else {
            Err("Nonfinite surface sag".into())
        }
    }
    pub(super) fn normal_variation(self, point: V3, variation: V3) -> Result<V3, String> {
        // The closed radial Jacobian below ignores Zernike terms; any nonzero
        // Zernike coefficient (even m = 0) takes the generic 2D path.
        if !self.is_radial()
            || self
                .zernike
                .is_some_and(|z| z.coefficients.iter().any(|a| *a != 0.))
        {
            let (_, fx, fy) = self.sag2(point[0], point[1])?;
            let (hxx, hxy, hyy) = self.hessian2(point[0], point[1])?;
            let g = [-fx, -fy, 1.];
            let dg = [
                -(hxx * variation[0] + hxy * variation[1]),
                -(hxy * variation[0] + hyy * variation[1]),
                0.,
            ];
            let normal = unit(g)?;
            let dn = scale(sub(dg, scale(normal, dot(normal, dg))), 1. / norm(g));
            if dn.iter().all(|v| v.is_finite()) {
                return Ok(dn);
            }
            return Err("Nonfinite surface-normal derivative".into());
        }
        let r = point[0].hypot(point[1]);
        let mut polynomial = 0.;
        let mut first = 0.;
        let mut second = 0.;
        for (power, a) in self
            .even
            .iter()
            .enumerate()
            .map(|(i, a)| (4 + 2 * i as i32, *a))
            .chain(self.odd.iter().enumerate().map(|(i, a)| (3 + i as i32, *a)))
        {
            polynomial += a * r.powi(power);
            first += a * power as f64 * r.powi(power - 1);
            second += a * (power * (power - 1)) as f64 * r.powi(power - 2);
        }
        let radial = if r > 0. {
            [point[0] / r, point[1] / r]
        } else {
            [0., 0.]
        };
        let gradient = radial.map(|v| first * v);
        let transverse = if r > 0. { first / r } else { 0. };
        let radial_change = radial[0] * variation[0] + radial[1] * variation[1];
        let hessian_product = [0, 1].map(|axis| {
            transverse * variation[axis] + (second - transverse) * radial[axis] * radial_change
        });
        let c = if self.radius == 0. {
            0.
        } else {
            1. / self.radius
        };
        let a = 1. - (1. + self.conic) * c * (point[2] - polynomial);
        let da = -(1. + self.conic)
            * c
            * (variation[2] - gradient[0] * variation[0] - gradient[1] * variation[1]);
        // The implicit conic with z replaced by z-P(r) stays regular at a vertical rim.
        let g = [
            -c * point[0] - a * gradient[0],
            -c * point[1] - a * gradient[1],
            a,
        ];
        let dg = [
            -c * variation[0] - da * gradient[0] - a * hessian_product[0],
            -c * variation[1] - da * gradient[1] - a * hessian_product[1],
            da,
        ];
        let normal = unit(g)?;
        let dn = scale(sub(dg, scale(normal, dot(normal, dg))), 1. / norm(g));
        if dn.iter().all(|v| v.is_finite()) {
            Ok(dn)
        } else {
            Err("Nonfinite surface-normal derivative".into())
        }
    }
    fn conic_roots(self, p: V3, v: V3) -> Vec<f64> {
        let curvature = if self.radius == 0. {
            0.
        } else {
            1. / self.radius
        };
        let a = curvature * (v[0] * v[0] + v[1] * v[1] + (1. + self.conic) * v[2] * v[2]);
        let b = curvature * (p[0] * v[0] + p[1] * v[1] + (1. + self.conic) * p[2] * v[2]) - v[2];
        let c =
            curvature * (p[0] * p[0] + p[1] * p[1] + (1. + self.conic) * p[2] * p[2]) - 2. * p[2];
        if a == 0. {
            return if b != 0. { vec![-c / (2. * b)] } else { vec![] };
        }
        let discriminant = b * b - a * c;
        if !discriminant.is_finite() || discriminant < 0. {
            return vec![];
        }
        let q = -b - discriminant.sqrt().copysign(b);
        if q == 0. {
            vec![-b / a]
        } else {
            vec![q / a, c / q]
        }
    }
    fn residual(self, p: V3, v: V3, t: f64) -> Result<(V3, f64, f64), String> {
        if !t.is_finite() {
            return Err("Nonfinite intersection distance".into());
        }
        let hit = add(p, scale(v, t));
        if !self.is_radial() {
            let (z, fx, fy) = self.sag2(hit[0], hit[1])?;
            let derivative = v[2] - (fx * v[0] + fy * v[1]);
            return Ok((hit, hit[2] - z, derivative));
        }
        let radius = hit[0].hypot(hit[1]);
        let (z, slope) = self.sag(radius)?;
        let derivative = v[2]
            - if radius > 1e-15 {
                slope * (hit[0] * v[0] + hit[1] * v[1]) / radius
            } else {
                0.
            };
        Ok((hit, hit[2] - z, derivative))
    }
    fn refine(self, p: V3, v: V3, mut t: f64) -> Option<f64> {
        for _ in 0..50 {
            let (_, error, derivative) = self.residual(p, v, t).ok()?;
            if error.abs() < 1e-10 {
                return Some(t);
            }
            if !derivative.is_finite() || derivative.abs() < 1e-14 {
                return None;
            }
            let step = error / derivative;
            let mut fraction = 1.;
            let mut next = None;
            for _ in 0..24 {
                let candidate = t - fraction * step;
                if let Ok((_, next_error, _)) = self.residual(p, v, candidate) {
                    if next_error.abs() < error.abs() {
                        next = Some(candidate);
                        break;
                    }
                }
                fraction *= 0.5;
            }
            t = next?;
        }
        None
    }
    pub fn intersect(self, p: V3, v: V3, vertex: f64) -> Result<Intersection, String> {
        let relative = [p[0], p[1], p[2] - vertex];
        if self.is_plane() {
            if v[2].abs() < 1e-14 {
                return Err("Ray parallel to surface plane".into());
            }
            let distance = -relative[2] / v[2];
            let point = [p[0] + distance * v[0], p[1] + distance * v[1], vertex];
            if !distance.is_finite() || !point.iter().all(|x| x.is_finite()) {
                return Err("Nonfinite surface intersection".into());
            }
            return Ok(Intersection {
                point,
                normal: [0., 0., 1.],
                distance,
            });
        }
        // Work near the axial vertex, following Geopter's perpendicular-foot
        // construction, so a distant source does not square its axial gap.
        let origin_distance = -dot(relative, v) / dot(v, v);
        let relative = add(relative, scale(v, origin_distance));
        let plane = if v[2].abs() > 1e-14 {
            -relative[2] / v[2]
        } else {
            0.
        };
        let mut candidates = if self.is_radial() {
            self.conic_roots(relative, v)
        } else {
            vec![]
        };
        let polished = !self.is_radial()
            || self.even.iter().chain(self.odd).any(|a| *a != 0.)
            || self
                .zernike
                .is_some_and(|z| z.coefficients.iter().any(|a| *a != 0.));
        if polished {
            candidates.push(plane);
            candidates.push(0.);
            candidates = candidates
                .into_iter()
                .filter_map(|t| self.refine(relative, v, t))
                .collect();
        }
        // A sequential surface selects its vertex sheet. The prescribed gap may
        // place that sheet at a negative signed propagation distance.
        let distance = candidates
            .into_iter()
            .filter(|t| {
                self.residual(relative, v, *t)
                    .is_ok_and(|(hit, error, _)| error.abs() <= 1e-8 * (1. + hit[2].abs()))
            })
            .min_by(|a, b| (a - plane).abs().total_cmp(&(b - plane).abs()))
            .ok_or("Ray misses surface profile")?;
        let local = add(relative, scale(v, distance));
        let normal = if !self.is_radial() {
            let (_, fx, fy) = self.sag2(local[0], local[1])?;
            unit([-fx, -fy, 1.])?
        } else {
            self.radial_normal(local)?
        };
        let point = [local[0], local[1], local[2] + vertex];
        if !point.iter().all(|x| x.is_finite()) {
            return Err("Nonfinite surface intersection".into());
        }
        Ok(Intersection {
            point,
            normal,
            distance: origin_distance + distance,
        })
    }
    fn radial_normal(self, local: V3) -> Result<V3, String> {
        let radial = local[0].hypot(local[1]);
        let (_, slope) = self.sag(radial)?;
        if slope.is_infinite() {
            let c = 1. / self.radius;
            unit([-c * local[0], -c * local[1], 0.])
        } else {
            unit(if radial > 1e-15 {
                [-slope * local[0] / radial, -slope * local[1] / radial, 1.]
            } else {
                [0., 0., 1.]
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::optics::{Field, FieldType, Interaction, PupilMapping, System, Wavelength};

    fn toroidal_surface() -> Surface {
        let mut s = Surface::sphere(40., 10., "AIR", 8.);
        s.conic = -0.5;
        s.toroid = Some(Toroid {
            revolution: 30.,
            even_y: vec![1e-4],
        });
        s
    }
    fn closed_form_toroid_z(x: f64, y: f64) -> f64 {
        let c = 1. / 40.;
        let zy = c * y * y / (1. + (1. - 0.5 * c * c * y * y).sqrt()) + 1e-4 * y.powi(4);
        let ring = 30. + zy;
        zy + x * x / ring / (1. + (1. - x * x / (ring * ring)).sqrt())
    }
    #[test]
    fn toroidal_sag_gradient_and_hessian_match_closed_form() {
        let s = toroidal_surface();
        let p = s.profile();
        assert!(!p.is_radial());
        assert!(p.sag(1.).unwrap_err().contains("rotationally symmetric"));
        for (x, y) in [(1.3, -2.1), (0., 3.2), (-2.5, 0.4)] {
            let (z, gx, gy) = p.sag2(x, y).unwrap();
            assert!((z - closed_form_toroid_z(x, y)).abs() < 1e-14, "{x} {y}");
            let h = 1e-6;
            let at = |x, y| p.sag2(x, y).unwrap().0;
            assert!(((at(x + h, y) - at(x - h, y)) / (2. * h) - gx).abs() < 1e-8);
            assert!(((at(x, y + h) - at(x, y - h)) / (2. * h) - gy).abs() < 1e-8);
            let (hxx, hxy, hyy) = p.hessian2(x, y).unwrap();
            let g = |x, y| p.sag2(x, y).unwrap();
            assert!(((g(x + h, y).1 - g(x - h, y).1) / (2. * h) - hxx).abs() < 1e-6);
            assert!(((g(x, y + h).1 - g(x, y - h).1) / (2. * h) - hxy).abs() < 1e-6);
            assert!(((g(x, y + h).2 - g(x, y - h).2) / (2. * h) - hyy).abs() < 1e-6);
        }
    }
    #[test]
    fn toroidal_intersection_recovers_surface_point_and_normal_from_both_sides() {
        let s = toroidal_surface();
        let p = s.profile();
        let (x, y) = (1.3, -2.1);
        let (z, gx, gy) = p.sag2(x, y).unwrap();
        let expected = [x, y, 7. + z];
        let normal = unit([-gx, -gy, 1.]).unwrap();
        let direction = unit([0.17, -0.09, 1.]).unwrap();
        for distance in [-8., 8.] {
            let origin = sub(expected, scale(direction, distance));
            let hit = p.intersect(origin, direction, 7.).unwrap();
            assert!((hit.distance - distance).abs() < 2e-9);
            assert!(norm(sub(hit.point, expected)) < 2e-9);
            assert!(norm(sub(hit.normal, normal)) < 2e-9);
        }
    }
    #[test]
    fn cylinder_toroid_focuses_only_the_powered_section() {
        let mut s = System::default();
        let mut cyl = Surface::sphere(40., 1., "N-BK7_SCHOTT", 5.);
        cyl.toroid = Some(Toroid {
            revolution: 0.,
            even_y: vec![],
        });
        let n = crate::optics::index("N-BK7_SCHOTT", 587.5618).unwrap();
        cyl.thickness = n * 40. / (n - 1.);
        s.surfaces = vec![cyl];
        s.stop_index = 0;
        s.fields = vec![Field::meridional(0.)];
        s.update().unwrap();
        // Flat X section: an X fan ray stays at its height.
        let x_ray = s.trace_field(1., 0., &s.fields[0], s.reference()).unwrap();
        assert!((x_ray.image()[0] - 1.).abs() < 1e-12);
        assert!(x_ray.image()[1].abs() < 1e-12);
        // Powered Y section: a paraxial Y ray crosses the axis at nR/(n-1).
        let y_ray = s
            .trace_field(0., 1e-3, &s.fields[0], s.reference())
            .unwrap();
        assert!(y_ray.image()[1].abs() < 1e-9);
        assert!(y_ray.image()[0].abs() < 1e-12);
    }
    #[test]
    fn radial_zernike_surface_traces_onto_the_deformed_profile() {
        // m = 0 Zernike terms keep the radial path; the conic seed alone would
        // miss the Z4 deformation, so the intersection must be refined.
        let mut s = System::default();
        s.surfaces[1].zernike = Some(SagZernike {
            normalize: 12.5,
            coefficients: vec![0., 0., 0., 5e-4],
        });
        s.update().unwrap();
        let ray = s.trace_field(1., -2., &s.fields[0], s.reference()).unwrap();
        let hit = ray.segments[1].point;
        let r = hit[0].hypot(hit[1]);
        let base = -50. + (2500. - r * r).sqrt();
        let u = r / 12.5;
        let expected = 5. + base + 5e-4 * (2. * u * u - 1.);
        assert!((hit[2] - expected).abs() < 1e-9, "{} vs {expected}", hit[2]);
    }
    #[test]
    fn zernike_sag_adds_fringe_terms_and_guards_the_radial_api() {
        let mut s = Surface::sphere(0., 1., "AIR", 8.);
        s.zernike = Some(SagZernike {
            normalize: 10.,
            coefficients: vec![0.2, 0.5],
        });
        let p = s.profile();
        assert!(!p.is_radial());
        assert!(p.sag(1.).unwrap_err().contains("rotationally symmetric"));
        // Z1 = 1 (piston), Z2 = u (x tilt): z = 0.2 + 0.05·x.
        let (z, gx, gy) = p.sag2(3., -2.).unwrap();
        assert!((z - (0.2 + 0.05 * 3.)).abs() < 1e-15);
        assert!((gx - 0.05).abs() < 1e-15);
        assert!(gy.abs() < 1e-15);
        // Only m = 0 terms keep the profile radial and the radial API working.
        s.zernike = Some(SagZernike {
            normalize: 10.,
            coefficients: vec![0., 0., 0., 1e-3],
        });
        let p = s.profile();
        assert!(p.is_radial());
        let (z, dz) = p.sag(5.).unwrap();
        let u = 0.5_f64;
        assert!((z - 1e-3 * (2. * u * u - 1.)).abs() < 1e-15);
        assert!((dz - 1e-3 * 4. * u / 10.).abs() < 1e-15);
        let (z2, _, _) = p.sag2(3., 4.).unwrap();
        assert!((z - z2).abs() < 1e-15);
    }
    #[test]
    fn non_radial_normal_variation_matches_perturbed_normal() {
        let mut s = Surface::sphere(50., 1., "AIR", 8.);
        s.zernike = Some(SagZernike {
            normalize: 6.,
            coefficients: vec![0., 0., 0., 0., 1e-3, -2e-3, 0., 0., 0., 0., 5e-4],
        });
        let p = s.profile();
        let (x, y) = (1.7, -1.1);
        let (z, gx, gy) = p.sag2(x, y).unwrap();
        let step = 1e-5;
        for v in [[step, 0., 0.], [0., step, 0.]] {
            let (zn, fx, fy) = p.sag2(x + v[0], y + v[1]).unwrap();
            let actual = unit([-fx, -fy, 1.]).unwrap();
            let dn = p.normal_variation([x, y, z], [v[0], v[1], zn - z]).unwrap();
            let predicted = add(unit([-gx, -gy, 1.]).unwrap(), dn);
            assert!(norm(sub(actual, predicted)) < 1e-6);
        }
    }
    #[test]
    fn toroid_rejects_radial_asphere_coefficients_on_validate() {
        let mut s = toroidal_surface();
        s.even = vec![1e-5];
        assert!(s.profile().validate().unwrap_err().contains("Toroidal"));
    }

    #[test]
    fn signed_sphere_conic_even_and_odd_intersections_match_closed_form_points() {
        let (x, y) = (1.2_f64, -0.8_f64);
        let r = x.hypot(y);
        let even = [0.001, -0.00003];
        let odd = [0.002, -0.0001];
        let mut p = EndpointSurface::plane();
        for kind in 0..5 {
            p.radius = if kind == 1 { -40. } else { 40. };
            p.conic = if kind < 2 { 0. } else { -1. };
            p.even = if kind == 3 { even.to_vec() } else { vec![] };
            p.odd = if kind == 4 { odd.to_vec() } else { vec![] };
            let (z, slope) = match kind {
                0 => (40. - (1600. - r * r).sqrt(), r / (1600. - r * r).sqrt()),
                1 => (-40. + (1600. - r * r).sqrt(), -r / (1600. - r * r).sqrt()),
                2 => (r * r / 80., r / 40.),
                3 => (
                    r * r / 80. + 0.001 * r.powi(4) - 0.00003 * r.powi(6),
                    r / 40. + 0.004 * r.powi(3) - 0.00018 * r.powi(5),
                ),
                _ => (
                    r * r / 80. + 0.002 * r.powi(3) - 0.0001 * r.powi(4),
                    r / 40. + 0.006 * r * r - 0.0004 * r.powi(3),
                ),
            };
            let expected = [x, y, 7. + z];
            let normal = unit([-slope * x / r, -slope * y / r, 1.]).unwrap();
            let direction = unit([0.17, -0.09, 1.]).unwrap();
            for distance in [-8., 8.] {
                let origin = sub(expected, scale(direction, distance));
                let hit = p.profile().intersect(origin, direction, 7.).unwrap();
                assert!((hit.distance - distance).abs() < 2e-9, "kind {kind}");
                assert!(norm(sub(hit.point, expected)) < 2e-9, "kind {kind}");
                assert!(norm(sub(hit.normal, normal)) < 2e-9, "kind {kind}");
            }
        }
    }
    #[test]
    fn sphere_intersection_reaches_vertex_sheet_when_vertex_plane_point_is_outside_domain() {
        let mut p = EndpointSurface::plane();
        p.radius = 5.;
        let direction = unit([-2., 0., 1.]).unwrap();
        let expected = [4., 0., 2.];
        let origin = sub(expected, scale(direction, 3.));
        let plane_x = origin[0] - origin[2] * direction[0] / direction[2];
        assert!(plane_x > 5.);
        let hit = p.profile().intersect(origin, direction, 0.).unwrap();
        assert!(norm(sub(hit.point, expected)) < 1e-12);
        assert!((hit.distance - 3.).abs() < 1e-12);
        p.radius = 1.;
        let tangent = p
            .profile()
            .intersect([1., 0., -1.], [0., 0., 1.], 0.)
            .unwrap();
        assert_eq!(tangent.point, [1., 0., 1.]);
        assert_eq!(tangent.normal, [-1., 0., 0.]);
        assert!((tangent.distance - 2.).abs() < 1e-14);
        assert!(
            p.profile()
                .intersect([1.1, 0., -1.], [0., 0., 1.], 0.)
                .is_err()
        );
    }
    fn uniform_source() -> System {
        let mut s = System::default();
        s.object_material = "1.500000:60.000000".into();
        s.object_distance = Some(100.);
        s.object_surface.radius = 50.;
        s.wavelengths = vec![Wavelength::new(587.562, 1.)];
        s.reference_wavelength = 0;
        let mut plane = Surface::sphere(0., 20., "AIR", 10.);
        plane.interaction = Interaction::Dummy;
        s.surfaces = vec![plane];
        s.stop_index = 0;
        s.pupil.value = 2.;
        s.field_type = FieldType::ObjectHeight;
        s.fields = vec![Field {
            x: 1.,
            ..Field::meridional(2.)
        }];
        s
    }
    #[test]
    fn curved_emitter_and_signed_spherical_detector_follow_straight_line_optical_path() {
        for radius in [-40., 40.] {
            for distance in [-20., 20.] {
                let mut s = uniform_source();
                s.image_surface.radius = radius;
                s.surfaces[0].thickness = distance;
                s.update().unwrap();
                let ray = s
                    .trace_field(0.25, -0.1, &s.fields[0], s.reference())
                    .unwrap();
                let source = [1., 2., -100. + 50. - 2495_f64.sqrt()];
                assert!(norm(sub(ray.object_point.unwrap(), source)) < 1e-12);
                let first = [0.25, -0.1, 0.];
                let direction = unit(sub(first, source)).unwrap();
                let center = [0., 0., distance + radius];
                let delta = sub(first, center);
                let b = dot(delta, direction);
                let discriminant = b * b - (dot(delta, delta) - radius * radius);
                let candidates = [-b - discriminant.sqrt(), -b + discriminant.sqrt()];
                let t = *candidates
                    .iter()
                    .min_by(|a, b| {
                        (**a - distance / direction[2])
                            .abs()
                            .total_cmp(&(**b - distance / direction[2]).abs())
                    })
                    .unwrap();
                let expected = add(first, scale(direction, t));
                assert!(norm(sub(ray.image(), expected)) < 1e-10);
                assert!(
                    (ray.opl + ray.entrance_opl - 1.5 * (norm(sub(first, source)) + t)).abs()
                        < 1e-10
                );
                let hit = ray.image_hit.as_ref().unwrap();
                let normal = [
                    -expected[0] / radius,
                    -expected[1] / radius,
                    1. - (expected[2] - distance) / radius,
                ];
                assert!(norm(sub(hit.normal, normal)) < 1e-10);
                assert_eq!(hit.n_before, 1.5);
                assert_eq!(hit.n_after, 1.5);
                assert_eq!(hit.point, ray.image());
                assert_eq!(hit.opl, ray.opl);
                assert!((s.object_surface.semi_diameter - 5_f64.sqrt()).abs() < 1e-12);
                assert!(s.image_surface.semi_diameter > 0.);
                s.object_surface.clear_aperture = true;
                s.object_surface.aperture = 0.5;
                s.update().unwrap();
                assert!(
                    s.trace_field(0., 0., &s.fields[0], s.reference())
                        .unwrap_err()
                        .contains("object aperture")
                );
                assert!((s.object_surface.semi_diameter - 5_f64.sqrt()).abs() < 1e-12);
            }
        }
    }
    #[test]
    fn curved_angle_source_keeps_reference_field_direction_and_internal_stop_target() {
        for mapping in [PupilMapping::RealStop, PupilMapping::ReferenceEntrancePupil] {
            let mut s = uniform_source();
            s.pupil_mapping = mapping;
            s.object_surface.radius = 500.;
            s.field_type = FieldType::Angle;
            s.fields = vec![Field {
                x: 1.,
                ..Field::meridional(2.)
            }];
            s.surfaces[0].thickness = 10.;
            s.surfaces.push(s.surfaces[0].clone());
            s.stop_index = 1;
            s.update().unwrap();
            let ray = s.trace_field(0., 0., &s.fields[0], s.reference()).unwrap();
            let source = ray.object_point.unwrap();
            let sag = 500. - (250000. - source[0] * source[0] - source[1] * source[1]).sqrt();
            assert!((source[2] - (-100. + sag)).abs() < 1e-10);
            let expected = unit([1_f64.to_radians().tan(), 2_f64.to_radians().tan(), 1.]).unwrap();
            assert!(norm(sub(ray.launch_direction, expected)) < 1e-11);
            assert!(ray.segments[1].point[0].hypot(ray.segments[1].point[1]) < 1e-9);
        }
    }
    #[test]
    fn image_aperture_controls_sampled_area_vignetting_and_unclipped_envelope() {
        use crate::optics::{PupilSampling, aperture};
        let mut s = uniform_source();
        s.object_material = "AIR".into();
        s.object_distance = None;
        s.object_surface = EndpointSurface::plane();
        s.field_type = FieldType::Angle;
        s.fields = vec![Field::meridional(0.)];
        s.surfaces[0].clear_aperture = false;
        s.image_surface.radius = 10.;
        s.image_surface.clear_aperture = true;
        s.image_surface.aperture = 0.55;
        s.update().unwrap();
        let sampling = PupilSampling::Grid { count: 9 };
        let spot = s.spot_field(&s.fields[0], s.reference(), sampling).unwrap();
        assert_eq!(spot["sampled_rays"], 49);
        assert_eq!(spot["blocked"], 36);
        assert_eq!(spot["points"].as_array().unwrap().len(), 13);
        assert!((spot["transmitted_weight"].as_f64().unwrap() - 13. / 49.).abs() < 1e-12);
        let report = aperture::semi_diameters(&s).unwrap();
        assert_eq!(report.image.reached_rays, 5);
        assert_eq!(report.image.semi_diameter, 1.);
        assert_eq!(report.object.reached_rays, 0);
        assert_eq!(report.failed_rays, 0);
        let (next, vignette) = aperture::update_vignetting(&s).unwrap();
        for factor in next.fields[0].vignetting {
            assert!((factor - 0.45).abs() < 2e-9);
        }
        assert!(
            vignette.fields[0]
                .boundaries
                .iter()
                .all(|b| b.limiting_surface.is_none() && b.limiting_endpoint == Some("image"))
        );
        assert_eq!(
            next.spot_field(&next.fields[0], next.reference(), sampling)
                .unwrap(),
            spot
        );
        s.image_surface.clear_aperture = false;
        s.image_surface.radius = 0.5;
        s.update().unwrap();
        let missed = s.spot_field(&s.fields[0], s.reference(), sampling).unwrap();
        assert_eq!(missed["blocked"], 0);
        assert_eq!(missed["missed"], 36);
        assert_eq!(missed["points"].as_array().unwrap().len(), 13);
        assert_eq!(s.object_surface.semi_diameter, 0.);
    }
}
