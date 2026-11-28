//! Web `duoViewer.ts` and `duoSnap.ts` for the iPhone Duo's 3D view: the hinge eased to the
//! angle the device reports, the pose each stand, fold and rotation presents the Duo in, and the
//! views a turned Duo rests in, which ask the device for the display and rotation they show.

use std::f32::consts::{FRAC_PI_2, PI};

use serde_json::{Value, json};

use super::input::Screen;
use super::model::{DuoTouch, Mesh, Model};
use super::motion::{self, Quat, Vec3};

/// How long a requested display handoff may take before the view settles back on the shown one.
const HANDOFF_MS: f64 = 5000.;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum RestFace {
    Cover,
    #[default]
    Inside,
    Left,
    Right,
}

/// A view the Duo rests in, the rotation it shows its display in, and how far it may turn about y.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Snap {
    pub rotation: Quat,
    pub face: RestFace,
    pub orientation: &'static str,
    pub yaw_limit: f32,
}

const ORIENTATIONS: [&str; 4] = ["portrait", "landscape_left", "portrait_upside_down", "landscape_right"];

/// The display an iPhone Duo readout shows: 1 the cover, 3 the inner.
pub fn panel(screen: &Screen) -> u8 {
    if screen.screen_id == Some(1) { 1 } else { 3 }
}

fn direction(m: &[f32; 16], v: Vec3) -> Vec3 {
    [0, 1, 2].map(|r| m[r] * v[0] + m[4 + r] * v[1] + m[8 + r] * v[2])
}

fn unit(v: Vec3) -> Vec3 {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l == 0. { v } else { v.map(|c| c / l) }
}

fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// `createDuoScene`'s `restFrames`: each face the Duo rests on, as its display's normal and up
/// with the hinge at `angle`; the inner panel's leaves too while it is half open.
pub fn rest_frames(model: &Model, angle: f32, panel: u8) -> Vec<(RestFace, Vec3, Vec3)> {
    let parts = model.parts(angle);
    let frame = |face, keep: &dyn Fn(&Mesh) -> bool| {
        let (mut normal, mut up) = ([0.; 3], [0.; 3]);
        for m in model.meshes.iter().filter(|m| keep(m)) {
            let part = &parts[usize::from(m.part.min(2))];
            if let Some(&n) = m.normals.first() {
                normal = add(normal, unit(direction(part, n)));
            }
            up = add(up, unit(direction(part, [0., 1., 0.])));
        }
        (face, unit(normal), unit(up))
    };
    if panel == 1 {
        return vec![frame(RestFace::Cover, &|m| m.panel == 1)];
    }
    let inside = frame(RestFace::Inside, &|m| m.panel == 3);
    if angle > 20. && angle < 165. {
        vec![inside, frame(RestFace::Left, &|m| m.panel == 3 && m.part == 1), frame(RestFace::Right, &|m| m.panel == 3 && m.part == 2)]
    } else {
        vec![inside]
    }
}

