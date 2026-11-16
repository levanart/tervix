//! Original 24-unit outline geometry; no glyph fonts, embedded assets or OS access.
use crate::workspace::{Message, PaneView};
use iced::{Color, Element, Rectangle, Renderer, Theme, mouse, widget::canvas};

#[derive(Debug, Clone, Copy)]
pub(super) enum Icon {
    Terminal,
    PowerShell,
    Cmd,
    Linux,
    Ssh,
    System,
    Processes,
    Docker,
    Logs,
    Settings,
    Help,
    Add,
    SplitVertical,
    SplitHorizontal,
    Close,
    Move,
    Minimize,
    Maximize,
    Restore,
}

impl From<PaneView> for Icon {
    fn from(view: PaneView) -> Self {
        match view {
            PaneView::Terminal => Self::Terminal,
            PaneView::PowerShell => Self::PowerShell,
            PaneView::Cmd => Self::Cmd,
            PaneView::Wsl => Self::Linux,
            PaneView::Ssh => Self::Ssh,
            PaneView::DockerExec | PaneView::DockerContainers | PaneView::DockerInspect => {
                Self::Docker
            }
            PaneView::DockerLogs => Self::Logs,
            PaneView::Monitor => Self::System,
            PaneView::Processes => Self::Processes,
            PaneView::Settings => Self::Settings,
            PaneView::Help => Self::Help,
        }
    }
}

type Line = &'static [(f32, f32)];
impl Icon {
    fn lines(self) -> &'static [Line] {
        match self {
            Self::Terminal => &[
                &[(3., 5.), (21., 5.), (21., 19.), (3., 19.), (3., 5.)],
                &[(6., 9.), (9., 12.), (6., 15.)],
                &[(12., 15.), (17., 15.)],
            ],
            Self::PowerShell => &[
                &[(6., 5.), (22., 5.), (18., 19.), (2., 19.), (6., 5.)],
                &[(9., 9.), (13., 12.), (7., 16.)],
                &[(13., 16.), (17., 16.)],
            ],
            Self::Cmd => &[
                &[(3., 5.), (21., 5.), (21., 19.), (3., 19.), (3., 5.)],
                &[(3., 8.), (21., 8.)],
                &[(6., 11.), (9., 14.), (6., 17.)],
                &[(12., 17.), (17., 17.)],
            ],
            Self::Linux => &[
                &[
                    (5., 19.),
                    (8., 12.),
                    (8., 7.),
                    (10., 3.),
                    (14., 3.),
                    (16., 7.),
                    (16., 12.),
                    (19., 19.),
                    (15., 21.),
                    (12., 19.),
                    (9., 21.),
                    (5., 19.),
                ],
                &[(10., 10.), (12., 12.), (14., 10.), (10., 10.)],
                &[(10., 7.), (10., 8.)],
                &[(14., 7.), (14., 8.)],
            ],
            Self::Ssh => &[
                &[(3., 6.), (15., 6.), (15., 18.), (3., 18.), (3., 6.)],
                &[(6., 10.), (9., 12.), (6., 14.)],
                &[
                    (17., 11.),
                    (21., 11.),
                    (21., 18.),
                    (17., 18.),
                    (17., 11.),
                    (17., 8.),
                    (21., 8.),
                    (21., 11.),
                ],
            ],
            Self::System => &[
                &[(3., 5.), (21., 5.), (21., 17.), (3., 17.), (3., 5.)],
                &[
                    (5., 12.),
                    (8., 12.),
                    (10., 8.),
                    (13., 15.),
                    (16., 10.),
                    (19., 10.),
                ],
                &[(12., 17.), (12., 21.)],
                &[(8., 21.), (16., 21.)],
            ],
            Self::Processes => &[
                &[(4., 5.), (6., 5.)],
                &[(9., 5.), (20., 5.)],
                &[(4., 10.), (6., 10.)],
                &[(9., 10.), (17., 10.)],
                &[(4., 15.), (6., 15.)],
                &[(9., 15.), (20., 15.)],
                &[(4., 20.), (6., 20.)],
                &[(9., 20.), (15., 20.)],
            ],
            Self::Docker => &[
                &[
                    (3., 12.),
                    (21., 12.),
                    (19., 18.),
                    (15., 21.),
                    (8., 21.),
                    (4., 18.),
                    (3., 12.),
                ],
                &[(5., 12.), (5., 7.), (10., 7.), (10., 12.)],
                &[(10., 7.), (10., 2.), (15., 2.), (15., 12.)],
                &[(15., 7.), (20., 7.), (20., 12.)],
            ],
            Self::Logs => &[
                &[
                    (5., 3.),
                    (15., 3.),
                    (20., 8.),
                    (20., 21.),
                    (5., 21.),
                    (5., 3.),
                ],
                &[(15., 3.), (15., 8.), (20., 8.)],
                &[(8., 11.), (16., 11.)],
                &[(8., 15.), (16., 15.)],
                &[(8., 18.), (13., 18.)],
            ],
            Self::Settings => &[
                &[(4., 6.), (20., 6.)],
                &[(4., 12.), (20., 12.)],
                &[(4., 18.), (20., 18.)],
                &[(8., 3.), (8., 9.)],
                &[(16., 9.), (16., 15.)],
                &[(10., 15.), (10., 21.)],
            ],
            Self::Help => &[
                &[
                    (8., 7.),
                    (8., 5.),
                    (10., 3.),
                    (14., 3.),
                    (17., 6.),
                    (17., 9.),
                    (12., 13.),
                    (12., 16.),
                ],
                &[(12., 20.), (12., 21.)],
            ],
            Self::Add => &[&[(12., 4.), (12., 20.)], &[(4., 12.), (20., 12.)]],
            Self::SplitVertical => &[
                &[(3., 4.), (21., 4.), (21., 20.), (3., 20.), (3., 4.)],
                &[(12., 4.), (12., 20.)],
            ],
            Self::SplitHorizontal => &[
                &[(3., 4.), (21., 4.), (21., 20.), (3., 20.), (3., 4.)],
                &[(3., 12.), (21., 12.)],
            ],
            Self::Close => &[&[(6., 6.), (18., 18.)], &[(6., 18.), (18., 6.)]],
            Self::Move => &[
                &[(3., 12.), (21., 12.)],
                &[(12., 3.), (12., 21.)],
                &[(7., 8.), (3., 12.), (7., 16.)],
                &[(17., 8.), (21., 12.), (17., 16.)],
                &[(8., 7.), (12., 3.), (16., 7.)],
                &[(8., 17.), (12., 21.), (16., 17.)],
            ],
            Self::Minimize => &[&[(5., 16.), (19., 16.)]],
            Self::Maximize => &[&[(5., 5.), (19., 5.), (19., 19.), (5., 19.), (5., 5.)]],
            Self::Restore => &[
                &[(5., 8.), (16., 8.), (16., 19.), (5., 19.), (5., 8.)],
                &[(8., 8.), (8., 5.), (19., 5.), (19., 16.), (16., 16.)],
            ],
        }
    }
}

