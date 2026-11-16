//! Bounded presentation geometry. The domain tree remains the only layout authority.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use iced::Rectangle;
use tervix_application::{LayoutGeometry, PaneId};

pub(super) const DURATION: Duration = Duration::from_millis(160);

#[derive(Clone, Default)]
pub(super) struct Snapshot {
    pub panes: HashMap<PaneId, Rectangle>,
}

impl Snapshot {
    pub fn measured(geometry: &LayoutGeometry, zoomed: Option<PaneId>) -> Self {
        let panes = geometry
            .panes()
            .iter()
            .map(|tile| {
                let rect = tile.rect;
                let bounds = if zoomed == Some(tile.pane.id) {
                    Rectangle {
                        x: 0.0,
                        y: 0.0,
                        width: geometry.viewport().width().max(300) as f32,
                        height: geometry.viewport().height().max(220) as f32,
                    }
                } else {
                    Rectangle {
                        x: rect.x as f32,
                        y: rect.y as f32,
                        width: rect.width as f32,
                        height: rect.height as f32,
                    }
                };
                (tile.pane.id, bounds)
            })
            .collect();
        Self { panes }
    }
}

pub(super) struct Motion {
    from: Snapshot,
    to: Snapshot,
    started: Instant,
    pub now: Instant,
}

impl Motion {
    pub fn new(from: Snapshot, to: Snapshot, now: Instant) -> Option<Self> {
        // Removed panes disappear immediately; new panes enter at their valid
        // destination size. Never manufacture zero-sized or session-less tiles.
        let changed = to
            .panes
            .iter()
            .any(|(id, rect)| from.panes.get(id).is_some_and(|previous| previous != rect));
        changed.then_some(Self {
            from,
            to,
            started: now,
            now,
        })
    }

    pub fn finished(&self) -> bool {
        self.now.saturating_duration_since(self.started) >= DURATION
    }

    pub fn sample(&self) -> Snapshot {
        Snapshot {
            panes: self
                .to
                .panes
                .keys()
                .filter_map(|id| self.rectangle(*id).map(|rect| (*id, rect)))
                .collect(),
        }
    }

    // Rendering asks for individual allocations instead of allocating a HashMap
    // on every frame. Full snapshots are needed only when retargeting.
    pub fn rectangle(&self, id: PaneId) -> Option<Rectangle> {
        let to = self.to.panes.get(&id)?;
        let from = self.from.panes.get(&id).unwrap_or(to);
        let t = (self
            .now
            .saturating_duration_since(self.started)
            .as_secs_f32()
            / DURATION.as_secs_f32())
        .clamp(0.0, 1.0);
        let t = t * t * (3.0 - 2.0 * t);
        let lerp = |a: f32, b: f32| a + (b - a) * t;
        Some(Rectangle {
            x: lerp(from.x, to.x),
            y: lerp(from.y, to.y),
            width: lerp(from.width, to.width),
            height: lerp(from.height, to.height),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn interpolation_retarget_minima_and_completion() -> Result<(), Box<dyn std::error::Error>> {
        let id = PaneId::from_uuid(Uuid::new_v4());
        let snapshot = |x, width| Snapshot {
            panes: HashMap::from([(
                id,
                Rectangle {
                    x,
                    y: 0.0,
                    width,
                    height: 220.0,
                },
            )]),
        };
        let now = Instant::now();
        let mut motion = Motion::new(snapshot(0.0, 900.0), snapshot(600.0, 300.0), now)
            .ok_or("different geometry must animate")?;
        assert_eq!(motion.sample().panes[&id].width, 900.0);
        motion.now = now + DURATION / 2;
        let midway = motion.sample();
        assert_eq!(midway.panes[&id].x, 300.0);
        assert_eq!(midway.panes[&id].width, 600.0);
        let mut retarget =
            Motion::new(midway.clone(), snapshot(0.0, 1200.0), motion.now).ok_or("retarget")?;
        assert_eq!(retarget.sample().panes, midway.panes);
        for step in 0..=20 {
            retarget.now = motion.now + DURATION * step / 20;
            let rect = retarget.sample().panes[&id];
            assert!(rect.width >= 300.0 && rect.height >= 220.0);
        }
        assert!(retarget.finished());
        assert_eq!(retarget.sample().panes[&id].width, 1200.0);
        Ok(())
    }

    #[test]
    fn topology_changes_keep_exact_target_ids_and_valid_new_pane_sizes()
    -> Result<(), Box<dyn std::error::Error>> {
        let first = PaneId::from_uuid(Uuid::new_v4());
        let second = PaneId::from_uuid(Uuid::new_v4());
        let rect = Rectangle {
            x: 0.0,
            y: 0.0,
            width: 600.0,
            height: 220.0,
        };
        let from = Snapshot {
            panes: HashMap::from([(first, rect)]),
        };
        let to = Snapshot {
            panes: HashMap::from([
                (
                    first,
                    Rectangle {
                        width: 300.0,
                        ..rect
                    },
                ),
                (
                    second,
                    Rectangle {
                        x: 300.0,
                        width: 300.0,
                        ..rect
                    },
                ),
            ]),
        };
        let motion = Motion::new(from, to.clone(), Instant::now()).ok_or("split")?;
        assert_eq!(motion.sample().panes.len(), 2);
        assert_eq!(motion.sample().panes[&second], to.panes[&second]);
        assert!(Motion::new(to.clone(), to, Instant::now()).is_none());
        Ok(())
    }
}