fn add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// `duoViewSnaps`: each face held to the camera in each rotation its display shows, a leaf only
/// upright (the right) or upside down (the left) and tipped back; on the inner panel, no view or
/// turn that shows a leaf edge on.
pub fn snaps(frames: &[(RestFace, Vec3, Vec3)], panel: u8) -> Vec<Snap> {
    let leaf = |face: RestFace| matches!(face, RestFace::Left | RestFace::Right);
    let facing = |q: Quat| frames.iter().filter(|f| leaf(f.0)).all(|f| motion::rotate(f.1, q)[2] >= 0.04);
    let mut snaps = vec![];
    for &(face, normal, up) in frames {
        let normal = unit(normal);
        if normal.iter().map(|c| c * c).sum::<f32>() < 0.5 {
            continue;
        }
        let right = unit(cross(up, normal));
        let face_rotation = motion::invert(motion::from_basis(right, unit(cross(normal, right)), normal));
        for (index, orientation) in ORIENTATIONS.into_iter().enumerate() {
            if (face == RestFace::Right && index != 0) || (face == RestFace::Left && index != 2) {
                continue;
            }
            let roll = (if panel == 3 { FRAC_PI_2 } else { 0. }) - index as f32 * FRAC_PI_2;
            let mut rotation = motion::mul(motion::axis_angle([0., 0., 1.], roll), face_rotation);
            if leaf(face) {
                rotation = motion::mul(motion::axis_angle([1., 0., 0.], PI / 12.), rotation);
            }
            if panel == 3 && !facing(rotation) {
                continue;
            }
            let mut yaw_limit = if face == RestFace::Cover { PI / 9. } else { PI / 3. };
            if panel == 3 {
                for sign in [-1., 1.] {
                    if let Some(step) = (1..=60).find(|&step| !facing(motion::mul(motion::axis_angle([0., 1., 0.], sign * step as f32 * PI / 180.), rotation))) {
                        yaw_limit = yaw_limit.min((step - 1) as f32 * PI / 180.);
                    }
                }
            }
            snaps.push(Snap { rotation, face, orientation, yaw_limit });
        }
    }
    snaps
}

/// `nearestDuoView`: the snap nearest `rotation`, turned about y as near as its limit allows.
pub fn nearest(rotation: Quat, snaps: &[Snap]) -> Option<Snap> {
    let views: Vec<(Quat, f32)> = snaps.iter().map(|s| (s.rotation, s.yaw_limit)).collect();
    motion::nearest_view(rotation, &views).map(|(index, rotation)| Snap { rotation, ..snaps[index] })
}

#[derive(Default)]
pub struct DuoView {
    /// The hinge angle shown, eased toward `target`.
    pub angle: f32,
    target: f32,
    /// When the hinge last stepped, in `now_ms` milliseconds.
    at: Option<f64>,
    seen: Option<Screen>,
    fold_pending: bool,
    physical_pose: &'static str,
    presentation_angle: f32,
    /// The view the Duo rests in for its stand and rotation.
    pub presentation: Quat,
    pub touch: Option<DuoTouch>,
    /// The face the Duo rests on.
    pub rest_face: RestFace,
    /// The display a snap asked the device to hand off to, until it shows or `HANDOFF_MS` passes.
    requested_panel: Option<u32>,
    handoff_until: Option<f64>,
    requested_orientation: bool,
    view_orientation: Option<String>,
}

fn reported_angle(screen: &Screen) -> f32 {
    screen.hinge_angle.map_or(if screen.screen_id == Some(1) { 0. } else { 180. }, |a| a as f32)
}

/// `duoScreenSettled`: the cover shows exactly when the Duo is closed.
pub fn settled(screen: &Screen) -> bool {
    (screen.screen_id == Some(1)) == (reported_angle(screen) == 0.)
}

fn pose_name(pose: Option<&str>) -> Option<&'static str> {
    ["closed", "book", "open", "laptop", "tent"].into_iter().find(|p| Some(*p) == pose)
}

