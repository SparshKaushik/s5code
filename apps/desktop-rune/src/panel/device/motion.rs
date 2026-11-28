//! Web `deviceMotion.ts` and `nearestDeviceView`: the 3D view's rotation follows a drag on a
//! spring, coasts and settles after a flick, and springs to the nearest rest view when let go.
//! Times are milliseconds, as web's `performance.now()`.

use std::f32::consts::TAU;

/// x, y, z, w, as three.js's `Quaternion`.
pub type Quat = [f32; 4];
pub type Vec3 = [f32; 3];

pub const IDENTITY: Quat = [0., 0., 0., 1.];

const GAIN: f32 = 0.006;
const RESPONSE: f32 = 0.5;
const DAMPING: f32 = 0.78;
const FREQUENCY: f32 = TAU / RESPONSE;
const DECAY: f32 = FREQUENCY * DAMPING;
const STEP_SECONDS: f32 = 1. / 120.;
const FLICK_THRESHOLD: f32 = 4.5;
const FLICK_LIMIT: f32 = 22.;
const SPIN_DECAY: f32 = 1.1;
const SETTLE_FREQUENCY: f32 = 2.;

// Quaternion and vector arithmetic, as three.js does it.

/// `a.multiply(b)`.
pub fn mul(a: Quat, b: Quat) -> Quat {
    [
        a[0] * b[3] + a[3] * b[0] + a[1] * b[2] - a[2] * b[1],
        a[1] * b[3] + a[3] * b[1] + a[2] * b[0] - a[0] * b[2],
        a[2] * b[3] + a[3] * b[2] + a[0] * b[1] - a[1] * b[0],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

pub fn invert(q: Quat) -> Quat {
    [-q[0], -q[1], -q[2], q[3]]
}

pub fn normalize(q: Quat) -> Quat {
    let l = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if l == 0. { IDENTITY } else { q.map(|c| c / l) }
}

fn dot4(a: Quat, b: Quat) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3]
}

/// The angle between two orientations; `atan2` keeps small angles exact in f32, where `acos` near
/// 1 can't resolve below about a milliradian.
pub fn angle_to(a: Quat, b: Quat) -> f32 {
    let r = mul(invert(normalize(a)), normalize(b));
    2. * (r[0] * r[0] + r[1] * r[1] + r[2] * r[2]).sqrt().atan2(r[3].abs())
}

pub fn axis_angle(axis: Vec3, angle: f32) -> Quat {
    let (s, c) = (angle / 2.).sin_cos();
    [axis[0] * s, axis[1] * s, axis[2] * s, c]
}

/// three.js `slerp`.
fn slerp(a: Quat, b: Quat, t: f32) -> Quat {
    if t <= 0. {
        return a;
    }
    if t >= 1. {
        return b;
    }
    let mut cos = dot4(a, b);
    let b = if cos < 0. {
        cos = -cos;
        b.map(|c| -c)
    } else {
        b
    };
    if cos >= 1. {
        return a;
    }
    let sin_sq = 1. - cos * cos;
    if sin_sq <= f32::EPSILON {
        let s = 1. - t;
        return normalize([0, 1, 2, 3].map(|k| s * a[k] + t * b[k]));
    }
    let sin = sin_sq.sqrt();
    let half = sin.atan2(cos);
    let (ra, rb) = (((1. - t) * half).sin() / sin, (t * half).sin() / sin);
    [0, 1, 2, 3].map(|k| a[k] * ra + b[k] * rb)
}

fn length(v: Vec3) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

fn scale(v: Vec3, s: f32) -> Vec3 {
    v.map(|c| c * s)
}

fn add_scaled(v: Vec3, w: Vec3, s: f32) -> Vec3 {
    [v[0] + w[0] * s, v[1] + w[1] * s, v[2] + w[2] * s]
}

fn clamp_length(v: Vec3, max: f32) -> Vec3 {
    let l = length(v);
    if l > max { scale(v, max / l) } else { v }
}

