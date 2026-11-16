//! Shared immutable frames with independent per-view filtering, sorting and chart range.
use crate::design;
use crate::i18n::{Choice, Key as K, Language, Noun};
use crate::workspace::{Message, PaneView, Workspace};
use iced::{
    Color, Element, Length, Point, Rectangle, Renderer, Theme,
    widget::{
        button, canvas, column, container, pick_list, progress_bar, row, scrollable, text,
        text_input,
    },
};
use std::{
    cmp::Ordering,
    sync::Arc,
    time::{Duration, SystemTime},
};
use tervix_application::{
    ViewId,
    monitoring::{MonitorReader, MonitoringService, Process, Snapshot},
};

pub(super) struct Monitor {
    service: MonitoringService,
    reader: MonitorReader,
    pub(super) frame: Option<Arc<Snapshot>>,
}
impl Monitor {
    pub(super) fn new(service: MonitoringService) -> Self {
        let reader = service.reader();
        let frame = reader.latest();
        Self {
            service,
            reader,
            frame,
        }
    }
}

pub(super) fn keyboard(state: &mut Workspace, action: crate::workspace::Action) {
    use crate::workspace::Action;
    let Some(pane) = state
        .controller
        .layout()
        .panes()
        .into_iter()
        .find(|p| p.id == state.controller.layout().focused())
    else {
        return;
    };
    if state.views.get(&pane.view) == Some(&PaneView::Processes) {
        let config = state.process_views.entry(pane.view).or_default();
        match action {
            Action::SortProcesses => {
                config.sort = match config.sort {
                    Sort::Cpu => Sort::Memory,
                    Sort::Memory => Sort::Uptime,
                    Sort::Uptime => Sort::Pid,
                    Sort::Pid => Sort::Name,
                    Sort::Name => Sort::Cpu,
                };
                config.ascending = matches!(config.sort, Sort::Pid | Sort::Name);
            }
            Action::ReverseProcesses => config.ascending = !config.ascending,
            _ => {}
        }
    }
    if state.views.get(&pane.view) == Some(&PaneView::Monitor)
        && matches!(action, Action::HistoryRange)
    {
        let seconds = state.history_seconds.entry(pane.view).or_insert(60);
        *seconds = if *seconds == 60 { 300 } else { 60 };
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum Sort {
    Pid,
    Name,
    #[default]
    Cpu,
    Memory,
    Uptime,
}
fn sort_item(s: Sort, l: Language) -> Choice<Sort> {
    Choice::new(
        s,
        match s {
            Sort::Pid => "PID",
            Sort::Cpu => "CPU",
            Sort::Name => l.text(K::Name),
            Sort::Memory => l.text(K::Memory),
            Sort::Uptime => l.text(K::Uptime),
        },
    )
}
#[derive(Default)]
pub(super) struct ProcessView {
    query: String,
    sort: Sort,
    ascending: bool,
    selected: Option<tervix_application::monitoring::ProcessIdentity>,
}

impl ProcessView {
    fn validate_selection(&mut self, processes: &[Process]) {
        if self
            .selected
            .is_some_and(|selected| !processes.iter().any(|p| p.identity == Some(selected)))
        {
            self.selected = None;
        }
    }
}

pub(super) fn reconcile(state: &mut Workspace) {
    if let Some(monitor) = &state.monitor {
        monitor.service.set_visible(visible(state));
    }
    let reachable = state.controller.reachable_views();
    state.process_views.retain(|id, _| reachable.contains(id));
    state.history_seconds.retain(|id, _| reachable.contains(id));
}

pub(super) fn visible(state: &Workspace) -> bool {
    state.controller.layout().panes().iter().any(|p| {
        state.zoomed.is_none_or(|id| p.id == id)
            && matches!(
                state.views.get(&p.view),
                Some(PaneView::Monitor | PaneView::Processes)
            )
    })
}

pub(super) fn update(state: &mut Workspace, message: &Message) -> bool {
    match message {
        Message::ServicesTick(now) => {
            let _ = now;
            if let Some(monitor) = &mut state.monitor
                && let Some(frame) = monitor.reader.poll()
            {
                for view in state.process_views.values_mut() {
                    view.validate_selection(
                        frame.metrics.as_ref().map_or(&[], |m| m.processes.as_ref()),
                    );
                }
                monitor.frame = Some(frame);
            }
            false
        }
        Message::ProcessQuery(id, query) => {
            state.process_views.entry(*id).or_default().query = query.clone();
            true
        }
        Message::ProcessSort(id, sort) => {
            let view = state.process_views.entry(*id).or_default();
            view.ascending = if view.sort == *sort {
                !view.ascending
            } else {
                matches!(sort, Sort::Pid | Sort::Name)
            };
            view.sort = *sort;
            true
        }
        Message::ProcessSelect(id, selected) => {
            let view = state.process_views.entry(*id).or_default();
            view.selected = *selected;
            view.validate_selection(
                state
                    .monitor
                    .as_ref()
                    .and_then(|m| m.frame.as_ref())
                    .and_then(|f| f.metrics.as_ref())
                    .map_or(&[], |m| m.processes.as_ref()),
            );
            true
        }
        Message::History(id, seconds) => {
            state.history_seconds.insert(*id, (*seconds).clamp(60, 300));
            true
        }
        _ => false,
    }
}

pub(super) fn title_summary(state: &Workspace) -> String {
    let panes = state.controller.layout().panes();
    let Some(pane) = panes
        .iter()
        .find(|p| p.id == state.controller.layout().focused())
    else {
        return String::new();
    };
    let kind = state.views.get(&pane.view);
    if kind == Some(&PaneView::Wsl) {
        return format!(
            " | WSL {}",
            match state.profiles.get(&pane.view) {
                Some(tervix_application::terminal::Profile::Wsl(Some(name))) => name.as_str(),
                _ => state.language.text(K::SystemDefault),
            }
        );
    }
    if !matches!(kind, Some(PaneView::Monitor | PaneView::Processes)) {
        return String::new();
    }
    let Some(frame) = state.monitor.as_ref().and_then(|m| m.frame.as_ref()) else {
        return format!(" | {}", state.language.text(K::TelemetryPending));
    };
    let Some(metrics) = &frame.metrics else {
        return format!(" | {}", state.language.text(K::TelemetryUnavailable));
    };
    if kind == Some(&PaneView::Processes) {
        let fallback = ProcessView::default();
        let view = state.process_views.get(&pane.view).unwrap_or(&fallback);
        format!(
            " | {} / {} | {}: {}",
            indices(&metrics.processes, view).len(),
            state.language.count(metrics.processes.len(), Noun::Process),
            state.language.text(K::Filter),
            view.query
        )
    } else {
        format!(
            " | CPU {} | RAM {}{}",
            percent(metrics.cpu_percent, state.language),
            bytes(metrics.total_memory, state.language),
            if frame.is_stale() {
                format!(" | {}", state.language.text(K::Stale))
            } else {
                String::new()
            }
        )
    }
}

fn percent(value: Option<f32>, l: Language) -> String {
    value.map_or_else(|| l.text(K::NotAvailable).into(), |v| format!("{v:.1}%"))
}
fn bytes(value: Option<u64>, l: Language) -> String {
    value.map_or_else(
        || l.text(K::NotAvailable).into(),
        |v| {
            if v >= 1 << 30 {
                format!("{:.1} {}", v as f64 / (1u64 << 30) as f64, l.text(K::GiB))
            } else {
                format!("{:.1} {}", v as f64 / (1u64 << 20) as f64, l.text(K::MiB))
            }
        },
    )
}
fn uptime(value: Option<Duration>, l: Language) -> String {
    value.map_or_else(
        || l.text(K::NotAvailable).into(),
        |d| {
            l.format(
                K::HoursMinutes,
                &[
                    &(d.as_secs() / 3600).to_string(),
                    &format!("{:02}", d.as_secs() / 60 % 60),
                ],
            )
        },
    )
}
fn card(label: &'static str, value: String, detail: String) -> Element<'static, Message> {
    container(
        column![
            text(label).size(10).color(design::MUTED),
            text(value).size(28).color(design::TEXT),
            text(detail).size(11).color(design::MUTED)
        ]
        .spacing(6),
    )
    .padding(14)
    .width(Length::Fill)
    .style(design::surface)
    .into()
}

fn compare_optional<T: PartialOrd>(a: Option<T>, b: Option<T>, ascending: bool) -> Ordering {
    match (a, b) {
        (Some(a), Some(b)) => {
            let order = a.partial_cmp(&b).unwrap_or(Ordering::Equal);
            if ascending { order } else { order.reverse() }
        }
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (None, None) => Ordering::Equal,
    }
}
fn indices(processes: &[Process], view: &ProcessView) -> Vec<usize> {
    let query = view.query.to_lowercase();
    let mut rows: Vec<_> = processes
        .iter()
        .enumerate()
        .filter(|(_, p)| {
            query.is_empty()
                || p.name.to_lowercase().contains(&query)
                || p.pid.0.to_string().contains(&query)
                || p.path
                    .as_ref()
                    .is_some_and(|p| p.to_string_lossy().to_lowercase().contains(&query))
        })
        .map(|(i, _)| i)
        .collect();
    rows.sort_by(|a, b| {
        let (a, b) = (&processes[*a], &processes[*b]);
        let order = match view.sort {
            Sort::Pid => compare_optional(Some(a.pid), Some(b.pid), view.ascending),
            Sort::Name => compare_optional(
                Some(a.name.to_lowercase()),
                Some(b.name.to_lowercase()),
                view.ascending,
            ),
            Sort::Cpu => compare_optional(a.cpu_percent, b.cpu_percent, view.ascending),
            Sort::Memory => compare_optional(a.resident_bytes, b.resident_bytes, view.ascending),
            Sort::Uptime => compare_optional(a.uptime, b.uptime, view.ascending),
        };
        order.then_with(|| a.pid.cmp(&b.pid))
    });
    rows
}

pub(super) fn view(
    state: &Workspace,
    id: ViewId,
    processes: bool,
    width: f32,
) -> Element<'_, Message> {
    let narrow = width < 520.0;
    let l = state.language;
    let Some(frame) = state.monitor.as_ref().and_then(|m| m.frame.as_ref()) else {
        return text(l.text(K::TelemetryConnecting)).into();
    };
    let status = if let Some(error) = &frame.error {
        l.format(K::TelemetryError, &[&error.to_string()])
    } else if frame.is_stale() {
        l.text(K::FreshSample).into()
    } else {
        l.format(
            K::Cadence,
            &[
                &format!("{:.1}", frame.interval.as_secs_f32()),
                &frame.age().map_or(0, |a| a.as_secs()).to_string(),
            ],
        )
    };
    let mut content = column![text(status).size(11).color(if frame.is_stale() {
        Color::from_rgb8(244, 183, 109)
    } else {
        Color::from_rgb8(83, 215, 184)
    })]
    .spacing(12);
    let Some(metrics) = &frame.metrics else {
        return content.push(text(l.text(K::MetricsHelp)).size(13)).into();
    };
    if processes {
        let fallback = ProcessView::default();
        let config = state.process_views.get(&id).unwrap_or(&fallback);
        let rows = indices(&metrics.processes, config);
        content = content.push(
            text_input(l.text(K::ProcessSearch), &config.query)
                .id(iced::widget::text_input::Id::new(format!("search-{id:?}")))
                .on_input(move |q| Message::ProcessQuery(id, q))
                .size(13)
                .padding(8),
        );
        content = content.push(
            column![
                text(format!(
                    "{} / {}",
                    rows.len(),
                    l.count(metrics.processes.len(), Noun::Process)
                ))
                .size(12),
                row![
                    pick_list(
                        [Sort::Pid, Sort::Name, Sort::Cpu, Sort::Memory, Sort::Uptime]
                            .map(|s| sort_item(s, l)),
                        Some(sort_item(config.sort, l)),
                        move |s| Message::ProcessSort(id, s.value)
                    )
                    .text_size(12)
                    .style(design::picker),
                    button(
                        text(if config.ascending {
                            l.text(K::Ascending)
                        } else {
                            l.text(K::Descending)
                        })
                        .size(12)
                    )
                    .on_press(Message::ProcessSort(id, config.sort))
                ]
                .spacing(6)
            ]
            .spacing(8),
        );
        let mut headers = row![].spacing(8);
        for (sort, portion) in [
            (Sort::Name, 3),
            (Sort::Pid, 1),
            (Sort::Cpu, 1),
            (Sort::Memory, 1),
        ] {
            if narrow && sort == Sort::Memory {
                continue;
            }
            headers = headers.push(
                button(text(sort_item(sort, l).to_string()).size(11))
                    .width(Length::FillPortion(portion))
                    .on_press(Message::ProcessSort(id, sort)),
            );
        }
        content = content.push(headers);
        let mut table = column![].spacing(4);
        for index in rows {
            let p = &metrics.processes[index];
            if p.identity.is_some() && config.selected == p.identity {
                let details = l.format(
                    K::ProcessDetail,
                    &[
                        p.state.as_deref().unwrap_or(l.text(K::NotAvailable)),
                        &p.identity.as_ref().map_or_else(
                            || l.text(K::NotAvailable).into(),
                            |i| format!("{:?}", i.start_time),
                        ),
                        &uptime(p.uptime, l),
                        &bytes(p.private_bytes, l),
                        &p.path.as_ref().map_or_else(
                            || l.text(K::NotAvailable).into(),
                            |p| p.display().to_string(),
                        ),
                    ],
                );
                content = content.push(
                    container(
                        column![
                            text(format!("{} · PID {}", p.name, p.pid.0)).size(15),
                            text(format!(
                                "CPU {} · RAM {}",
                                percent(p.cpu_percent, l),
                                bytes(p.resident_bytes, l)
                            ))
                            .size(12),
                            text(details).size(12),
                            text(l.text(K::ReadOnly)).size(11)
                        ]
                        .spacing(6),
                    )
                    .padding(10)
                    .width(Length::Fill)
                    .style(design::surface),
                );
            }
            let mut cells = row![
                text(&p.name).size(13).width(Length::FillPortion(3)),
                text(p.pid.0.to_string()).size(12).width(Length::Fill),
                text(percent(p.cpu_percent, l)).size(12).width(Length::Fill),
            ]
            .spacing(8);
            if !narrow {
                cells = cells.push(
                    text(bytes(p.resident_bytes, l))
                        .size(12)
                        .width(Length::Fill),
                );
            }
            table = table.push(
                button(cells)
                    .padding(9)
                    .width(Length::Fill)
                    .style(if p.identity.is_some() && config.selected == p.identity {
                        design::selected
                    } else {
                        design::control
                    })
                    .on_press(Message::ProcessSelect(id, p.identity)),
            );
        }
        content = content.push(table);
    } else {
        let used = metrics
            .total_memory
            .zip(metrics.available_memory)
            .map(|(t, a)| t.saturating_sub(a));
        content = content.push(
            column![
                text(l.text(K::SystemOverview)).size(22),
                text(l.text(K::SharedSampler)).size(11).color(design::MUTED)
            ]
            .spacing(3),
        );
        content = content.push(
            row![
                card(
                    l.text(K::CpuMachine),
                    percent(metrics.cpu_percent, l),
                    l.format(K::LogicalCores, &[&metrics.cores.len().to_string()])
                ),
                card(
                    l.text(K::MemoryInUse),
                    bytes(used, l),
                    l.format(
                        K::MemoryAvailable,
                        &[
                            &bytes(metrics.available_memory, l),
                            &bytes(metrics.total_memory, l)
                        ]
                    )
                )
            ]
            .spacing(8),
        );
        let memory_share = memory_percent(metrics.total_memory, metrics.available_memory);
        let mut memory = column![
            row![
                text(l.text(K::PhysicalMemory))
                    .size(10)
                    .color(design::MUTED),
                iced::widget::horizontal_space(),
                text(percent(memory_share, l))
                    .size(12)
                    .color(design::VIOLET)
            ]
            .spacing(8)
        ]
        .spacing(10);
        if let Some(value) = memory_share {
            memory = memory.push(progress_bar(0.0..=100.0, value).height(8).style(|_| {
                progress_bar::Style {
                    background: design::CANVAS.into(),
                    bar: design::VIOLET.into(),
                    border: iced::Border {
                        radius: 4.0.into(),
                        ..Default::default()
                    },
                }
            }));
        }
        memory = memory.push(
            text(l.format(
                K::MemoryUsedAvailable,
                &[&bytes(used, l), &bytes(metrics.available_memory, l)],
            ))
            .size(11)
            .color(design::MUTED),
        );
        content = content.push(
            container(memory)
                .padding(14)
                .width(Length::Fill)
                .style(design::surface),
        );
        let seconds = state.history_seconds.get(&id).copied().unwrap_or(60);
        content = content.push(
            row![
                text(l.text(K::Utilization)).size(11).color(design::MUTED),
                iced::widget::horizontal_space(),
                button(text(l.text(K::Seconds60)).size(11))
                    .style(design::control)
                    .on_press(Message::History(id, 60)),
                button(text(l.text(K::Minutes5)).size(11))
                    .style(design::control)
                    .on_press(Message::History(id, 300)),
            ]
            .spacing(8),
        );
        content = content.push(
            container(
                column![
                    row![
                        text("● CPU").size(11).color(design::BLUE),
                        text(l.text(K::MemoryLegend)).size(11).color(design::VIOLET)
                    ]
                    .spacing(18),
                    canvas::Canvas::new(Chart {
                        frame,
                        seconds,
                        language: l
                    })
                    .width(Length::Fill)
                    .height(168),
                    text(l.format(
                        K::HistoryCaption,
                        &[&seconds.to_string(), &frame.history.len().to_string()]
                    ))
                    .size(10)
                    .color(design::MUTED)
                ]
                .spacing(10),
            )
            .padding(14)
            .width(Length::Fill)
            .style(design::surface),
        );
        content = content.push(row![
            text(l.text(K::LogicalProcessors))
                .size(10)
                .color(design::MUTED),
            iced::widget::horizontal_space(),
            text(l.format(K::Cores, &[&metrics.cores.len().to_string()]))
                .size(11)
                .color(design::TEAL)
        ]);
        for (pair, values) in metrics.cores.chunks(2).enumerate() {
            let mut bars = row![].spacing(10);
            for (offset, value) in values.iter().enumerate() {
                let mut cell = column![row![
                    text(format!("CPU {:02}", pair * 2 + offset + 1))
                        .size(10)
                        .color(design::MUTED),
                    iced::widget::horizontal_space(),
                    text(percent(*value, l)).size(11)
                ]]
                .spacing(7);
                if let Some(value) = value {
                    cell = cell.push(progress_bar(0.0..=100.0, *value).height(5).style(|_| {
                        progress_bar::Style {
                            background: design::CANVAS.into(),
                            bar: design::TEAL.into(),
                            border: iced::Border {
                                radius: 3.0.into(),
                                ..Default::default()
                            },
                        }
                    }));
                }
                bars = bars.push(
                    container(cell)
                        .padding(10)
                        .width(Length::Fill)
                        .style(design::surface),
                );
            }
            content = content.push(bars);
        }
        content = content.push(
            row![
                card(
                    l.text(K::Pagefile),
                    bytes(metrics.used_swap, l),
                    l.format(K::Configured, &[&bytes(metrics.total_swap, l)])
                ),
                card(
                    l.text(K::Processes),
                    metrics.processes.len().to_string(),
                    l.text(K::ReadOnly).into()
                )
            ]
            .spacing(8),
        );
    }
    scrollable(content.width(Length::Fill).padding(iced::Padding {
        right: 14.0,
        ..Default::default()
    }))
    .id(scrollable::Id::new(format!("metrics-{id:?}")))
    .height(Length::Fill)
    .into()
}

struct Chart<'a> {
    language: Language,
    frame: &'a Snapshot,
    seconds: u64,
}
fn memory_percent(total: Option<u64>, available: Option<u64>) -> Option<f32> {
    total
        .zip(available)
        .filter(|(total, _)| *total > 0)
        .map(|(total, available)| total.saturating_sub(available) as f32 / total as f32 * 100.0)
}

#[derive(Default)]
struct ChartCache {
    geometry: canvas::Cache,
    key: std::cell::Cell<Option<(Option<SystemTime>, u64, Language)>>,
}
impl canvas::Program<Message> for Chart<'_> {
    type State = ChartCache;
    fn draw(
        &self,
        cache: &ChartCache,
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let key = Some((self.frame.observed_at, self.seconds, self.language));
        if cache.key.replace(key) != key {
            cache.geometry.clear();
        }
        let geometry = cache.geometry.draw(renderer, bounds.size(), |frame| {
            let left = 34.0;
            let top = 8.0;
            let width = (bounds.width - left - 4.0).max(1.0);
            let height = (bounds.height - top - 22.0).max(1.0);
            for fraction in [0.0, 0.25, 0.5, 0.75, 1.0] {
                // Keep a one-logical-pixel grid independent of path stroke joins.
                frame.fill_rectangle(
                    Point::new(left, top + height * fraction),
                    iced::Size::new(width, 1.0),
                    design::BORDER,
                );
                frame.fill_text(canvas::Text {
                    content: format!("{}", (100.0 - fraction * 100.0) as u32),
                    position: Point::new(0.0, top + height * fraction - 5.0),
                    color: design::MUTED,
                    size: 10.0.into(),
                    ..Default::default()
                });
            }
            for (fraction, label) in [
                (
                    0.0,
                    format!("−{}{}", self.seconds, self.language.text(K::SecondUnit)),
                ),
                (
                    0.5,
                    format!("−{}{}", self.seconds / 2, self.language.text(K::SecondUnit)),
                ),
                (1.0, self.language.text(K::Now).into()),
            ] {
                frame.fill_text(canvas::Text {
                    content: label,
                    position: Point::new(
                        left + width * fraction - if fraction == 1.0 { 22.0 } else { 0.0 },
                        top + height + 7.0,
                    ),
                    color: design::MUTED,
                    size: 10.0.into(),
                    ..Default::default()
                });
            }
            let Some(end) = self.frame.observed_at else {
                return;
            };
            let total = self.frame.metrics.as_ref().and_then(|m| m.total_memory);
            for memory in [false, true] {
                let mut previous = None;
                for point in self.frame.history.iter() {
                    let age = end
                        .duration_since(point.observed_at)
                        .unwrap_or_default()
                        .as_secs_f32();
                    if age > self.seconds as f32 {
                        continue;
                    }
                    let value = if memory {
                        memory_percent(total, point.available_memory)
                    } else {
                        point.cpu_percent
                    };
                    if let Some(value) = value {
                        let point = Point::new(
                            left + width * (1.0 - age / self.seconds as f32),
                            top + height * (1.0 - value.clamp(0.0, 100.0) / 100.0),
                        );
                        if let Some(previous) = previous
                            && let Some(path) = ribbon(previous, point, 2.0)
                        {
                            frame.fill(&path, if memory { design::VIOLET } else { design::BLUE });
                        }
                        frame.fill(
                            &canvas::Path::circle(point, 1.6),
                            if memory { design::VIOLET } else { design::BLUE },
                        );
                        previous = Some(point);
                    } else {
                        previous = None;
                    }
                }
            }
        });
        vec![geometry]
    }
}

