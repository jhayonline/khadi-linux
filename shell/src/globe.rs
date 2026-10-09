//! The rotating globe in the right column: dotted land on a wire sphere, with this
//! machine and the places it is connected to marked on it.

use crate::geo::{Place, Snapshot};
use crate::theme::Theme;
use crate::ui;
use eframe::egui::{Pos2, Sense, Stroke, Ui, pos2, vec2};

/// Land, as ranges of 10-degree cells of longitude (0 is 180°W) for each 10-degree band
/// of latitude, from the North Pole down. Coarse on purpose: enough to read as Earth.
const LAND: [&[(u8, u8)]; 18] = [
    &[(12, 14)],
    &[(7, 10), (12, 15), (19, 20), (26, 32)],
    &[(1, 8), (10, 11), (13, 14), (16, 16), (18, 35)],
    &[(2, 2), (5, 8), (10, 11), (17, 33)],
    &[(5, 11), (17, 31), (32, 32)],
    &[(6, 10), (17, 19), (21, 31)],
    &[(7, 8), (16, 30)],
    &[(8, 9), (16, 23), (25, 25), (27, 28), (30, 30)],
    &[(10, 12), (17, 22), (27, 29)],
    &[(10, 14), (19, 21), (28, 33)],
    &[(10, 13), (19, 22), (30, 32)],
    &[(11, 13), (19, 21), (29, 33)],
    &[(11, 12), (20, 20), (29, 29), (31, 33), (35, 35)],
    &[(11, 11), (32, 32), (35, 35)],
    &[(11, 11)],
    &[(11, 12), (20, 33)],
    &[(0, 35)],
    &[(0, 35)],
];

/// How far the globe leans towards the viewer, in degrees.
const TILT: f32 = 18.0;
/// Degrees of rotation per second.
pub const SPEED: f32 = 9.0;

/// Whether the 10-degree cell containing this position is land.
pub fn is_land(lon: f32, lat: f32) -> bool {
    let row = (((90.0 - lat) / 10.0) as usize).min(17);
    let cell = ((((lon + 180.0).rem_euclid(360.0)) / 10.0) as u8).min(35);
    LAND[row].iter().any(|(from, to)| (*from..=*to).contains(&cell))
}

/// How far a link's arc rises off the surface at its middle, in globe radii.
const LIFT: f32 = 0.22;

/// The point on the unit sphere for a position, before the globe is turned.
fn sphere(lon: f32, lat: f32) -> [f32; 3] {
    let (l, p) = (lon.to_radians(), lat.to_radians());
    [p.cos() * l.sin(), p.sin(), p.cos() * l.cos()]
}

/// Turns a point with the globe and leans it towards the viewer. Returns where it
/// lands on the unit disc, or `None` when it is on the far side.
fn view(point: [f32; 3], rotation: f32) -> Option<(f32, f32)> {
    let (r, t) = (rotation.to_radians(), TILT.to_radians());
    let [x, y, z] = point;
    let (x, z) = (x * r.cos() + z * r.sin(), z * r.cos() - x * r.sin());
    let depth = y * t.sin() + z * t.cos();
    (depth > 0.0).then_some((x, y * t.cos() - z * t.sin()))
}

/// Projects a position onto the unit disc for a globe turned by `rotation` degrees.
/// Returns `None` for the far side.
pub fn project(lon: f32, lat: f32, rotation: f32) -> Option<(f32, f32)> {
    view(sphere(lon, lat), rotation)
}

/// Points along the shortest path between two places, rising off the surface in the
/// middle like a trajectory.
fn arc(from: &Place, to: &Place) -> Vec<[f32; 3]> {
    let (a, b) = (sphere(from.lon, from.lat), sphere(to.lon, to.lat));
    let angle = (a[0] * b[0] + a[1] * b[1] + a[2] * b[2]).clamp(-1.0, 1.0).acos();
    // The same place, or its exact opposite: there is no single path to draw.
    if angle < 1e-3 || (std::f32::consts::PI - angle) < 1e-3 {
        return Vec::new();
    }
    (0..=32)
        .map(|step| {
            let t = step as f32 / 32.0;
            let (wa, wb) = (((1.0 - t) * angle).sin() / angle.sin(), (t * angle).sin() / angle.sin());
            let lift = 1.0 + LIFT * (std::f32::consts::PI * t).sin();
            [0, 1, 2].map(|i| (a[i] * wa + b[i] * wb) * lift)
        })
        .collect()
}