fn dot3(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// `v.applyQuaternion(q)`.
fn rotate(v: Vec3, q: Quat) -> Vec3 {
    let t = [2. * (q[1] * v[2] - q[2] * v[1]), 2. * (q[2] * v[0] - q[0] * v[2]), 2. * (q[0] * v[1] - q[1] * v[0])];
    [
        v[0] + q[3] * t[0] + q[1] * t[2] - q[2] * t[1],
        v[1] + q[3] * t[1] + q[2] * t[0] - q[0] * t[2],
        v[2] + q[3] * t[2] + q[0] * t[1] - q[1] * t[0],
    ]
}

/// The rotation as an axis scaled by its angle; q and -q are the same orientation.
pub fn rotation_vector(q: Quat) -> Vec3 {
    let mut q = normalize(q);
    if q[3] < 0. {
        q = q.map(|c| -c);
    }
    let l = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2]).sqrt();
    if l < 1e-8 { [0.; 3] } else { scale([q[0], q[1], q[2]], 2. * l.atan2(q[3]) / l) }
}

/// three.js `Euler(x, y, z, "YXZ")` as a rotation.
pub fn euler_yxz(x: f32, y: f32, z: f32) -> Quat {
    mul(mul(axis_angle([0., 1., 0.], y), axis_angle([1., 0., 0.], x)), axis_angle([0., 0., 1.], z))
}

pub fn from_vector(v: Vec3) -> Quat {
    let angle = length(v);
    if angle < 1e-8 { IDENTITY } else { axis_angle(scale(v, 1. / angle), angle) }
}

/// The rotation as a column-major matrix.
pub fn matrix(q: Quat) -> [f32; 16] {
    let [x, y, z, w] = q;
    [
        1. - 2. * (y * y + z * z),
        2. * (x * y + z * w),
        2. * (x * z - y * w),
        0.,
        2. * (x * y - z * w),
        1. - 2. * (x * x + z * z),
        2. * (y * z + x * w),
        0.,
        2. * (x * z + y * w),
        2. * (y * z - x * w),
        1. - 2. * (x * x + y * y),
        0.,
        0.,
        0.,
        0.,
        1.,
    ]
}

/// `nearestDeviceView` over views of `rotation`, each allowed to turn about y within `yaw_limit`.
pub fn nearest_view(rotation: Quat, views: &[(Quat, f32)]) -> Option<Quat> {
    let mut closest = None;
    let mut distance = f32::INFINITY;
    for &(view, yaw_limit) in views {
        let relative = mul(rotation, invert(view));
        let turn = 2. * relative[1].atan2(relative[3]);
        let yaw = turn.sin().atan2(turn.cos()).clamp(-yaw_limit, yaw_limit);
        let candidate = mul(axis_angle([0., 1., 0.], yaw), view);
        let next = angle_to(candidate, rotation);
        if next < distance - 1e-8 {
            distance = next;
            closest = Some(candidate);
        }
    }
    closest
}


struct Spring {
    at: f64,
    rotation: Quat,
    velocity: Vec3,
    steps: u32,
}

struct Spin {
    at: f64,
    rotation: Quat,
    velocity: Vec3,
    correction: Vec3,
}

struct Drag {
    start: Quat,
    rest: Quat,
    x: f32,
    y: f32,
    error: Vec3,
    at: f64,
}

/// `createDeviceMotion`. `choose` picks the rest view for a rotation when a turn ends.
pub struct Motion {
    pub rotation: Quat,
    target: Quat,
    velocity: Vec3,
    gesture_velocity: Vec3,
    spring: Option<Spring>,
    spin: Option<Spin>,
    drag: Option<Drag>,
    held: bool,
    interrupted_drag: bool,
    last_input: f64,
}

impl Default for Motion {
    fn default() -> Self {
        Motion {
            rotation: IDENTITY,
            target: IDENTITY,
            velocity: [0.; 3],
            gesture_velocity: [0.; 3],
            spring: None,
            spin: None,
            drag: None,
            held: false,
            interrupted_drag: false,
            last_input: f64::NEG_INFINITY,
        }
    }
}

