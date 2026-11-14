//! `Boxes`: a tiny orthographic box renderer for previews (player model +
//! cosmetics): Minecraft-style cubes with box UV are transformed on the CPU,
//! back faces culled, the rest depth-sorted and handed to the canvas as free
//! quads. Coordinates are model pixels with y pointing down, exactly like
//! vanilla entity models.
//!
//! Faces are cut into one solid quad per texel and every texel is
//! depth-sorted on its own. Sorting whole faces (or cubes) can't be right
//! when cubes overlap: a cape, a hood around the head or blades crossing the
//! back would paint over parts that are in front of them. Per texel there is
//! no such overlap, and fully transparent texels are dropped, so hats, wings
//! and blades show what is really behind them.
#![allow(dead_code)]

use horizon_client::native_ui::{Canvas, Image};
use std::sync::Arc;

/// Texels per face side at most (big textures are sampled, not drawn texel by texel).
const MAX_SIDE: usize = 48;

/// Where a texture's texels come from: the image, with its own size.
pub struct Skin<'a> {
    pub image: &'a Image,
}

pub struct Boxes {
    /// 3x3 rotation (row-major) + translation.
    m: [f32; 12],
    stack: Vec<[f32; 12]>,
    view: [f32; 9],
    cx: f32,
    cy: f32,
    scale: f32,
    quads: Vec<Quad>,
    /// Draw faces that point away too (cosmetics: vanilla renders those without back-face culling).
    pub two_sided: bool,
    /// Colour laid over every texel after the lighting (alpha = strength), like the game's red hurt flash; 0 = none.
    pub overlay: u32,
}

struct Quad {
    xy: [f32; 8],
    z: f32,
    color: u32,
}

impl Default for Boxes {
    fn default() -> Self {
        Self::new()
    }
}

fn mul3(a: &[f32; 9], b: &[f32; 9]) -> [f32; 9] {
    let mut out = [0.0; 9];
    for r in 0..3 {
        for c in 0..3 {
            out[r * 3 + c] = a[r * 3] * b[c] + a[r * 3 + 1] * b[3 + c] + a[r * 3 + 2] * b[6 + c];
        }
    }
    out
}

impl Boxes {
    pub fn new() -> Self {
        let mut b = Boxes {
            m: [0.0; 12],
            stack: Vec::with_capacity(32),
            view: [0.0; 9],
            cx: 0.0,
            cy: 0.0,
            scale: 1.0,
            quads: Vec::with_capacity(4096),
            two_sided: false,
            overlay: 0,
        };
        b.identity();
        b
    }

    pub fn begin(&mut self, cx: f32, cy: f32, scale: f32, yaw: f32, pitch: f32) {
        self.cx = cx;
        self.cy = cy;
        self.scale = scale;
        self.overlay = 0;
        // view = Rx(pitch) * Ry(yaw)
        let (cyw, syw, cp, sp) = (yaw.cos(), yaw.sin(), pitch.cos(), pitch.sin());
        let ry = [cyw, 0.0, syw, 0.0, 1.0, 0.0, -syw, 0.0, cyw];
        let rx = [1.0, 0.0, 0.0, 0.0, cp, -sp, 0.0, sp, cp];
        self.view = mul3(&rx, &ry);
        self.identity();
        self.stack.clear();
        self.quads.clear();
        self.two_sided = false;
    }

    pub fn identity(&mut self) {
        self.m = [0.0; 12];
        self.m[0] = 1.0;
        self.m[4] = 1.0;
        self.m[8] = 1.0;
    }

    pub fn push(&mut self) {
        if self.stack.len() < 64 {
            self.stack.push(self.m);
        }
    }

    pub fn pop(&mut self) {
        if let Some(m) = self.stack.pop() {
            self.m = m;
        }
    }

    pub fn translate(&mut self, x: f32, y: f32, z: f32) {
        let m = &mut self.m;
        m[9] += m[0] * x + m[1] * y + m[2] * z;
        m[10] += m[3] * x + m[4] * y + m[5] * z;
        m[11] += m[6] * x + m[7] * y + m[8] * z;
    }

    /// Vanilla ModelPart order: v' = Rz(roll) * Ry(yaw) * Rx(pitch) * v.
    pub fn rotate(&mut self, pitch: f32, yaw: f32, roll: f32) {
        if roll != 0.0 {
            self.rot(2, roll);
        }
        if yaw != 0.0 {
            self.rot(1, yaw);
        }
        if pitch != 0.0 {
            self.rot(0, pitch);
        }
    }