impl DuoView {
    /// Web `setScreen` for a new readout with the view at `rotation`: the pose to turn to, and
    /// whether at once, when the readout changes how the Duo should be presented. `snaps` gives a
    /// display's rest views with the hinge at an angle.
    pub fn screen(&mut self, next: &Screen, rotation: Quat, snaps: &dyn Fn(u8, f32) -> Vec<Snap>) -> Option<(Quat, bool)> {
        if self.seen.as_ref() == Some(next) {
            return None;
        }
        let previous = self.seen.replace(next.clone());
        let first = previous.is_none();
        let cover = next.screen_id == Some(1);
        let owned_handoff = self.requested_panel.is_some();
        if next.screen_id == self.requested_panel {
            self.clear_handoff();
        }
        let requested_rotation = std::mem::take(&mut self.requested_orientation);
        let changed_display = previous.as_ref().map(|p| p.screen_id) != Some(next.screen_id);
        let owned_rotation = requested_rotation && !changed_display;
        let changed_pose = next.hinge_pose.is_some() && next.hinge_pose != previous.as_ref().and_then(|p| p.hinge_pose.clone());
        let before = previous.as_ref().map(reported_angle);
        let after = reported_angle(next);
        if changed_pose {
            self.fold_pending = false;
        } else if before.is_some_and(|b| b != after) {
            self.fold_pending = true;
        }
        let landed_fold = self.fold_pending && settled(next) && !owned_handoff;
        if settled(next) {
            self.fold_pending = false;
        }
        let rotated = !self.fold_pending
            && previous.as_ref().is_some_and(|p| p.screen_id == next.screen_id && p.hinge_angle == next.hinge_angle && p.orientation != next.orientation);
        let left_physical_pose = rotated && !owned_rotation && !owned_handoff && next.hinge_pose.is_none();
        if first || changed_pose || left_physical_pose || landed_fold {
            self.physical_pose = pose_name(next.hinge_pose.as_deref()).unwrap_or(if cover { "closed" } else { "open" });
            self.presentation_angle = after;
        }
        self.target = after;
        if !(first || changed_pose || landed_fold || (rotated && !owned_rotation && !owned_handoff)) {
            // A sensor rotation can hand the view to the other display: face it, without asking
            // for another rotation.
            if changed_display && !owned_handoff && (self.rest_face == RestFace::Cover) != cover && (requested_rotation || settled(next)) {
                let snap = nearest(rotation, &snaps(panel(next), self.angle))?;
                self.rest_face = snap.face;
                return Some((snap.rotation, false));
            }
            return None;
        }
        self.view_orientation = Some(next.orientation.clone());
        let pose_angle = self.presentation_angle;
        let fold = (180. - pose_angle) * PI / 360.;
        let mut roll = if self.physical_pose == "closed" { 0. } else { FRAC_PI_2 };
        if next.width < next.height {
            match next.orientation.as_str() {
                "landscape_left" => roll -= FRAC_PI_2,
                "landscape_right" => roll += FRAC_PI_2,
                _ => {}
            }
        }
        if next.orientation == "portrait_upside_down" {
            roll -= PI;
        }
        self.presentation = match self.physical_pose {
            "laptop" => motion::euler_yxz(-fold + PI / 9., -PI / 9., FRAC_PI_2),
            "tent" => motion::euler_yxz(FRAC_PI_2 + PI / 18., -PI / 9., -FRAC_PI_2),
            _ => motion::mul(motion::axis_angle([0., 0., 1.], roll), motion::euler_yxz(0., FRAC_PI_2 * (1. - pose_angle / 180.).powi(3), 0.)),
        };
        if first {
            self.angle = self.target;
        }
        // Closed or open on its cover, the Duo faces the cover's actual hinged normal; a tent must
        // stand on both edges, cover outward.
        if cover
            && !landed_fold
            && self.physical_pose != "tent"
            && let Some(snap) = nearest(self.presentation, &snaps(1, self.angle))
        {
            self.presentation = snap.rotation;
        }
        self.rest_face = if cover {
            RestFace::Cover
        } else if self.physical_pose == "laptop" {
            RestFace::Right
        } else {
            RestFace::Inside
        };
        Some((self.presentation, first))
    }

    /// Web `choose`'s requests when a turn comes to rest on `snap`: the device asked to hand off
    /// to the display the snap faces, or else to rotate to the snap's orientation.
    pub fn request(&mut self, snap: &Snap, screen: &Screen, now: f64) -> Option<Value> {
        self.rest_face = snap.face;
        let panel = if snap.face == RestFace::Cover { 1 } else { 3 };
        if screen.supports_physical && Some(panel) != self.requested_panel.or(screen.screen_id) {
            self.requested_panel = Some(panel);
            self.handoff_until = Some(now + HANDOFF_MS);
            self.touch = None;
            return Some(json!({"control": "physical", "value": if panel == 1 { "facedown" } else { "faceup" }}));
        }
        if self.requested_panel.is_none() && self.view_orientation.as_deref() != Some(snap.orientation) {
            self.view_orientation = Some(snap.orientation.into());
            self.requested_orientation = true;
            return Some(json!({"control": "orientation", "value": snap.orientation}));
        }
        None
    }