pub(super) fn view(icon: Icon, size: f32, color: Color) -> Element<'static, Message> {
    canvas::Canvas::new(Outline { icon, color })
        .width(size)
        .height(size)
        .into()
}

struct Outline {
    icon: Icon,
    color: Color,
}
impl canvas::Program<Message> for Outline {
    type State = ();
    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        frame.scale(bounds.width.min(bounds.height) / 24.0);
        for line in self.icon.lines() {
            let path = canvas::Path::new(|p| {
                for (i, &(x, y)) in line.iter().enumerate() {
                    if i == 0 {
                        p.move_to(iced::Point::new(x, y));
                    } else {
                        p.line_to(iced::Point::new(x, y));
                    }
                }
            });
            frame.stroke(
                &path,
                canvas::Stroke::default()
                    .with_width(1.7)
                    .with_color(self.color),
            );
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_paths_are_finite_nonempty_and_inside_viewbox() {
        for icon in PaneView::ALL.map(Icon::from).into_iter().chain([
            Icon::Add,
            Icon::SplitVertical,
            Icon::SplitHorizontal,
            Icon::Close,
            Icon::Move,
            Icon::Minimize,
            Icon::Maximize,
            Icon::Restore,
        ]) {
            assert!(!icon.lines().is_empty());
            for line in icon.lines() {
                assert!(line.len() >= 2);
                for &(x, y) in *line {
                    assert!(x.is_finite() && y.is_finite());
                    assert!((0.0..=24.0).contains(&x) && (0.0..=24.0).contains(&y));
                }
                // Exercise the renderer's actual path builder, not a string parser.
                let _ = canvas::Path::new(|p| {
                    p.move_to(iced::Point::new(line[0].0, line[0].1));
                    for &(x, y) in &line[1..] {
                        p.line_to(iced::Point::new(x, y));
                    }
                });
            }
        }
    }
}