fn integrate(target: Quat, q: &mut Quat, speed: &mut Vec3, dt: f32) {
    let error = rotation_vector(mul(target, invert(*q)));
    let acceleration = add_scaled(scale(error, FREQUENCY * FREQUENCY), *speed, -2. * DECAY);
    *speed = clamp_length(add_scaled(*speed, acceleration, dt), 9.);
    *q = normalize(mul(from_vector(scale(*speed, dt)), *q));
}

impl Motion {
    fn begin_spring(&mut self, next: Quat, now: f64) {
        self.target = normalize(next);
        self.spring = Some(Spring { at: now, rotation: self.rotation, velocity: self.velocity, steps: 0 });
    }

    fn release(&mut self, now: f64, choose: &dyn Fn(Quat) -> Quat) {
        let Some(drag) = self.drag.take() else { return };
        if now - self.last_input > 100. {
            self.gesture_velocity = [0.; 3];
        }
        let prediction = mul(from_vector(scale(self.gesture_velocity, 0.085)), self.target);
        self.velocity = clamp_length(add_scaled(scale(self.velocity, 0.2), self.gesture_velocity, 0.8), FLICK_LIMIT);
        let moved = drag.x != 0. || drag.y != 0.;
        if moved && length(self.velocity) >= FLICK_THRESHOLD {
            let projected = mul(from_vector(scale(self.velocity, 1. / SPIN_DECAY)), self.rotation);
            self.target = normalize(choose(projected));
            self.spin = Some(Spin { at: now, rotation: self.rotation, velocity: self.velocity, correction: rotation_vector(mul(self.target, invert(projected))) });
            return;
        }
        self.begin_spring(if moved { choose(prediction) } else { drag.rest }, now);
    }

    /// Steps the rotation to `now`; true when it moved.
    pub fn advance(&mut self, now: f64) -> bool {
        if self.held || !now.is_finite() {
            return false;
        }
        if let Some(spin) = &self.spin {
            let seconds = ((now - spin.at) / 1000.).max(0.) as f32;
            let coast = (-SPIN_DECAY * seconds).exp();
            let settle = (-SETTLE_FREQUENCY * seconds).exp();
            let correction = from_vector(scale(spin.correction, 1. - (1. + SETTLE_FREQUENCY * seconds) * settle));
            self.rotation = normalize(mul(correction, mul(from_vector(scale(spin.velocity, (1. - coast) / SPIN_DECAY)), spin.rotation)));
            self.velocity = add_scaled(rotate(scale(spin.velocity, coast), correction), spin.correction, SETTLE_FREQUENCY * SETTLE_FREQUENCY * seconds * settle);
            if angle_to(self.rotation, self.target) < 0.01 && length(self.velocity) < 0.08 {
                self.spin = None;
                self.rotation = self.target;
                self.velocity = [0.; 3];
            }
            return true;
        }
        if let Some(drag) = &mut self.drag {
            // Follow the target during the gesture, keeping the logarithm's winding so a long
            // drag can't reverse its spring force when it crosses a half turn.
            let seconds = ((now - drag.at) / 1000.).clamp(0., 0.05) as f32;
            drag.at = now;
            let steps = (seconds * 120.).ceil() as u32;
            let dt = if steps > 0 { seconds / steps as f32 } else { 0. };
            for _ in 0..steps {
                let mut error = rotation_vector(mul(self.target, invert(self.rotation)));
                if dot3(error, error) > 1e-10 {
                    let axis = scale(error, 1. / length(error));
                    let turns = ((dot3(drag.error, axis) - length(error)) / TAU + 0.5).floor();
                    error = add_scaled(error, axis, turns * TAU);
                }
                drag.error = error;
                let omega = TAU / 0.68;
                let acceleration = add_scaled(scale(clamp_length(error, 1.2), omega * omega), self.velocity, -2. * 1.12 * omega);
                self.velocity = clamp_length(add_scaled(self.velocity, acceleration, dt), 9.);
                self.rotation = normalize(mul(from_vector(scale(self.velocity, dt)), self.rotation));
            }
            return steps > 0;
        }
        let target = self.target;
        let Some(spring) = &mut self.spring else { return false };
        let seconds = ((now - spring.at) / 1000.).max(0.) as f32;
        let steps = ((seconds / STEP_SECONDS).floor() as u32).min(240);
        // Fixed ticks make a release the same at any refresh rate; the last partial tick
        // interpolates toward the next without committing it.
        while spring.steps < steps {
            integrate(target, &mut spring.rotation, &mut spring.velocity, STEP_SECONDS);
            spring.steps += 1;
        }
        let (mut next, mut next_velocity) = (spring.rotation, spring.velocity);
        integrate(target, &mut next, &mut next_velocity, STEP_SECONDS);
        let fraction = ((seconds - steps as f32 * STEP_SECONDS) / STEP_SECONDS).min(1.);
        self.rotation = slerp(spring.rotation, next, fraction);
        self.velocity = [0, 1, 2].map(|k| spring.velocity[k] + (next_velocity[k] - spring.velocity[k]) * fraction);
        if steps == 240 || (angle_to(self.rotation, target) < 0.0005 && length(self.velocity) < 0.005) {
            self.rotation = target;
            self.velocity = [0.; 3];
            self.spring = None;
        }
        true
    }

