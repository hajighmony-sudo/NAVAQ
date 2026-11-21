//! Turns `SpatialSettings` into concrete rendering parameters: per-speaker distance, delay,
//! gain and direction, first-order wall reflections, interaural time differences and the
//! room's reverberation time (Sabine) including furniture absorption.
//!
//! Coordinates: x is lateral (+ right), y is up from the floor, z is forward. The room is
//! centred on x and z (`x` in ±width/2, `z` in ±depth/2) with the floor at y = 0. Speaker and
//! listener positions are in this room frame.

use crate::spatial::{SpatialError, SpatialSettings, SpeakerLayout, SpeakerPosition};

pub const SPEED_OF_SOUND_M_S: f32 = 343.0;
/// Average head radius used for interaural time differences.
const HEAD_RADIUS_M: f32 = 0.0875;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    LeftWall,
    RightWall,
    FrontWall,
    BackWall,
    Floor,
    Ceiling,
}

/// Rendering parameters for one virtual speaker.
#[derive(Debug, Clone, PartialEq)]
pub struct SpeakerFeed {
    pub name: String,
    /// 0 = straight ahead, positive to the right, in degrees (-180..=180).
    pub azimuth_deg: f32,
    pub elevation_deg: f32,
    pub distance_m: f32,
    /// Propagation delay from the speaker to the listener.
    pub delay_ms: f32,
    /// Extra delay that time-aligns this speaker with the farthest one (0 for the farthest).
    pub alignment_delay_ms: f32,
    /// Physical level at the listener relative to 1 m (capped at 1).
    pub gain: f32,
    /// Level trim so that near speakers match the farthest one (farthest = 1).
    pub compensation_gain: f32,
    /// Interaural time difference in ms; positive when the right ear hears it first.
    pub itd_ms: f32,
}

/// A first-order reflection of one speaker off one surface.
#[derive(Debug, Clone, PartialEq)]
pub struct Reflection {
    pub speaker_index: usize,
    pub surface: Surface,
    /// Arrival time after the direct sound.
    pub delay_ms: f32,
    /// Level relative to the direct sound.
    pub gain: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpatialRendering {
    pub speakers: Vec<SpeakerFeed>,
    pub reflections: Vec<Reflection>,
    /// Reverberation time (Sabine), seconds.
    pub rt60_s: f32,
    /// Distance at which direct and reverberant sound have equal level.
    pub critical_distance_m: f32,
    /// Total absorption of the room in metric sabins (surfaces plus furniture).
    pub absorption_sabins: f32,
    pub reverb_mix: f32,
}

fn speaker_positions(layout: &SpeakerLayout) -> Vec<(String, SpeakerPosition)> {
    match layout {
        SpeakerLayout::Stereo { left, right } => {
            vec![("L".into(), *left), ("R".into(), *right)]
        }
        SpeakerLayout::Surround5_1 {
            front_left,
            front_right,
            center,
            surround_left,
            surround_right,
        } => vec![
            ("FL".into(), *front_left),
            ("FR".into(), *front_right),
            ("C".into(), *center),
            ("SL".into(), *surround_left),
            ("SR".into(), *surround_right),
        ],
        SpeakerLayout::Custom { speakers } => speakers
            .iter()
            .enumerate()
            .map(|(i, p)| (format!("S{}", i + 1), *p))
            .collect(),
    }
}

fn distance(a: SpeakerPosition, b: SpeakerPosition) -> f32 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2) + (a.z - b.z).powi(2)).sqrt()
}

/// Reverberation time by Sabine's formula: `0.161 * V / A`, clamped to 0.05..=5 s.
pub fn reverberation_time(settings: &SpatialSettings) -> (f32, f32) {
    let room = settings.room;
    let walls = 2.0 * room.height * (room.width + room.depth);
    let floor_or_ceiling = room.width * room.depth;
    let surfaces = walls * settings.absorption.walls
        + floor_or_ceiling * settings.absorption.floor
        + floor_or_ceiling * settings.absorption.ceiling;
    let furniture: f32 = settings
        .furniture
        .iter()
        .map(|item| item.kind.sabins() * item.count as f32)
        .sum();
    let total = (surfaces + furniture).max(0.01);
    ((0.161 * room.volume() / total).clamp(0.05, 5.0), total)
}

fn inside_room(settings: &SpatialSettings, p: SpeakerPosition) -> bool {
    const EPS: f32 = 1.0e-3;
    let room = settings.room;
    p.x.abs() <= room.width / 2.0 + EPS
        && p.z.abs() <= room.depth / 2.0 + EPS
        && p.y >= -EPS
        && p.y <= room.height + EPS
}