/// Draws the globe in a square of the given size, turned by `rotation` degrees, with
/// the places in `geo` marked.
pub fn show(ui: &mut Ui, theme: &Theme, size: f32, rotation: f32, geo: &Snapshot) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), size), Sense::hover());
    let center = rect.center();
    // Room around the sphere for the arcs that rise off it.
    let radius = (size / 2.0 - 4.0) / (1.0 + LIFT * 0.6);
    let painter = ui.painter_at(rect);
    let at = |(x, y): (f32, f32)| pos2(center.x + radius * x, center.y - radius * y);

    painter.circle_filled(center, radius, ui::GROUND);
    painter.circle_stroke(center, radius, Stroke::new(1.0, ui::LINE_STRONG));

    // The wire sphere: lines are broken where they pass behind the globe.
    let grid = Stroke::new(1.0, ui::LINE);
    let wire = |points: Vec<Option<Pos2>>| {
        for pair in points.windows(2) {
            if let [Some(a), Some(b)] = pair {
                painter.line_segment([*a, *b], grid);
            }
        }
    };
    for lon in (-180..180).step_by(30) {
        wire((-90..=90).step_by(6).map(|lat| project(lon as f32, lat as f32, rotation).map(at)).collect());
    }
    for lat in (-60..=60).step_by(30) {
        wire((-180..=180).step_by(6).map(|lon| project(lon as f32, lat as f32, rotation).map(at)).collect());
    }

    let land = theme.muted();
    for lat in (-875..900).step_by(50) {
        for lon in (-1775..1800).step_by(50) {
            let (lon, lat) = (lon as f32 / 10.0, lat as f32 / 10.0);
            if is_land(lon, lat) {
                if let Some(point) = project(lon, lat, rotation) {
                    painter.circle_filled(at(point), 1.1, land);
                }
            }
        }
    }

    // Links first, so that this machine's mark sits on top of where they meet.
    let link = Stroke::new(1.2, theme.main);
    for place in &geo.links {
        if let Some(home) = &geo.home {
            let points: Vec<Option<Pos2>> = arc(home, place).into_iter().map(|p| view(p, rotation).map(at)).collect();
            for pair in points.windows(2) {
                if let [Some(a), Some(b)] = pair {
                    painter.line_segment([*a, *b], link);
                }
            }
        }
        if let Some(point) = project(place.lon, place.lat, rotation) {
            painter.circle_filled(at(point), 2.6, theme.main);
        }
    }
    if let Some(point) = geo.home.as_ref().and_then(|home| project(home.lon, home.lat, rotation)) {
        painter.circle_filled(at(point), 3.4, ui::TEXT);
        painter.circle_stroke(at(point), 6.5, Stroke::new(1.0, ui::TEXT));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_land_from_sea() {
        assert!(is_land(10.0, 50.0), "central Europe");
        assert!(is_land(-100.0, 40.0), "North America");
        assert!(is_land(135.0, -25.0), "Australia");
        assert!(is_land(0.0, -85.0), "Antarctica");
        assert!(!is_land(-150.0, 0.0), "the Pacific");
        assert!(!is_land(-30.0, 30.0), "the Atlantic");
        assert!(!is_land(80.0, -30.0), "the Indian Ocean");
    }

    #[test]
    fn arcs_join_their_ends_and_rise_between() {
        let place = |lat, lon| Place { lat, lon, label: String::new() };
        let (from, to) = (place(0.0, 0.0), place(0.0, 90.0));
        let points = arc(&from, &to);
        let length = |p: &[f32; 3]| (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
        assert!((length(&points[0]) - 1.0).abs() < 1e-4);
        assert!((length(points.last().unwrap()) - 1.0).abs() < 1e-4);
        assert!((length(&points[16]) - (1.0 + LIFT)).abs() < 1e-3, "highest in the middle");
        assert!(arc(&from, &from).is_empty());
        assert!(arc(&from, &place(0.0, 180.0)).is_empty());
    }

    #[test]
    fn projects_the_near_side_only() {
        let (x, y) = project(0.0, TILT, 0.0).unwrap();
        assert!(x.abs() < 1e-5 && y.abs() < 1e-5, "the centre faces the viewer");
        assert!(project(180.0, 0.0, 0.0).is_none());
        assert!(project(0.0, 0.0, 180.0).is_none());
        let (x, _) = project(60.0, 0.0, 0.0).unwrap();
        assert!(x > 0.0 && x <= 1.0);
    }
}