    fn begin_drag(&mut self, now: f64) {
        if self.held || self.drag.is_some() {
            return;
        }
        self.advance(now);
        self.drag = Some(Drag { start: self.rotation, rest: self.target, x: 0., y: 0., error: [0.; 3], at: now });
        self.target = self.rotation;
        self.gesture_velocity = [0.; 3];
        self.last_input = f64::NEG_INFINITY;
        self.spring = None;
        self.spin = None;
    }

    /// A turning gesture starts or ends; at its end the phone flicks or springs to rest.
    pub fn drag_active(&mut self, active: bool, now: f64, choose: &dyn Fn(Quat) -> Quat) {
        if active {
            if self.drag.is_some() || self.spin.is_some() {
                self.advance(now);
                self.drag = None;
                self.spin = None;
            }
            self.begin_drag(now);
        } else {
            self.advance(now);
            self.release(now, choose);
        }
    }

    /// Turns the target by a drag of (x, y) points.
    pub fn orbit(&mut self, x: f32, y: f32, now: f64) {
        if self.held || !(x.is_finite() && y.is_finite() && now.is_finite()) || (x == 0. && y == 0.) {
            return;
        }
        self.advance(now);
        self.begin_drag(now);
        let Some(drag) = &mut self.drag else { return };
        let previous = self.target;
        drag.x += x;
        drag.y += y;
        self.target = normalize(mul(from_vector([drag.y * GAIN, drag.x * GAIN, 0.]), drag.start));
        let seconds = (now - self.last_input) / 1000.;
        let change = rotation_vector(mul(self.target, invert(previous)));
        self.gesture_velocity = if seconds > 0. && seconds < 0.1 { clamp_length(scale(change, 1. / seconds as f32), FLICK_LIMIT) } else { [0.; 3] };
        self.last_input = now;
    }

    /// Holds the phone still while a touch is on its screen.
    pub fn hold(&mut self, active: bool, now: f64, choose: &dyn Fn(Quat) -> Quat) {
        if self.held == active {
            return;
        }
        if active {
            self.interrupted_drag = self.drag.is_some() || self.spin.is_some();
            self.drag = None;
            self.spin = None;
            self.spring = None;
            self.velocity = [0.; 3];
        }
        self.held = active;
        if !active {
            if self.interrupted_drag {
                self.begin_spring(choose(self.rotation), now);
            } else if angle_to(self.rotation, self.target) > 0.0005 {
                self.begin_spring(self.target, now);
            }
            self.interrupted_drag = false;
        }
    }

    pub fn needs_frame(&self) -> bool {
        !self.held && (self.spring.is_some() || self.spin.is_some() || (self.drag.is_some() && (angle_to(self.rotation, self.target) > 0.0005 || length(self.velocity) > 0.005)))
    }