    /// Web `resetPose`: the rest view for the rotation shown among `active` (else the
    /// presentation), and the orientation request it makes.
    pub fn reset(&mut self, screen: Option<&Screen>, active: &[Snap]) -> (Quat, Option<Value>) {
        self.clear_handoff();
        self.requested_orientation = false;
        let Some(snap) = active.iter().find(|s| screen.is_some_and(|screen| screen.orientation == s.orientation)) else {
            return (self.presentation, None);
        };
        let mut command = None;
        if self.view_orientation.as_deref() != Some(snap.orientation) {
            self.view_orientation = Some(snap.orientation.into());
            self.requested_orientation = true;
            command = Some(json!({"control": "orientation", "value": snap.orientation}));
        }
        (snap.rotation, command)
    }

    /// When a requested handoff gives up.
    pub fn handoff_deadline(&self) -> Option<f64> {
        self.handoff_until
    }

    /// Gives up on a handoff unanswered past its deadline; true when it did, and the view should
    /// settle on the display shown.
    pub fn expire(&mut self, now: f64) -> bool {
        let late = self.handoff_until.is_some_and(|at| now >= at);
        if late {
            self.clear_handoff();
        }
        late
    }

    fn clear_handoff(&mut self) {
        (self.requested_panel, self.handoff_until) = (None, None);
    }

    /// Eases the shown hinge angle toward the reported one at `now` milliseconds, as web's
    /// render loop does; true when it moved.
    pub fn ease(&mut self, now: f64) -> bool {
        let seconds = self.at.map_or(0.016, |at| ((now - at) / 1000.).max(0.) as f32);
        self.at = Some(now);
        if !self.moving() {
            return false;
        }
        let amount = 1. - (-14. * if seconds > 0. { seconds } else { 0.016 }).exp();
        self.angle += (self.target - self.angle) * amount;
        if (self.angle - self.target).abs() <= 0.01 {
            self.angle = self.target;
        }
        true
    }