fn ribbon(from: Point, to: Point, thickness: f32) -> Option<canvas::Path> {
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let length = dx.hypot(dy);
    if length <= f32::EPSILON {
        return None;
    }
    let (x, y) = (
        -dy / length * thickness / 2.0,
        dx / length * thickness / 2.0,
    );
    Some(canvas::Path::new(|path| {
        path.move_to(Point::new(from.x + x, from.y + y));
        path.line_to(Point::new(to.x + x, to.y + y));
        path.line_to(Point::new(to.x - x, to.y - y));
        path.line_to(Point::new(from.x - x, from.y - y));
        path.close();
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_tracks_process_incarnation_and_clears_on_reuse_or_unknown_identity() {
        use tervix_application::monitoring::{ProcessId, ProcessIdentity};
        let identity = ProcessIdentity {
            pid: ProcessId(7),
            start_time: 100,
        };
        let mut view = ProcessView {
            selected: Some(identity),
            ..Default::default()
        };
        let mut p = process(7, "original", None);
        p.identity = Some(identity);
        view.validate_selection(&[p.clone()]);
        assert_eq!(view.selected, Some(identity));
        p.identity = Some(ProcessIdentity {
            start_time: 101,
            ..identity
        });
        view.validate_selection(&[p.clone()]);
        assert_eq!(view.selected, None);
        view.selected = Some(identity);
        p.identity = None;
        view.validate_selection(&[p]);
        assert_eq!(view.selected, None);
    }

    #[test]
    fn offset_chart_retains_cached_meshes_and_axis_labels() -> Result<(), &'static str> {
        let frame = Snapshot {
            revision: 1,
            observed_at: None,
            sampled_at: None,
            interval: Duration::from_secs(1),
            metrics: None,
            error: None,
            history: Arc::from([]),
        };
        let renderer = crate::test_renderer::renderer(iced::Font::default());
        let bounds = Rectangle {
            x: 700.0,
            y: 500.0,
            width: 300.0,
            height: 168.0,
        };
        let chart = Chart {
            language: Language::Ru,
            frame: &frame,
            seconds: 60,
        };
        let geometry = canvas::Program::<Message>::draw(
            &chart,
            &ChartCache::default(),
            &renderer,
            &Theme::Dark,
            bounds,
            iced::mouse::Cursor::Unavailable,
        );
        let iced_wgpu::Geometry::Cached(cache) = &geometry[0] else {
            return Err("chart should retain cached geometry");
        };
        assert!(
            cache.text.is_some(),
            "fixture must cover cached axis labels"
        );
        assert!(
            cache.meshes.is_some(),
            "plot must include filled grid geometry"
        );
        Ok(())
    }

    #[test]
    fn memory_visualization_preserves_unknown_and_handles_zero_capacity() {
        assert_eq!(memory_percent(None, Some(1)), None);
        assert_eq!(memory_percent(Some(0), Some(0)), None);
        assert_eq!(memory_percent(Some(100), None), None);
        assert_eq!(memory_percent(Some(100), Some(25)), Some(75.0));
        assert_eq!(memory_percent(Some(100), Some(110)), Some(0.0));
    }

    #[test]
    fn clicking_inactive_process_search_focuses_domain_and_keeps_typing_in_that_input()
    -> Result<(), Box<dyn std::error::Error>> {
        use crate::workspace::{self, Action};
        use iced::advanced::{Layout, Shell, layout, widget::Tree};
        use iced::{Event, Size, keyboard, mouse};
        use tervix_application::{
            Axis,
            monitoring::{Metrics, MonitoringConfig, TelemetryError, TelemetryProvider},
        };
        struct Source;
        impl TelemetryProvider for Source {
            fn sample(&mut self) -> Result<Metrics, TelemetryError> {
                Ok(Metrics::default())
            }
        }
        let service =
            MonitoringService::start(Some(Box::new(Source)), MonitoringConfig::default())?;
        let reader = service.reader();
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while !reader.latest().is_some_and(|frame| frame.metrics.is_some())
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        let (mut state, _) = workspace::initialize();
        state.monitor = Some(Monitor::new(service));
        assert!(
            state
                .monitor
                .as_ref()
                .and_then(|m| m.frame.as_ref())
                .is_some()
        );
        let _ = workspace::update(&mut state, Message::ReducedMotion(true));
        let first = state.controller.layout().panes()[0];
        state.views.insert(first.view, PaneView::Processes);
        state.configuring = true;
        let _ = workspace::update(&mut state, Message::Action(Action::Split(Axis::Vertical)));
        state.configuring = false;
        let second = *state
            .controller
            .layout()
            .panes()
            .iter()
            .find(|p| p.id != first.id)
            .ok_or("second pane")?;
        state.views.insert(second.view, PaneView::Processes);
        assert_eq!(state.controller.layout().focused(), second.id);
        let renderer = crate::test_renderer::renderer(iced::Font::default());
        let bounds = Rectangle::with_size(state.window_size);
        let limits = layout::Limits::new(Size::ZERO, state.window_size);
        let mut element = workspace::view(&state);
        let mut tree = Tree::new(&element);
        let node = element.as_widget().layout(&mut tree, &renderer, &limits);
        // Find the actual inactive Processes input via its pointer interaction,
        // rather than sending ProcessQuery or synthesizing domain focus directly.
        let point = (0..bounds.height as u32)
            .step_by(4)
            .map(|y| Point::new(100.0, y as f32))
            .find(|p| {
                element.as_widget().mouse_interaction(
                    &tree,
                    Layout::new(&node),
                    mouse::Cursor::Available(*p),
                    &bounds,
                    &renderer,
                ) == mouse::Interaction::Text
            })
            .ok_or("actual Processes search input not found")?;
        let mut messages = Vec::new();
        let status = element.as_widget_mut().on_event(
            &mut tree,
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            Layout::new(&node),
            mouse::Cursor::Available(point),
            &renderer,
            &mut iced::advanced::clipboard::Null,
            &mut Shell::new(&mut messages),
            &bounds,
        );
        assert_eq!(
            status,
            iced::event::Status::Captured,
            "textbox captures its press"
        );
        assert!(matches!(messages.first(), Some(Message::PanePressed(id)) if *id == first.id));
        drop(element);
        for message in messages {
            let _ = workspace::update(&mut state, message);
        }
        assert_eq!(state.controller.layout().focused(), first.id);
        // Reordering the focused pane to the front must retain the clicked input's
        // tree and caret. Ordinary typing must still reach the real text widget.
        let mut element = workspace::view(&state);
        tree.diff(&element);
        let node = element.as_widget().layout(&mut tree, &renderer, &limits);
        let mut messages = Vec::new();
        let status = element.as_widget_mut().on_event(
            &mut tree,
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Character("x".into()),
                modified_key: keyboard::Key::Character("x".into()),
                physical_key: keyboard::key::Physical::Code(keyboard::key::Code::KeyX),
                location: keyboard::Location::Standard,
                modifiers: keyboard::Modifiers::empty(),
                text: Some("x".into()),
            }),
            Layout::new(&node),
            mouse::Cursor::Available(point),
            &renderer,
            &mut iced::advanced::clipboard::Null,
            &mut Shell::new(&mut messages),
            &bounds,
        );
        assert_eq!(status, iced::event::Status::Captured);
        assert!(
            messages.iter().any(
                |m| matches!(m, Message::ProcessQuery(id, q) if *id == first.view && q == "x")
            )
        );
        drop(element);
        for message in messages {
            let _ = workspace::update(&mut state, message);
        }
        let _ = workspace::update(&mut state, Message::Action(Action::SortProcesses));
        assert_eq!(state.process_views[&first.view].sort, Sort::Memory);
        assert_eq!(state.process_views[&first.view].query, "x");
        assert!(!state.process_views.contains_key(&second.view));
        state.configuring = true;
        let _ = workspace::update(&mut state, Message::Action(Action::Close));
        assert_eq!(state.controller.layout().panes(), vec![second]);
        Ok(())
    }
    #[test]
    fn absent_values_are_na_and_sort_last_in_both_directions() {
        assert_eq!(percent(None, Language::En), "N/A");
        assert_eq!(bytes(None, Language::En), "N/A");
        assert_eq!(uptime(None, Language::En), "N/A");
        for ascending in [false, true] {
            assert_eq!(
                compare_optional(None, Some(0u64), ascending),
                Ordering::Greater
            );
        }
    }

    fn process(pid: u32, name: &str, cpu: Option<f32>) -> Process {
        Process {
            pid: tervix_application::monitoring::ProcessId(pid),
            identity: None,
            name: name.into(),
            cpu_percent: cpu,
            resident_bytes: None,
            private_bytes: None,
            path: Some(format!("C:/tools/{name}.exe").into()),
            state: None,
            uptime: None,
        }
    }

    #[test]
    fn independent_tables_filter_pid_name_path_and_sort_unknown_values_last() {
        let rows = [
            process(5, "Alpha", Some(10.0)),
            process(9, "beta", None),
            process(3, "Gamma", Some(20.0)),
        ];
        let mut a = ProcessView::default();
        let b = ProcessView {
            query: "TOOLS/BETA".into(),
            sort: Sort::Name,
            ascending: true,
            ..Default::default()
        };
        assert_eq!(indices(&rows, &a), vec![2, 0, 1]);
        a.ascending = true;
        assert_eq!(indices(&rows, &a), vec![0, 2, 1]);
        assert_eq!(indices(&rows, &b), vec![1]);
        a.query = "3".into();
        assert_eq!(indices(&rows, &a), vec![2]);
        a.query = "missing".into();
        assert!(indices(&rows, &a).is_empty());
        assert_eq!(indices(&rows, &b), vec![1]);
    }

    #[test]
    fn duplicate_panels_share_one_frame_and_visibility_aggregates_across_stretch()
    -> Result<(), Box<dyn std::error::Error>> {
        use crate::workspace::{Action, initialize, update};
        use tervix_application::{
            Axis,
            monitoring::{Metrics, MonitoringConfig, TelemetryError, TelemetryProvider},
        };
        struct Source;
        impl TelemetryProvider for Source {
            fn sample(&mut self) -> Result<Metrics, TelemetryError> {
                Ok(Metrics {
                    cpu_percent: Some(12.0),
                    ..Default::default()
                })
            }
        }
        let service =
            MonitoringService::start(Some(Box::new(Source)), MonitoringConfig::new(500, 1000, 60))?;
        let reader = service.reader();
        let (mut state, _) = initialize();
        state.monitor = Some(Monitor::new(service));
        state.configuring = true;
        let terminal = state.controller.layout().focused();
        let _ = update(&mut state, Message::Action(Action::Split(Axis::Vertical)));
        let first = state.controller.layout().focused();
        let _ = update(&mut state, Message::ChangeView(first, PaneView::Monitor));
        let _ = update(&mut state, Message::Action(Action::Split(Axis::Horizontal)));
        let second = state.controller.layout().focused();
        let _ = update(&mut state, Message::ChangeView(second, PaneView::Processes));
        let wait =
            |state: &mut Workspace, cadence: u64| -> Result<(), Box<dyn std::error::Error>> {
                let deadline = std::time::Instant::now() + Duration::from_secs(4);
                while std::time::Instant::now() < deadline {
                    let _ = update(state, Message::ServicesTick(std::time::Instant::now()));
                    if state
                        .monitor
                        .as_ref()
                        .and_then(|m| m.frame.as_ref())
                        .is_some_and(|f| f.interval == Duration::from_millis(cadence))
                    {
                        return Ok(());
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err("monitor visibility cadence did not settle".into())
            };
        wait(&mut state, 500)?;
        let frame = state
            .monitor
            .as_ref()
            .and_then(|m| m.frame.as_ref())
            .ok_or("frame missing")?;
        let other = reader.latest().ok_or("reader missing")?;
        assert!(Arc::ptr_eq(frame, &other));
        let _ = view(
            &state,
            state.controller.layout().panes()[1].view,
            false,
            300.0,
        );
        let _ = update(&mut state, Message::PaneAction(terminal, Action::Zoom));
        wait(&mut state, 1000)?;
        let _ = update(&mut state, Message::Action(Action::Zoom));
        wait(&mut state, 500)?;
        assert_eq!(state.controller.layout().panes().len(), 3);
        Ok(())
    }
}