    fn rot(&mut self, axis: usize, a: f32) {
        let (c, s) = (a.cos(), a.sin());
        let r = match axis {
            0 => [1.0, 0.0, 0.0, 0.0, c, -s, 0.0, s, c],
            1 => [c, 0.0, s, 0.0, 1.0, 0.0, -s, 0.0, c],
            _ => [c, -s, 0.0, s, c, 0.0, 0.0, 0.0, 1.0],
        };
        let cur = [self.m[0], self.m[1], self.m[2], self.m[3], self.m[4], self.m[5], self.m[6], self.m[7], self.m[8]];
        let out = mul3(&cur, &r);
        self.m[..9].copy_from_slice(&out);
    }

    /// Model space to screen: x, y and the depth (larger = farther).
    fn project(&self, x: f32, y: f32, z: f32) -> [f32; 3] {
        let m = &self.m;
        let wx = m[0] * x + m[1] * y + m[2] * z + m[9];
        let wy = m[3] * x + m[4] * y + m[5] * z + m[10];
        let wz = m[6] * x + m[7] * y + m[8] * z + m[11];
        let v = &self.view;
        let vx = v[0] * wx + v[1] * wy + v[2] * wz;
        let vy = v[3] * wx + v[4] * wy + v[5] * wz;
        let vz = v[6] * wx + v[7] * wy + v[8] * wz;
        [self.cx + vx * self.scale, self.cy + vy * self.scale, vz]
    }

    /// A model-space normal in camera space.
    fn normal(&self, nx: f32, ny: f32, nz: f32) -> [f32; 3] {
        let m = &self.m;
        let wx = m[0] * nx + m[1] * ny + m[2] * nz;
        let wy = m[3] * nx + m[4] * ny + m[5] * nz;
        let wz = m[6] * nx + m[7] * ny + m[8] * nz;
        let v = &self.view;
        [
            v[0] * wx + v[1] * wy + v[2] * wz,
            v[3] * wx + v[4] * wy + v[5] * wz,
            v[6] * wx + v[7] * wy + v[8] * wz,
        ]
    }

    /// One cube. (x, y, z) is the min corner, (w, h, d) the size in pixels, (u, v) the box-UV origin on a
    /// `tex_w` x `tex_h` texture.
    #[allow(clippy::too_many_arguments)]
    pub fn cube(
        &mut self,
        texture: &Image,
        tex_w: u32,
        tex_h: u32,
        o: [f32; 3],
        size: [f32; 3],
        uv: [f32; 2],
        inflate: f32,
        mirror: bool,
        tint: u32,
    ) {
        let (x, y, zz) = (o[0], o[1], o[2]);
        let (w, h, d) = (size[0], size[1], size[2]);
        let (u, v) = (uv[0], uv[1]);
        let (x0, y0, z0) = (x - inflate, y - inflate, zz - inflate);
        let (x1, y1, z1) = (x + w + inflate, y + h + inflate, zz + d + inflate);
        let (iu, iv) = (1.0 / tex_w as f32, 1.0 / tex_h as f32);
        // Region rects {u0, v0, u1, v1} in texture pixels.
        let front = [u + d, v + d, u + d + w, v + d + h];
        let mut right = [u, v + d, u + d, v + d + h];
        let mut left = [u + d + w, v + d, u + d + w + d, v + d + h];
        let back = [u + d + w + d, v + d, u + d + w + d + w, v + d + h];
        let top = [u + d, v, u + d + w, v + d];
        let bottom = [u + d + w, v, u + d + w + w, v + d];
        if mirror {
            std::mem::swap(&mut right, &mut left);
        }
        let f = |s: &mut Self, r: [f32; 4], n: [f32; 3], a: [f32; 3], b: [f32; 3], q: [f32; 3], dd: [f32; 3]| {
            s.face(texture, tint, iu, iv, r, mirror, n, a, b, q, dd);
        };
        f(self, front, [0.0, 0.0, -1.0], [x0, y0, z0], [x1, y0, z0], [x1, y1, z0], [x0, y1, z0]);
        f(self, right, [-1.0, 0.0, 0.0], [x0, y0, z1], [x0, y0, z0], [x0, y1, z0], [x0, y1, z1]);
        f(self, left, [1.0, 0.0, 0.0], [x1, y0, z0], [x1, y0, z1], [x1, y1, z1], [x1, y1, z0]);
        f(self, back, [0.0, 0.0, 1.0], [x1, y0, z1], [x0, y0, z1], [x0, y1, z1], [x1, y1, z1]);
        f(self, top, [0.0, -1.0, 0.0], [x0, y0, z1], [x1, y0, z1], [x1, y0, z0], [x0, y0, z0]);
        f(self, bottom, [0.0, 1.0, 0.0], [x0, y1, z0], [x1, y1, z0], [x1, y1, z1], [x0, y1, z1]);
    }

