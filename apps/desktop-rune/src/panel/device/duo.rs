//! Web `duoViewer.ts` for the iPhone Duo's 3D view: the hinge eased to the angle the device
//! reports, and the pose each stand, fold and rotation presents the Duo in.

use std::f32::consts::{FRAC_PI_2, PI};

use super::input::Screen;
use super::model::DuoTouch;
use super::motion::{self, Quat};

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
    /// Web `setScreen` for a new readout: the pose to turn to, and whether at once, when the
    /// readout changes how the Duo should be presented.
    pub fn screen(&mut self, next: &Screen) -> Option<(Quat, bool)> {
        if self.seen.as_ref() == Some(next) {
            return None;
        }
        let previous = self.seen.replace(next.clone());
        let first = previous.is_none();
        let cover = next.screen_id == Some(1);
        let changed_pose = next.hinge_pose.is_some() && next.hinge_pose != previous.as_ref().and_then(|p| p.hinge_pose.clone());
        let before = previous.as_ref().map(reported_angle);
        let after = reported_angle(next);
        if changed_pose {
            self.fold_pending = false;
        } else if before.is_some_and(|b| b != after) {
            self.fold_pending = true;
        }
        let landed_fold = self.fold_pending && settled(next);
        if settled(next) {
            self.fold_pending = false;
        }
        let rotated = !self.fold_pending
            && previous.as_ref().is_some_and(|p| p.screen_id == next.screen_id && p.hinge_angle == next.hinge_angle && p.orientation != next.orientation);
        let left_physical_pose = rotated && next.hinge_pose.is_none();
        if first || changed_pose || left_physical_pose || landed_fold {
            self.physical_pose = pose_name(next.hinge_pose.as_deref()).unwrap_or(if cover { "closed" } else { "open" });
            self.presentation_angle = after;
        }
        self.target = after;
        if !(first || changed_pose || landed_fold || rotated) {
            return None;
        }
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
        Some((self.presentation, first))
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
        Screen { width: 1320., height: 2868., orientation: orientation.into(), screen_id: Some(id), supports_hinge: true, hinge_angle: Some(angle), hinge_pose: pose.map(Into::into) }
    }

    #[test]
    fn the_duo_presents_each_stand_and_eases_its_hinge() {
        let mut view = DuoView::default();
        let (laptop, now) = view.screen(&screen(3, 110., Some("laptop"), "landscape_left")).unwrap();
        assert!(now && view.angle == 110., "the first readout poses the Duo at once");
        assert_eq!(laptop, motion::euler_yxz(-(70. * PI / 360.) + PI / 9., -PI / 9., FRAC_PI_2));
        assert_eq!(view.screen(&screen(3, 110., Some("laptop"), "landscape_left")), None, "an unchanged readout leaves the view");
        // Folding: the hinge eases to the new angle, and the view springs to the open pose.
        let (_, now) = view.screen(&screen(3, 40., None, "landscape_left")).unwrap();
        assert!(!now && view.moving());
        let mut t = 0.;
        let mut frames = 0;
        while view.ease(t) {
            t += 16.;
            frames += 1;
        }
        assert!(frames > 5 && view.angle == 40., "the hinge eases over frames: {frames}");
        let (closed, now) = view.screen(&screen(1, 0., None, "portrait")).unwrap();
        assert!(!now, "a landed fold springs to its pose");
        assert!(motion::angle_to(closed, motion::euler_yxz(0., FRAC_PI_2, 0.)) < 1e-4, "closed, the cover turns to the camera");
        // A plain rotation while open rolls the view.
        let mut view = DuoView::default();
        view.screen(&screen(3, 180., None, "portrait"));
        let (rolled, _) = view.screen(&screen(3, 180., None, "landscape_left")).unwrap();
        assert!(motion::angle_to(rolled, motion::axis_angle([0., 0., 1.], 0.)) < 1e-4, "{rolled:?}");
        assert!(settled(&screen(1, 0., None, "portrait")) && !settled(&screen(1, 30., None, "portrait")));
    }
}