    /// Turns to `next` on the release spring, or at once.
    pub fn set_pose(&mut self, next: Quat, now: f64, immediate: bool) {
        self.advance(now);
        self.drag = None;
        self.spin = None;
        self.begin_spring(next, now);
        if immediate {
            self.rotation = self.target;
            self.velocity = [0.; 3];
            self.spring = None;
        }
    }

    /// Springs to `next`, ending any gesture.
    pub fn reset(&mut self, next: Quat, now: f64) {
        self.advance(now);
        self.drag = None;
        self.spin = None;
        self.begin_spring(next, now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const YAW_LIMIT: f32 = std::f32::consts::PI / 3.;

    /// A phone's one rest view, as the device panel chooses it.
    fn phone_view(rotation: Quat) -> Quat {
        nearest_view(rotation, &[(IDENTITY, YAW_LIMIT)]).unwrap_or(IDENTITY)
    }

    fn yaw(q: Quat) -> f32 {
        2. * q[1].atan2(q[3])
    }

    /// Steps `m` a frame at a time from `from` until it rests, returning the frames taken.
    fn settle(m: &mut Motion, mut now: f64) -> u32 {
        let mut frames = 0;
        while m.needs_frame() {
            now += 16.;
            m.advance(now);
            frames += 1;
            assert!(frames < 1000, "it comes to rest");
        }
        frames
    }

    #[test]
    fn a_dragged_phone_follows_on_a_spring_and_springs_to_its_nearest_view() {
        let mut m = Motion::default();
        m.drag_active(true, 0., &phone_view);
        m.orbit(100., 0., 0.);
        m.advance(16.);
        assert!(yaw(m.rotation) > 0. && yaw(m.rotation) < 0.6 - 1e-3, "the phone trails the finger: {}", yaw(m.rotation));
        assert!(m.needs_frame());
        // A slow drag far round, then a pause before letting go: no flick.
        m.orbit(200., 0., 400.);
        m.advance(1000.);
        m.drag_active(false, 1000., &phone_view);
        let frames = settle(&mut m, 1000.);
        assert!(frames > 5, "the release springs over frames, not at once");
        assert!((yaw(m.rotation) - YAW_LIMIT).abs() < 1e-3, "it rests at the yaw limit: {}", yaw(m.rotation));
        assert!(!m.needs_frame() && !m.advance(5000.), "and then stays still");
    }

    #[test]
    fn a_flick_coasts_and_settles_on_a_rest_view() {
        let mut m = Motion::default();
        m.drag_active(true, 0., &phone_view);
        for i in 1..=5 {
            m.orbit(120., 0., f64::from(i) * 16.);
            m.advance(f64::from(i) * 16.);
        }
        m.drag_active(false, 80., &phone_view);
        assert!(m.spin.is_some(), "a fast release spins");
        settle(&mut m, 80.);
        let rest = yaw(m.rotation);
        assert!(rest.abs() <= YAW_LIMIT + 1e-3, "{rest}");
        assert!(angle_to(m.rotation, phone_view(m.rotation)) < 0.02);
    }

    #[test]
    fn a_touch_holds_the_phone_and_a_reset_springs_home() {
        let mut m = Motion::default();
        m.orbit(80., 40., 0.);
        m.hold(true, 10., &phone_view);
        assert!(!m.needs_frame() && !m.advance(100.), "a touch holds it still");
        m.orbit(50., 0., 120.);
        m.hold(false, 200., &phone_view);
        settle(&mut m, 200.);
        assert!(angle_to(m.rotation, phone_view(m.rotation)) < 1e-3, "an interrupted turn springs to rest");
        m.reset(IDENTITY, 2000.);
        settle(&mut m, 2000.);
        assert_eq!(m.rotation, IDENTITY);
        assert!((angle_to(axis_angle([0., 1., 0.], 2e-4), IDENTITY) - 2e-4).abs() < 1e-6, "small angles stay exact, so the rest checks can pass");
        let q = axis_angle([0., 1., 0.], 0.3);
        let v = rotate([0., 0., 1.], q);
        assert!((v[0] - 0.3f32.sin()).abs() < 1e-5 && (matrix(q)[8] - v[0]).abs() < 1e-5, "a yaw turns the face toward +x");
    }
}