    #[allow(clippy::too_many_arguments)]
    fn face(&mut self, texture: &Image, tint: u32, iu: f32, iv: f32, r: [f32; 4], mirror: bool, n: [f32; 3], a: [f32; 3], b: [f32; 3], _q: [f32; 3], d: [f32; 3]) {
        if r[2] - r[0] <= 0.0 || r[3] - r[1] <= 0.0 {
            return;
        }
        let mut nv = self.normal(n[0], n[1], n[2]);
        if nv[2] > -0.0001 && nv[2] < 0.0001 {
            return; // Edge-on.
        }
        let away = nv[2] > 0.0;
        if away && !self.two_sided {
            return;
        }
        if away {
            // Light the inside as the side we are looking at.
            nv = [-nv[0], -nv[1], -nv[2]];
        }
        let (fa, fb, fd) = (self.project(a[0], a[1], a[2]), self.project(b[0], b[1], b[2]), self.project(d[0], d[1], d[2]));
        // Light from the front, above and a little to the left.
        let lit = (nv[0] * -0.35 + nv[1] * -0.55 + nv[2] * -0.76).max(0.0);
        let bright = 0.58 + 0.42 * lit;
        // Texel grid of the region, in real image pixels (textures can be bigger than the model's declared size).
        let (sx, sy) = (texture.width as f32 * iu, texture.height as f32 * iv);
        let (px0, py0, px1, py1) = (r[0] * sx, r[1] * sy, r[2] * sx, r[3] * sy);
        let cols = ((px1 - px0).round() as i32).max(1) as usize;
        let rows = ((py1 - py0).round() as i32).max(1) as usize;
        let (gc, gr) = (cols.min(MAX_SIDE), rows.min(MAX_SIDE));
        let stride = gc + 1;
        let mut grid = vec![[0.0f32; 3]; stride * (gr + 1)];
        for j in 0..=gr {
            let t = j as f32 / gr as f32;
            for i in 0..=gc {
                let u = i as f32 / gc as f32;
                grid[j * stride + i] = [
                    fa[0] + (fb[0] - fa[0]) * u + (fd[0] - fa[0]) * t,
                    fa[1] + (fb[1] - fa[1]) * u + (fd[1] - fa[1]) * t,
                    fa[2] + (fb[2] - fa[2]) * u + (fd[2] - fa[2]) * t,
                ];
            }
        }
        let (tw, th) = (texture.width as i32, texture.height as i32);
        let (ta, tr, tg, tb) = (tint >> 24, (tint >> 16) & 255, (tint >> 8) & 255, tint & 255);
        for j in 0..gr {
            let sy0 = (py0 + (j as f32 + 0.5) * (py1 - py0) / gr as f32) as i32;
            if sy0 < 0 || sy0 >= th {
                continue;
            }
            for i in 0..gc {
                let uu = (i as f32 + 0.5) / gc as f32;
                let sx0 = (if mirror { px1 - uu * (px1 - px0) } else { px0 + uu * (px1 - px0) }) as i32;
                if sx0 < 0 || sx0 >= tw {
                    continue;
                }
                let c = texture.argb[(sy0 * tw + sx0) as usize];
                let alpha = (c >> 24) * ta / 255;
                if alpha < 8 {
                    continue; // A transparent texel: whatever is behind shows through.
                }
                let mut rr = (((c >> 16) & 255) * tr / 255) as f32 * bright;
                let mut gg = (((c >> 8) & 255) * tg / 255) as f32 * bright;
                let mut bb = ((c & 255) * tb / 255) as f32 * bright;
                let oa = (self.overlay >> 24) as f32;
                if oa != 0.0 {
                    rr += (((self.overlay >> 16) & 255) as f32 - rr) * oa / 255.0;
                    gg += (((self.overlay >> 8) & 255) as f32 - gg) * oa / 255.0;
                    bb += ((self.overlay & 255) as f32 - bb) * oa / 255.0;
                }
                let color = alpha << 24 | (rr as u32).min(255) << 16 | (gg as u32).min(255) << 8 | (bb as u32).min(255);
                let k = j * stride + i;
                let (p0, p1, p2, p3) = (grid[k], grid[k + 1], grid[k + stride + 1], grid[k + stride]);
                self.quads.push(Quad {
                    xy: [p0[0], p0[1], p1[0], p1[1], p2[0], p2[1], p3[0], p3[1]],
                    z: (p0[2] + p1[2] + p2[2] + p3[2]) * 0.25,
                    color,
                });
            }
        }
    }

