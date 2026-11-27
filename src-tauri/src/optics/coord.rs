// SPDX-License-Identifier: GPL-3.0-or-later
// Sequential 3D coordinate frames and surface tilt/decenter transforms.
use super::math::*;
use serde::{Deserialize, Serialize};

/// 3D tilt and decenter applied at a sequential surface (coordinate break).
#[derive(schemars::JsonSchema, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoordinateTransform {
    /// Local lateral decenter [dx, dy] in mm.
    #[serde(default)]
    #[schemars(inner(range(min = -1e6, max = 1e6)))]
    pub decenter: [f64; 2],
    /// Local Euler tilt angles [rx, ry, rz] in degrees (about local X, Y, Z).
    #[serde(default)]
    #[schemars(inner(range(min = -360.0, max = 360.0)))]
    pub tilt: [f64; 3],
    /// When false (default, Zemax order 0), decenter is applied before tilt.
    /// When true (Zemax order 1), tilt is applied before decenter.
    #[serde(default)]
    pub tilt_first: bool,
    /// When true, restore the entering frame after intersecting this surface
    /// (localized surface perturbation, e.g. tolerance error or tilted single face).
    /// When false, subsequent surfaces inherit the transformed optical axis.
    #[serde(default)]
    pub return_after: bool,
}

impl CoordinateTransform {
    pub fn is_identity(&self) -> bool {
        self.decenter == [0., 0.] && self.tilt == [0., 0., 0.]
    }

    pub fn validate(&self) -> Result<(), String> {
        if !self.decenter.iter().all(|v| v.is_finite() && v.abs() <= 1e6) {
            return Err("Invalid coordinate decenter".into());
        }
        if !self.tilt.iter().all(|v| v.is_finite() && v.abs() <= 360.) {
            return Err("Coordinate tilt angles must be finite and within ±360 degrees".into());
        }
        Ok(())
    }
}

/// Orthonormal 3D frame: `axes` are local [X, Y, Z] unit vectors in global coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub origin: V3,
    pub axes: [V3; 3],
    pub aligned: bool,
}

impl Frame {
    pub fn identity() -> Self {
        Self {
            origin: [0., 0., 0.],
            axes: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
            aligned: true,
        }
    }

    pub fn is_centered(&self) -> bool {
        self.aligned && self.origin[0] == 0. && self.origin[1] == 0.
    }

    pub fn translate_z(&mut self, thickness: f64) {
        if self.aligned {
            self.origin[2] += thickness;
        } else {
            self.origin = add(self.origin, scale(self.axes[2], thickness));
        }
    }

    pub fn apply(&self, t: &CoordinateTransform) -> Self {
        if t.is_identity() {
            return *self;
        }
        let mut out = *self;
        let rot = rotation_xyz(t.tilt);
        if t.tilt_first {
            out.axes = compose_axes(out.axes, rot);
            out.origin = add(
                out.origin,
                add(
                    scale(out.axes[0], t.decenter[0]),
                    scale(out.axes[1], t.decenter[1]),
                ),
            );
        } else {
            out.origin = add(
                out.origin,
                add(
                    scale(out.axes[0], t.decenter[0]),
                    scale(out.axes[1], t.decenter[1]),
                ),
            );
            out.axes = compose_axes(out.axes, rot);
        }
        out.aligned = false;
        out
    }

    pub fn to_local_vec(&self, v: V3) -> V3 {
        if self.aligned {
            v
        } else {
            [dot(v, self.axes[0]), dot(v, self.axes[1]), dot(v, self.axes[2])]
        }
    }

    pub fn to_global_vec(&self, v: V3) -> V3 {
        if self.aligned {
            v
        } else {
            add(
                add(scale(self.axes[0], v[0]), scale(self.axes[1], v[1])),
                scale(self.axes[2], v[2]),
            )
        }
    }

    pub fn to_local_point(&self, p: V3) -> V3 {
        self.to_local_vec(sub(p, self.origin))
    }

    pub fn to_global_point(&self, p: V3) -> V3 {
        add(self.origin, self.to_global_vec(p))
    }
}

/// Returns columns [X', Y', Z'] of R = Rx(rx) * Ry(ry) * Rz(rz), angles in degrees.
fn rotation_xyz(tilt_deg: [f64; 3]) -> [V3; 3] {
    let (sx, cx) = tilt_deg[0].to_radians().sin_cos();
    let (sy, cy) = tilt_deg[1].to_radians().sin_cos();
    let (sz, cz) = tilt_deg[2].to_radians().sin_cos();
    // Column 0 (X')
    let c0 = [cy * cz, sx * sy * cz + cx * sz, -cx * sy * cz + sx * sz];
    // Column 1 (Y')
    let c1 = [-cy * sz, -sx * sy * sz + cx * cz, cx * sy * sz + sx * cz];
    // Column 2 (Z')
    let c2 = [sy, -sx * cy, cx * cy];
    [c0, c1, c2]
}

fn compose_axes(parent: [V3; 3], local: [V3; 3]) -> [V3; 3] {
    let map = |v: V3| {
        add(
            add(scale(parent[0], v[0]), scale(parent[1], v[1])),
            scale(parent[2], v[2]),
        )
    };
    [map(local[0]), map(local[1]), map(local[2])]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_roundtrip_preserves_vectors_and_points() {
        let mut f = Frame::identity();
        f.translate_z(10.);
        let f2 = f.apply(&CoordinateTransform {
            decenter: [1.5, -2.0],
            tilt: [15., -20., 30.],
            tilt_first: false,
            return_after: false,
        });
        let p = [3.2, -4.1, 18.7];
        let back = f2.to_global_point(f2.to_local_point(p));
        for i in 0..3 {
            assert!((back[i] - p[i]).abs() < 1e-12);
        }
        let v = unit([0.2, -0.5, 0.8]).unwrap();
        let vback = f2.to_global_vec(f2.to_local_vec(v));
        for i in 0..3 {
            assert!((vback[i] - v[i]).abs() < 1e-12);
        }
    }

    #[test]
    fn forty_five_degree_mirror_tilt_rotates_z_axis() {
        let f = Frame::identity().apply(&CoordinateTransform {
            decenter: [0., 0.],
            tilt: [45., 0., 0.],
            tilt_first: false,
            return_after: false,
        });
        let expected_z = [0., -std::f64::consts::FRAC_1_SQRT_2, std::f64::consts::FRAC_1_SQRT_2];
        for i in 0..3 {
            assert!((f.axes[2][i] - expected_z[i]).abs() < 1e-12);
        }
    }
}
