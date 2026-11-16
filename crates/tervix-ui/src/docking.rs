//! Immutable pointer proposals; only release dispatches a layout command.
use crate::{i18n::Key as K, workspace::Workspace};
use iced::{Point, Rectangle};
use tervix_application::{Command, Direction, Layout, LayoutCommand, PaneId, Size};

pub(super) struct Drag {
    source: PaneId,
    origin: Layout,
    press: Point,
    started: bool,
    pub preview: Option<Preview>,
}

pub(super) struct Preview {
    pub command: LayoutCommand,
    pub rect: Rectangle,
    pub label: K,
}
impl Workspace {
    fn dock_blocked(&self) -> bool {
        self.launcher.is_some()
            || self.pending_close.is_some()
            || self.pending_paste.is_some()
            || self.chrome.pending_exit
            || self.chrome.shutting_down
            || self
                .docker
                .as_ref()
                .is_some_and(|d| d.confirmation.is_some())
    }
    pub(super) fn start_dock(&mut self, source: PaneId) {
        if self.dock_blocked()
            || !self.configuring
            || self.zoomed.is_some()
            || self.geometry_motion.is_some()
            || self.controller.resize_active()
            || self.docking.is_some()
            || !self
                .controller
                .layout()
                .panes()
                .iter()
                .any(|p| p.id == source)
        {
            return;
        }
        if let Some(press) = self.dock_point {
            let valid_handle = self.geometry().is_ok_and(|geometry| {
                geometry
                    .panes()
                    .iter()
                    .find(|p| p.pane.id == source)
                    .is_some_and(|p| {
                        Rectangle {
                            x: p.rect.x as f32 + 19.0,
                            y: p.rect.y as f32 + 19.0,
                            width: 28.0,
                            height: 28.0,
                        }
                        .contains(press)
                    })
            });
            if !valid_handle {
                return;
            }
            self.docking = Some(Drag {
                source,
                origin: self.controller.layout().clone(),
                press,
                started: false,
                preview: None,
            });
            self.status = K::DockInvalid.into();
        }
    }
    pub(super) fn dock_pointer(&mut self, point: Point) {
        self.dock_point = Some(point);
        let Some(mut drag) = self.docking.take() else {
            return;
        };
        if self.dock_blocked()
            || self.controller.layout() != &drag.origin
            || !self.configuring
            || self.zoomed.is_some()
            || self.geometry_motion.is_some()
        {
            return;
        }
        drag.started |= point.distance(drag.press) >= 6.0;
        drag.preview = if drag.started {
            self.dock_proposal(&drag, point)
        } else {
            None
        };
        self.status = if drag.preview.is_some() {
            K::DockHint
        } else {
            K::DockInvalid
        }
        .into();
        self.docking = Some(drag);
    }
    fn dock_proposal(&self, drag: &Drag, point: Point) -> Option<Preview> {
        let geometry = self.geometry().ok()?;
        let target = geometry.panes().iter().find(|tile| {
            Rectangle {
                x: tile.rect.x as f32,
                y: tile.rect.y as f32,
                width: tile.rect.width as f32,
                height: tile.rect.height as f32,
            }
            .contains(point)
        })?;
        if target.pane.id == drag.source {
            return None;
        }
        let r = target.rect;
        let x = (point.x - r.x as f32) / r.width as f32;
        let y = (point.y - r.y as f32) / r.height as f32;
        let side = [
            (x, Direction::Left),
            (1.0 - x, Direction::Right),
            (y, Direction::Up),
            (1.0 - y, Direction::Down),
        ]
        .into_iter()
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .filter(|(d, _)| *d < 0.25)
        .map(|(_, s)| s);
        let command = match side {
            Some(side) => LayoutCommand::MovePane {
                source: drag.source,
                target: target.pane.id,
                side,
                viewport: self.viewport().ok()?,
                minimum: Size::new(300, 220).ok()?,
            },
            None => LayoutCommand::Swap(drag.source, target.pane.id),
        };
        let proposed = drag.origin.apply(command).ok()?;
        if proposed == drag.origin {
            return None;
        }
        let measured = proposed
            .measure(self.viewport().ok()?, Size::new(300, 220).ok()?)
            .ok()?;
        let rect = measured
            .panes()
            .iter()
            .find(|p| p.pane.id == drag.source)?
            .rect;
        Some(Preview {
            command,
            rect: Rectangle {
                x: rect.x as f32,
                y: rect.y as f32,
                width: rect.width as f32,
                height: rect.height as f32,
            },
            label: match side {
                None => K::DockSwap,
                Some(Direction::Left) => K::DockLeft,
                Some(Direction::Right) => K::DockRight,
                Some(Direction::Up) => K::DockUp,
                Some(Direction::Down) => K::DockDown,
            },
        })
    }
    pub(super) fn release_dock(&mut self) {
        let Some(drag) = self.docking.take() else {
            return;
        };
        if self.dock_blocked() || !self.configuring || self.controller.layout() != &drag.origin {
            return;
        }
        let Some(preview) = drag.preview else {
            return;
        };
        let before = self.displayed_geometry();
        if self
            .controller
            .dispatch(Command::Layout(preview.command))
            .is_ok()
        {
            self.status = K::LayoutUpdated.into();
            self.retarget_geometry(before);
        }
    }
}

#[cfg(test)]
#[path = "docking_tests.rs"]
mod tests;