    /// Sorts every texel back to front (larger view z = farther) and draws them.
    pub fn end(&mut self, c: &mut Canvas) {
        self.quads.sort_by(|a, b| b.z.total_cmp(&a.z));
        // a model faded with its page shows its back faces through its front ones: draw it solid
        c.push_solid();
        for q in self.quads.drain(..) {
            c.free_quad(None::<&Arc<Image>>, q.xy, [0.0; 8], q.color);
        }
        c.pop_alpha();
    }

    /// A preview wireframe, in the exact same model/camera transform as the skinned cubes.
    #[allow(clippy::too_many_arguments)]
    pub fn wire_box(&self, c: &mut Canvas, o: [f32; 3], size: [f32; 3], width: f32, colour: u32) {
        let (x, y, z) = (o[0], o[1], o[2]);
        let (w, h, d) = (size[0], size[1], size[2]);
        for i in 0..8u32 {
            if h == 0.0 && i & 2 != 0 {
                continue;
            }
            for bit in [1u32, 2, 4] {
                if h == 0.0 && bit == 2 {
                    continue;
                }
                let j = i ^ bit;
                if j <= i {
                    continue;
                }
                let at = |k: u32| [x + if k & 1 == 0 { 0.0 } else { w }, y + if k & 2 == 0 { 0.0 } else { h }, z + if k & 4 == 0 { 0.0 } else { d }];
                self.line_3d(c, at(i), at(j), width, colour);
            }
        }
    }

    /// Projects a crisp line without changing the model or the camera.
    pub fn line_3d(&self, c: &mut Canvas, a: [f32; 3], b: [f32; 3], width: f32, colour: u32) {
        let (p, q) = (self.project(a[0], a[1], a[2]), self.project(b[0], b[1], b[2]));
        let (dx, dy) = (q[0] - p[0], q[1] - p[1]);
        let len = (dx * dx + dy * dy).sqrt();
        if len < 0.001 {
            return;
        }
        let (nx, ny) = (-dy / len * width / 2.0, dx / len * width / 2.0);
        c.free_quad(None::<&Arc<Image>>, [p[0] + nx, p[1] + ny, q[0] + nx, q[1] + ny, q[0] - nx, q[1] - ny, p[0] - nx, p[1] - ny], [0.0; 8], colour);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cube_facing_the_viewer_draws_its_visible_texels() {
        let mut c = horizon_client::native_ui::canvas();
        c.begin(200, 200, 1.0);
        let skin = Image::new(64, 64, vec![0xFFC08060; 64 * 64]);
        let mut b = Boxes::new();
        b.begin(100.0, 100.0, 4.0, 0.0, 0.0);
        b.cube(&skin, 64, 64, [-4.0, -8.0, -4.0], [8.0, 8.0, 8.0], [0.0, 0.0], 0.0, false, 0xFFFFFFFF);
        // Front, top and bottom edge-on cases culled: at least the front face's 8 x 8 texels are there.
        assert!(b.quads.len() >= 64, "{}", b.quads.len());
        b.end(&mut c);
        assert!(b.quads.is_empty());
    }

    #[test]
    fn a_transparent_texture_draws_nothing() {
        let skin = Image::new(64, 64, vec![0; 64 * 64]);
        let mut b = Boxes::new();
        b.begin(0.0, 0.0, 1.0, 0.3, 0.2);
        b.cube(&skin, 64, 64, [0.0; 3], [4.0; 3], [0.0; 2], 0.0, true, 0xFFFFFFFF);
        assert!(b.quads.is_empty());
    }
}