    pub fn moving(&self) -> bool {
        (self.angle - self.target).abs() > 0.01
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(id: u32, angle: f64, pose: Option<&str>, orientation: &str) -> Screen {
        Screen { width: 1320., height: 2868., orientation: orientation.into(), screen_id: Some(id), supports_hinge: true, supports_physical: true, hinge_angle: Some(angle), hinge_pose: pose.map(Into::into) }
    }

    fn none(_: u8, _: f32) -> Vec<Snap> {
        vec![]
    }

    #[test]
    fn the_duo_presents_each_stand_and_eases_its_hinge() {
        let mut view = DuoView::default();
        let (laptop, now) = view.screen(&screen(3, 110., Some("laptop"), "landscape_left"), motion::IDENTITY, &none).unwrap();
        assert!(now && view.angle == 110., "the first readout poses the Duo at once");
        assert_eq!(laptop, motion::euler_yxz(-(70. * PI / 360.) + PI / 9., -PI / 9., FRAC_PI_2));
        assert_eq!(view.screen(&screen(3, 110., Some("laptop"), "landscape_left"), motion::IDENTITY, &none), None, "an unchanged readout leaves the view");
        // Folding: the hinge eases to the new angle, and the view springs to the open pose.
        let (_, now) = view.screen(&screen(3, 40., None, "landscape_left"), motion::IDENTITY, &none).unwrap();
        assert!(!now && view.moving());
        let mut t = 0.;
        let mut frames = 0;
        while view.ease(t) {
            t += 16.;
            frames += 1;
        }
        assert!(frames > 5 && view.angle == 40., "the hinge eases over frames: {frames}");
        let (closed, now) = view.screen(&screen(1, 0., None, "portrait"), motion::IDENTITY, &none).unwrap();
        assert!(!now, "a landed fold springs to its pose");
        assert!(motion::angle_to(closed, motion::euler_yxz(0., FRAC_PI_2, 0.)) < 1e-4, "closed, the cover turns to the camera");
        // A plain rotation while open rolls the view.
        let mut view = DuoView::default();
        view.screen(&screen(3, 180., None, "portrait"), motion::IDENTITY, &none);
        let (rolled, _) = view.screen(&screen(3, 180., None, "landscape_left"), motion::IDENTITY, &none).unwrap();
        assert!(motion::angle_to(rolled, motion::axis_angle([0., 0., 1.], 0.)) < 1e-4, "{rolled:?}");
        assert!(settled(&screen(1, 0., None, "portrait")) && !settled(&screen(1, 30., None, "portrait")));
    }

    #[test]
    fn a_turned_duo_rests_facing_a_display_and_asks_for_it() {
        let cover = snaps(&[(RestFace::Cover, [0., 0., 1.], [0., 1., 0.])], 1);
        assert_eq!(cover.iter().map(|s| s.orientation).collect::<Vec<_>>(), ORIENTATIONS);
        assert!(motion::angle_to(cover[0].rotation, motion::IDENTITY) < 1e-5 && cover[0].yaw_limit == PI / 9.);
        let tilted = motion::mul(motion::axis_angle([0., 1., 0.], 0.1), motion::axis_angle([0., 0., 1.], -1.4));
        let snap = nearest(tilted, &cover).unwrap();
        assert_eq!(snap.orientation, "landscape_left", "a quarter roll rests on the landscape view");
        assert!((motion::angle_to(snap.rotation, cover[1].rotation) - 0.1).abs() < 1e-3, "keeping its small turn");
        // Half open, the inner panel's views never show a leaf edge on, and its leaves rest alone.
        let (s, c) = (35f32.to_radians().sin(), 35f32.to_radians().cos());
        let frames = [(RestFace::Inside, [0., 0., 1.], [0., 1., 0.]), (RestFace::Left, [s, 0., c], [0., 1., 0.]), (RestFace::Right, [-s, 0., c], [0., 1., 0.])];
        let inner = snaps(&frames, 3);
        assert!(inner.iter().filter(|s| s.face == RestFace::Right).all(|s| s.orientation == "portrait"));
        for snap in &inner {
            assert!(frames[1..].iter().all(|f| motion::rotate(f.1, snap.rotation)[2] >= 0.04) && snap.yaw_limit <= PI / 3., "{snap:?}");
        }
        let landscape = inner.iter().find(|s| s.face == RestFace::Inside && s.orientation == "landscape_left").unwrap();
        assert!((landscape.yaw_limit - 52f32.to_radians()).abs() < 1e-4, "turning across the leaves stops before one shows edge on: {landscape:?}");
        // Coming to rest on the other display asks for it; until it shows, no rotation is asked.
        let mut view = DuoView::default();
        let open = screen(3, 180., None, "landscape_left");
        view.screen(&open, motion::IDENTITY, &none);
        assert_eq!(view.request(&cover[0], &open, 0.), Some(json!({"control": "physical", "value": "facedown"})));
        assert_eq!(view.request(&cover[1], &open, 1.), None);
        assert!(!view.expire(HANDOFF_MS - 1.) && view.expire(HANDOFF_MS));
        assert_eq!(view.request(&inner[0], &open, 0.), Some(json!({"control": "orientation", "value": inner[0].orientation})));
        // A rotation the view asked for doesn't re-present it.
        let turned = Screen { orientation: inner[0].orientation.into(), ..open.clone() };
        assert_eq!(view.screen(&turned, motion::IDENTITY, &none), None);
    }
}
