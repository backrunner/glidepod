//! Host points map through the actual painted surface, never through Retina pixels.
use crate::window_input::Action;
use glidepod_macos::gestures::GestureEvent;
use glidepod_runtime::input::InputEvent;
use gpui::{Bounds, Pixels, Point, px};

#[derive(Clone, Copy)]
pub struct Surface {
    pub bounds: Bounds<Pixels>,
    pub width: u32,
    pub height: u32,
}
impl Surface {
    pub fn point(self, p: Point<Pixels>, clamp: bool) -> Option<(f32, f32)> {
        if !f32::from(p.x).is_finite()
            || !f32::from(p.y).is_finite()
            || (!clamp && !self.bounds.contains(&p))
            || self.bounds.size.width <= px(0.)
            || self.bounds.size.height <= px(0.)
            || self.width == 0
            || self.height == 0
        {
            return None;
        }
        let x = (p.x - self.bounds.origin.x) / self.bounds.size.width * self.width as f32;
        let y = (p.y - self.bounds.origin.y) / self.bounds.size.height * self.height as f32;
        Some(self.clamp((x, y)))
    }
    fn clamp(self, p: (f32, f32)) -> (f32, f32) {
        (
            p.0.clamp(0., (self.width - 1) as f32),
            p.1.clamp(0., (self.height - 1) as f32),
        )
    }
}
enum Gesture {
    Scroll {
        point: (f32, f32),
        down: bool,
    },
    Transform {
        center: (f32, f32),
        radius: f32,
        angle: f32,
        active: u32,
    },
}
#[derive(Default)]
pub struct TouchGestures {
    gesture: Option<Gesture>,
}
impl TouchGestures {
    pub fn clear(&mut self) {
        self.gesture = None;
    }
    pub fn cancel(&mut self) -> Vec<Action> {
        if self.gesture.take().is_some() {
            vec![Action::Device(InputEvent::CancelTouch)]
        } else {
            vec![]
        }
    }
    pub fn handle(&mut self, event: GestureEvent, surface: Surface) -> Vec<Action> {
        if event.kind == 4
            || ![event.x, event.y, event.dx, event.dy]
                .iter()
                .all(|v| v.is_finite())
        {
            return self.cancel();
        }
        if event.phase == 4 {
            // A late scroll cancellation must not terminate a newer pinch.
            let owns = match &self.gesture {
                Some(Gesture::Scroll { .. }) => event.kind == 1,
                Some(Gesture::Transform { active, .. }) => {
                    (2..=3).contains(&event.kind) && *active & (1 << event.kind) != 0
                }
                None => false,
            };
            return if owns { self.cancel() } else { vec![] };
        }
        if !(1..=3).contains(&event.kind) || !(1..=3).contains(&event.phase) {
            return vec![];
        }
        let mut result = vec![];
        if event.phase == 1 {
            let Some(p) = surface.point(gpui::point(px(event.x as f32), px(event.y as f32)), false)
            else {
                return vec![];
            };
            if event.kind == 1 {
                result.extend(self.cancel());
                self.gesture = Some(Gesture::Scroll {
                    point: p,
                    down: false,
                });
            } else if let Some(Gesture::Transform { active, .. }) = &mut self.gesture {
                *active |= 1 << event.kind;
            } else {
                result.extend(self.cancel());
                // Start above Android's minimum scaling span on the normal
                // 720px / 240dpi app display, so short pinches can be recognized.
                let radius = surface.width.min(surface.height) as f32 * 0.3;
                // Near an edge, move the gesture center inward enough to keep
                // both contacts on-screen and leave room to pinch outward.
                let inset = radius * 1.5;
                let center = (
                    p.0.clamp(inset, surface.width as f32 - 1. - inset),
                    p.1.clamp(inset, surface.height as f32 - 1. - inset),
                );
                self.gesture = Some(Gesture::Transform {
                    center,
                    radius,
                    angle: 0.,
                    active: 1 << event.kind,
                });
                result.push(Action::Device(transform(center, radius, 0.)));
            }
        }
        match &mut self.gesture {
            Some(Gesture::Scroll { point, down }) if event.kind == 1 => {
                if event.dx != 0. || event.dy != 0. {
                    if !*down {
                        result.push(Action::Device(contact(*point)));
                        *down = true;
                    }
                    // AppKit already applies the user's natural-scroll preference.
                    point.0 += event.dx as f32 * surface.width as f32
                        / f32::from(surface.bounds.size.width);
                    point.1 += event.dy as f32 * surface.height as f32
                        / f32::from(surface.bounds.size.height);
                    *point = surface.clamp(*point);
                    result.push(Action::Motion(contact(*point)));
                }
                if event.phase == 3 {
                    if *down {
                        result.push(Action::Device(InputEvent::Touch { points: vec![] }));
                    }
                    self.gesture = None;
                }
            }
            Some(Gesture::Transform {
                center,
                radius,
                angle,
                active,
            }) if *active & (1 << event.kind) != 0 => {
                if event.kind == 2 {
                    *radius = (*radius * (1. + event.dx as f32).max(0.01))
                        .clamp(2., max_radius(*center, surface));
                } else {
                    // AppKit positive rotation is counterclockwise; Android y points down.
                    *angle = (*angle - (event.dx as f32).to_radians()) % std::f32::consts::TAU;
                }
                if event.dx != 0. {
                    result.push(Action::Motion(transform(*center, *radius, *angle)));
                }
                if event.phase == 3 {
                    *active &= !(1 << event.kind);
                    if *active == 0 {
                        result.push(Action::Device(InputEvent::Touch { points: vec![] }));
                        self.gesture = None;
                    }
                }
            }
            _ => {}
        }
        result
    }
}
fn contact(p: (f32, f32)) -> InputEvent {
    InputEvent::Touch {
        points: vec![(0, p.0, p.1)],
    }
}
fn max_radius(center: (f32, f32), surface: Surface) -> f32 {
    center
        .0
        .min(center.1)
        .min(surface.width as f32 - 1. - center.0)
        .min(surface.height as f32 - 1. - center.1)
}
fn transform(center: (f32, f32), radius: f32, angle: f32) -> InputEvent {
    let dx = radius * angle.cos();
    let dy = radius * angle.sin();
    InputEvent::Touch {
        points: vec![
            (0, center.0 - dx, center.1 - dy),
            (1, center.0 + dx, center.1 + dy),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, size};
    fn surface() -> Surface {
        Surface {
            bounds: Bounds::new(point(px(100.), px(50.)), size(px(360.), px(540.))),
            width: 720,
            height: 1080,
        }
    }
    fn event(kind: u32, phase: u32, dx: f64, dy: f64) -> GestureEvent {
        GestureEvent {
            kind,
            phase,
            x: 280.,
            y: 320.,
            dx,
            dy,
        }
    }
    fn points(action: &Action) -> &[(u32, f32, f32)] {
        match action {
            Action::Device(InputEvent::Touch { points })
            | Action::Motion(InputEvent::Touch { points }) => points,
            _ => panic!("Expected touch"),
        }
    }
    #[test]
    fn coordinates_follow_letterbox_resize_and_edges() {
        let mut s = surface();
        assert_eq!(
            s.point(point(px(280.), px(320.)), false),
            Some((360., 540.))
        );
        assert_eq!(s.point(point(px(99.), px(320.)), false), None);
        assert_eq!(s.point(point(px(900.), px(-10.)), true), Some((719., 0.)));
        s.bounds.size = size(px(180.), px(270.));
        assert_eq!(
            s.point(point(px(190.), px(185.)), false),
            Some((360., 540.))
        );
        assert_eq!(s.point(point(px(f32::NAN), px(0.)), true), None);
        s.bounds.size.width = px(0.);
        assert_eq!(s.point(point(px(100.), px(50.)), true), None);
    }
    #[test]
    fn scroll_preserves_anchor_direction_final_delta_and_does_not_tap_without_motion() {
        let mut g = TouchGestures::default();
        assert!(g.handle(event(1, 1, 0., 0.), surface()).is_empty());
        let a = g.handle(event(1, 2, 5., -10.), surface());
        assert_eq!(points(&a[0]), [(0, 360., 540.)]);
        assert_eq!(points(&a[1]), [(0, 370., 520.)]);
        let a = g.handle(event(1, 3, 0., -5.), surface());
        assert_eq!(points(&a[0]), [(0, 370., 510.)]);
        assert!(points(&a[1]).is_empty());
        assert!(g.handle(event(1, 2, 5., 10.), surface()).is_empty());
        g.handle(event(1, 1, 0., 0.), surface());
        assert!(g.handle(event(1, 3, 0., 0.), surface()).is_empty());
    }
    #[test]
    fn pinch_and_rotation_share_contacts_until_both_end() {
        let mut g = TouchGestures::default();
        let a = g.handle(event(2, 1, 0.25, 0.), surface());
        assert_eq!(points(&a[0]).len(), 2);
        let span = points(&a[1])[1].1 - points(&a[1])[0].1;
        assert!((span - 540.).abs() < 0.01);
        let a = g.handle(event(3, 1, 90., 0.), surface());
        let p = points(&a[0]);
        assert!((p[0].1 - p[1].1).abs() < 0.01);
        assert!(p[0].2 > p[1].2);
        assert!(g.handle(event(2, 3, 0., 0.), surface()).is_empty());
        assert!(points(&g.handle(event(3, 3, 0., 0.), surface())[0]).is_empty());
    }
    #[test]
    fn boundaries_cancel_and_focus_reset_do_not_leave_contacts() {
        let mut g = TouchGestures::default();
        let mut e = event(2, 1, 500., 0.);
        e.x = 100.;
        e.y = 50.;
        let actions = g.handle(e, surface());
        assert!((points(&actions[0])[1].1 - points(&actions[0])[0].1 - 432.).abs() < 0.01);
        assert!(points(&actions[1])[1].1 - points(&actions[1])[0].1 > 432.);
        for action in actions {
            match action {
                Action::Device(e) | Action::Motion(e) => e.validate(720, 1080).unwrap(),
                _ => unreachable!(),
            }
        }
        assert!(matches!(
            g.handle(event(2, 4, 0., 0.), surface()).as_slice(),
            [Action::Device(InputEvent::CancelTouch)]
        ));
        g.handle(event(2, 1, 0., 0.), surface());
        assert!(g.handle(event(1, 4, 0., 0.), surface()).is_empty());
        assert_eq!(g.handle(event(2, 2, 0.1, 0.), surface()).len(), 1);
        g.clear();
        assert!(g.handle(event(2, 2, 0.1, 0.), surface()).is_empty());
        assert!(g.handle(event(4, 4, 0., 0.), surface()).is_empty());
    }

    #[test]
    #[ignore = "requires GLIDEPOD_TEST_STORE and a running disposable Android VM"]
    fn gestures_reach_android_and_hit_targets() -> anyhow::Result<()> {
        use anyhow::{Context, ensure};
        use glidepod_runtime::Store;
        use std::{fs, path::PathBuf, sync::Arc, thread, time::Duration};
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()?;
        let data =
            PathBuf::from(std::env::var_os("GLIDEPOD_TEST_STORE").context("Missing test store")?)
                .canonicalize()?;
        ensure!(
            data.starts_with(root.join(".cache")),
            "Use a disposable store"
        );
        let guest = Store::new(data)?.guest()?;
        guest.install(&root.join("target/android/glidepod-fixture.apk"), None)?;
        guest.developer_shell("am force-stop dev.glidepod.fixture; su 0 rm -f /data/user/0/dev.glidepod.fixture/files/events.jsonl /data/user/0/dev.glidepod.fixture/files/clicks.json /data/user/0/dev.glidepod.fixture/files/scale.json", false)?;
        let app = Arc::new(guest.open_app("dev.glidepod.fixture", 720, 1080)?);
        let mut input = app.open_input()?;
        input.send(InputEvent::Focus)?;
        thread::sleep(Duration::from_millis(300));
        let read = |name: &str| -> anyhow::Result<String> {
            guest.developer_shell(
                &format!("su 0 cat /data/user/0/dev.glidepod.fixture/files/{name}"),
                false,
            )
        };
        let target: serde_json::Value = serde_json::from_str(&read("target.json")?)?;
        let x = target["x"].as_f64().context("Target x")? as f32;
        let y = target["y"].as_f64().context("Target y")? as f32;
        // Three native-point layouts (including letterboxing and fractional scale)
        // must hit the same actual Android button, not just serialize valid points.
        for (i, (origin, dimensions)) in [
            ((100., 50.), (360., 540.)),
            ((13.5, 70.25), (240., 360.)),
            ((200., 0.), (600., 900.)),
        ]
        .into_iter()
        .enumerate()
        {
            let s = Surface {
                bounds: Bounds::new(
                    point(px(origin.0), px(origin.1)),
                    size(px(dimensions.0), px(dimensions.1)),
                ),
                ..surface()
            };
            let host = point(
                px(origin.0 + x / 720. * dimensions.0),
                px(origin.1 + y / 1080. * dimensions.1),
            );
            let p = s.point(host, false).unwrap();
            input.send(contact(p))?;
            input.send(InputEvent::Touch { points: vec![] })?;
            thread::sleep(Duration::from_millis(100));
            let clicks: serde_json::Value = serde_json::from_str(&read("clicks.json")?)?;
            ensure!(
                clicks["clicks"] == i + 1,
                "Mapped click missed Android target: {clicks}"
            );
        }
        let mut g = TouchGestures::default();
        for e in [
            event(1, 1, 0., 0.),
            event(1, 2, 5., -10.),
            event(1, 3, 0., -5.),
            event(2, 1, 0., 0.),
            event(2, 2, 0.25, 0.),
            event(2, 2, 0.25, 0.),
            event(3, 1, 90., 0.),
            event(2, 3, 0., 0.),
            event(3, 3, 0., 0.),
        ] {
            for a in g.handle(e, surface()) {
                if let Action::Device(e) | Action::Motion(e) = a {
                    input.send(e)?;
                }
            }
            thread::sleep(Duration::from_millis(20));
        }
        // A cancelled gesture must not release a concurrently held game key.
        input.send(InputEvent::Key {
            code: 51,
            down: true,
            meta: 0,
        })?;
        input.send(contact((300., 600.)))?;
        input.send(InputEvent::CancelTouch)?;
        thread::sleep(Duration::from_millis(100));
        let log = read("events.jsonl")?;
        let events: Vec<serde_json::Value> = log
            .lines()
            .map(serde_json::from_str)
            .collect::<Result<_, _>>()?;
        ensure!(
            events.iter().all(|e| e["display"] == app.info.display_id),
            "Gesture leaked displays"
        );
        ensure!(
            events
                .iter()
                .any(|e| e["action"] == 3 && e["kind"] == "touch"),
            "Cancel missing"
        );
        ensure!(
            !events
                .iter()
                .any(|e| e["kind"] == "key" && e["key"] == 51 && e["action"] == 1),
            "Touch cancel released held game key"
        );
        ensure!(
            events.iter().any(|e| e["action"] == 1
                && e["contacts"][0][1] == 370.
                && e["contacts"][0][2] == 510.),
            "Scroll final coordinates changed"
        );
        let transformed = events
            .iter()
            .find(|e| {
                e["action"] == 2
                    && e["pointers"] == 2
                    && (e["contacts"][0][1].as_f64().unwrap_or(0.) - 360.).abs() < 0.01
            })
            .context("Rotation missing")?;
        ensure!(
            (transformed["contacts"][0][2].as_f64().unwrap() - 877.5).abs() < 0.1,
            "Rotation changed anchor or span"
        );
        let scale: serde_json::Value = serde_json::from_str(&read("scale.json")?)?;
        ensure!(
            scale["span"].as_f64().unwrap_or(0.) >= 540.
                && scale["total_scale"].as_f64().unwrap_or(0.) > 1.1,
            "Android ScaleGestureDetector did not recognize pinch"
        );
        input.send(InputEvent::Key {
            code: 51,
            down: false,
            meta: 0,
        })?;
        let shrink = std::iter::once(event(2, 1, 0., 0.))
            .chain((0..8).map(|_| event(2, 2, -0.02, 0.)))
            .chain(std::iter::once(event(2, 3, 0., 0.)));
        for e in shrink {
            for a in g.handle(e, surface()) {
                if let Action::Device(e) | Action::Motion(e) = a {
                    input.send(e)?;
                }
            }
            thread::sleep(Duration::from_millis(20));
        }
        let shrink: serde_json::Value = serde_json::from_str(&read("scale.json")?)?;
        ensure!(
            shrink["total_scale"].as_f64().unwrap_or(1.) < 0.99,
            "Android did not recognize inward pinch: {shrink}; configuration: {target}"
        );
        for mut e in [
            event(2, 1, 0., 0.),
            event(2, 2, 0.25, 0.),
            event(2, 2, 0.25, 0.),
            event(2, 3, 0., 0.),
        ] {
            e.x = 100.;
            e.y = 50.;
            for a in g.handle(e, surface()) {
                if let Action::Device(e) | Action::Motion(e) = a {
                    input.send(e)?;
                }
            }
            thread::sleep(Duration::from_millis(20));
        }
        let edge: serde_json::Value = serde_json::from_str(&read("scale.json")?)?;
        ensure!(
            edge["total_scale"].as_f64().unwrap_or(1.) > 1.1,
            "Pinch at the surface edge did not zoom: {edge}"
        );
        let out = root.join("evidence/input-gestures-20261107");
        fs::create_dir_all(&out)?;
        fs::write(out.join("guest-events.jsonl"), read("events.jsonl")?)?;
        fs::write(
            out.join("report.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "passed":true,"epoch":guest.endpoints.epoch,"display":app.info,"mapped_button_clicks":3,
                "scale_detector":{"outward":scale,"inward":shrink,"edge":edge},"android_target":target,"checks":["letterboxed and fractional scale target hits","scroll final position","pinch in both directions and at an edge recognized by Android","combined pinch and rotation","touch cancellation preserves held game key","display routing"],
                "native_hardware_verified":false
            }))?,
        )?;
        Ok(())
    }
}