/// Compute rendering parameters. Fails if the settings are invalid or the listener or any
/// speaker lies outside the room.
pub fn render(settings: &SpatialSettings) -> Result<SpatialRendering, SpatialError> {
    if !settings.is_valid() {
        return Err(SpatialError::Invalid);
    }
    let listener = settings.listener;
    let positions = speaker_positions(&settings.layout);
    if !inside_room(settings, listener) || positions.iter().any(|(_, p)| !inside_room(settings, *p))
    {
        return Err(SpatialError::OutsideRoom);
    }

    let distances: Vec<f32> = positions
        .iter()
        .map(|(_, p)| distance(*p, listener).max(0.1))
        .collect();
    let farthest = distances.iter().cloned().fold(0.0_f32, f32::max);

    let speakers: Vec<SpeakerFeed> = positions
        .iter()
        .zip(&distances)
        .map(|((name, p), &d)| {
            let (dx, dy, dz) = (p.x - listener.x, p.y - listener.y, p.z - listener.z);
            let horizontal = (dx * dx + dz * dz).sqrt();
            let lateral = (dx / d).clamp(-1.0, 1.0).asin();
            SpeakerFeed {
                name: name.clone(),
                azimuth_deg: dx.atan2(dz).to_degrees(),
                elevation_deg: dy.atan2(horizontal).to_degrees(),
                distance_m: d,
                delay_ms: d / SPEED_OF_SOUND_M_S * 1000.0,
                alignment_delay_ms: (farthest - d) / SPEED_OF_SOUND_M_S * 1000.0,
                gain: (1.0 / d).min(1.0),
                compensation_gain: d / farthest,
                itd_ms: HEAD_RADIUS_M / SPEED_OF_SOUND_M_S * (lateral + lateral.sin()) * 1000.0,
            }
        })
        .collect();

    // First-order image sources for the six room surfaces.
    let room = settings.room;
    let a = settings.absorption;
    let mut reflections = Vec::with_capacity(positions.len() * 6);
    for (index, ((_, p), &direct)) in positions.iter().zip(&distances).enumerate() {
        let images = [
            (
                Surface::LeftWall,
                SpeakerPosition {
                    x: -room.width - p.x,
                    ..*p
                },
                a.walls,
            ),
            (
                Surface::RightWall,
                SpeakerPosition {
                    x: room.width - p.x,
                    ..*p
                },
                a.walls,
            ),
            (
                Surface::BackWall,
                SpeakerPosition {
                    z: -room.depth - p.z,
                    ..*p
                },
                a.walls,
            ),
            (
                Surface::FrontWall,
                SpeakerPosition {
                    z: room.depth - p.z,
                    ..*p
                },
                a.walls,
            ),
            (Surface::Floor, SpeakerPosition { y: -p.y, ..*p }, a.floor),
            (
                Surface::Ceiling,
                SpeakerPosition {
                    y: 2.0 * room.height - p.y,
                    ..*p
                },
                a.ceiling,
            ),
        ];
        for (surface, image, alpha) in images {
            let path = distance(image, listener).max(direct);
            reflections.push(Reflection {
                speaker_index: index,
                surface,
                delay_ms: (path - direct) / SPEED_OF_SOUND_M_S * 1000.0,
                gain: ((1.0 - alpha) * direct / path).clamp(0.0, 1.0),
            });
        }
    }

    let (rt60, absorption) = reverberation_time(settings);
    Ok(SpatialRendering {
        speakers,
        reflections,
        rt60_s: rt60,
        critical_distance_m: 0.057 * (room.volume() / rt60).sqrt(),
        absorption_sabins: absorption,
        reverb_mix: settings.reverb_mix,
    })
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;
    use crate::spatial::{FurnitureItem, FurnitureKind};

    #[test]
    fn default_room_has_the_sabine_reverberation_time() {
        let r = render(&SpatialSettings::default()).unwrap();
        assert!((r.rt60_s - 0.32).abs() < 0.01, "{}", r.rt60_s);
        assert!(r.critical_distance_m > 0.5);
    }

    #[test]
    fn furniture_shortens_reverberation_and_a_bare_room_rings_longer() {
        let bare = SpatialSettings::default();
        let mut furnished = bare.clone();
        furnished.furniture = vec![
            FurnitureItem {
                kind: FurnitureKind::Sofa,
                count: 2,
            },
            FurnitureItem {
                kind: FurnitureKind::Curtains,
                count: 2,
            },
            FurnitureItem {
                kind: FurnitureKind::Custom { sabins: 3.0 },
                count: 1,
            },
        ];
        let mut hard = bare.clone();
        hard.absorption.walls = 0.05;
        hard.absorption.floor = 0.05;
        hard.absorption.ceiling = 0.05;
        let (bare_rt, _) = reverberation_time(&bare);
        let (furnished_rt, furnished_a) = reverberation_time(&furnished);
        let (hard_rt, _) = reverberation_time(&hard);
        assert!(furnished_rt < bare_rt && bare_rt < hard_rt);
        assert!(furnished_a > 29.4 + 8.0);
    }

    #[test]
    fn symmetric_stereo_has_mirrored_directions_and_no_alignment_delay() {
        let r = render(&SpatialSettings::default()).unwrap();
        let (l, rt) = (&r.speakers[0], &r.speakers[1]);
        assert!(l.azimuth_deg < 0.0 && rt.azimuth_deg > 0.0);
        assert!((l.azimuth_deg + rt.azimuth_deg).abs() < 1e-3);
        assert!((l.itd_ms + rt.itd_ms).abs() < 1e-6);
        assert!(rt.itd_ms > 0.0);
        assert!(l.alignment_delay_ms.abs() < 1e-4 && rt.alignment_delay_ms.abs() < 1e-4);
        assert!((l.distance_m - 2.773).abs() < 0.01);
        assert!((l.delay_ms - 2.773 / 343.0 * 1000.0).abs() < 0.05);
    }

    #[test]
    fn moving_toward_one_speaker_delays_and_trims_it() {
        let mut s = SpatialSettings::default();
        s.listener.x = 1.0; // nearer to the right speaker
        let r = render(&s).unwrap();
        let (l, rt) = (&r.speakers[0], &r.speakers[1]);
        assert!(rt.distance_m < l.distance_m);
        assert_eq!(l.alignment_delay_ms, 0.0);
        assert!(rt.alignment_delay_ms > 0.0);
        assert!(rt.compensation_gain < 1.0 && (l.compensation_gain - 1.0).abs() < 1e-6);
        assert!(rt.gain >= l.gain);
    }

    #[test]
    fn reflections_arrive_later_and_weaker_than_the_direct_sound() {
        let r = render(&SpatialSettings::default()).unwrap();
        assert_eq!(r.reflections.len(), 12);
        assert!(r
            .reflections
            .iter()
            .all(|x| x.delay_ms >= 0.0 && x.gain < 1.0));
        let ceiling = r
            .reflections
            .iter()
            .find(|x| x.speaker_index == 0 && x.surface == Surface::Ceiling)
            .unwrap();
        assert!(
            (ceiling.delay_ms - 5.67).abs() < 0.1,
            "{}",
            ceiling.delay_ms
        );
        // A more absorbent ceiling lowers the reflection.
        let mut soft = SpatialSettings::default();
        soft.absorption.ceiling = 0.9;
        let softer = render(&soft).unwrap();
        let soft_ceiling = softer
            .reflections
            .iter()
            .find(|x| x.speaker_index == 0 && x.surface == Surface::Ceiling)
            .unwrap();
        assert!(soft_ceiling.gain < ceiling.gain);
    }

    #[test]
    fn surround_layout_renders_five_speakers() {
        let p = |x, z| SpeakerPosition { x, y: 1.2, z };
        let mut s = SpatialSettings::default();
        s.layout = SpeakerLayout::Surround5_1 {
            front_left: p(-1.5, 2.0),
            front_right: p(1.5, 2.0),
            center: p(0.0, 2.2),
            surround_left: p(-1.8, -2.0),
            surround_right: p(1.8, -2.0),
        };
        let r = render(&s).unwrap();
        assert_eq!(r.speakers.len(), 5);
        assert_eq!(r.reflections.len(), 30);
        assert!(
            r.speakers[3].azimuth_deg.abs() > 90.0,
            "surrounds sit behind"
        );
        assert_eq!(r.speakers[2].name, "C");
    }

    #[test]
    fn rejects_speakers_outside_the_room_and_invalid_settings() {
        let mut s = SpatialSettings::default();
        s.layout = SpeakerLayout::Custom {
            speakers: vec![SpeakerPosition {
                x: 9.0,
                y: 1.0,
                z: 0.0,
            }],
        };
        assert_eq!(render(&s), Err(SpatialError::OutsideRoom));

        let mut bad = SpatialSettings::default();
        bad.reverb_mix = 3.0;
        assert_eq!(render(&bad), Err(SpatialError::Invalid));

        let mut crowded = SpatialSettings::default();
        crowded.furniture = vec![FurnitureItem {
            kind: FurnitureKind::Sofa,
            count: 500,
        }];
        assert_eq!(render(&crowded), Err(SpatialError::Invalid));
    }
}
