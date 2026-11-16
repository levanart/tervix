//! Native widgets only: no transport handles, raw inspect JSON or shell commands.
use crate::i18n::{Choice as Item, Key as K, Language};
use crate::{
    design,
    docker::{self, Channel, Docker, Event, PaneState, Sort},
    workspace::{Message, PaneView, Workspace},
};
use iced::{
    Element, Length,
    widget::{button, column, container, pick_list, row, scrollable, text, text_input, tooltip},
};
use tervix_application::{
    ViewId,
    docker::{Action, ConnectionState},
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct Choice {
    index: usize,
    label: String,
}
impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}
fn control<'a>(label: &'static str, id: ViewId, event: Event) -> iced::widget::Button<'a, Message> {
    button(text(label).size(12))
        .padding([5, 8])
        .style(design::control)
        .on_press(Message::Docker(id, event))
}
pub(super) fn view(
    state: &Workspace,
    id: ViewId,
    kind: PaneView,
    width: f32,
) -> Element<'_, Message> {
    let l = state.language;
    let Some(d) = &state.docker else {
        return text(l.text(K::DockerUnavailable)).into();
    };
    let Some(p) = d.panes.get(&id) else {
        return text(l.text(K::DockerPreparing)).into();
    };
    let choices: Vec<_> = d
        .endpoints
        .iter()
        .enumerate()
        .map(|(index, (label, _))| Choice {
            index,
            label: label.render(l),
        })
        .collect();
    let selected = choices.get(d.endpoint).cloned();
    let status = match &d.snapshot.state {
        ConnectionState::Disconnected => l.text(K::DockerDisconnected).into(),
        ConnectionState::Connecting => l.text(K::DockerConnecting).into(),
        ConnectionState::Connected => l.format(
            K::DockerConnected,
            &[
                &d.snapshot.containers.len().to_string(),
                endpoint_label(d.snapshot.endpoint.as_ref(), l),
            ],
        ),
        ConnectionState::Retrying(e) => l.format(K::DockerRetrying, &[&e.to_string()]),
    };
    let mut content = column![
        row![
            text(match d.snapshot.state {
                ConnectionState::Connected => l.text(K::DockerConnectedCompact).to_owned(),
                _ => status,
            })
            .size(12)
            .color(design::TEAL)
            .width(Length::Fill),
            control(l.text(K::ConnectionSetup), id, Event::Setup)
        ]
        .spacing(4),
    ]
    .spacing(6);
    if p.setup {
        content = content.push(
            column![
                pick_list(choices, selected, move |c| Message::Docker(
                    id,
                    Event::Endpoint(c.index)
                ))
                .text_size(12)
                .padding(5)
                .width(Length::Fill)
                .style(design::picker),
                row![
                    control(l.text(K::Connect), id, Event::Connect),
                    control(l.text(K::Disconnect), id, Event::Disconnect)
                ]
                .spacing(4),
                control(l.text(K::DiscoverContexts), id, Event::Discover),
                text(d.notice.render(l)).size(12),
            ]
            .spacing(6),
        );
    }
    if let Some(error) = &p.error {
        content = content.push(text(error.render(l)).size(12));
    }
    if p.loading() {
        content = content.push(text(l.text(K::DockerLoading)).size(12).color(design::TEAL));
    }
    // At the smallest size setup is a temporary task surface. Keep the log
    // subscription alive, but do not squeeze its viewport under a connection form.
    if kind == PaneView::DockerLogs && p.setup {
        return scrollable(content).height(Length::Fill).into();
    }
    let body = match kind {
        PaneView::DockerContainers => containers(d, p, id, l, width < 620.0),
        PaneView::DockerLogs => logs(p, id, l),
        PaneView::DockerInspect => inspect(d, p, id, l),
        _ => text(l.text(K::UnknownDocker)).into(),
    };
    // One scroller keeps all controls reachable even in minimum-size recursive tiles.
    if kind == PaneView::DockerLogs {
        content.push(body).height(Length::Fill).into()
    } else {
        scrollable(content.push(body)).height(Length::Fill).into()
    }
}

pub(super) fn confirmation(state: &Workspace) -> Option<Element<'_, Message>> {
    let l = state.language;
    let c = state.docker.as_ref()?.confirmation.as_ref()?;
    let id = c.source;
    let endpoint = state.docker.as_ref()?.snapshot.endpoint.as_ref();
    Some(
        container(
            column![
                text(l.format(K::ConfirmDocker, &[action_name(&c.action, l)])).size(22),
                text(&c.target.name).size(16),
                text(c.target.id.as_str())
                    .size(13)
                    .font(iced::Font::MONOSPACE),
                text(l.format(K::Engine, &[endpoint_label(endpoint, l)])).size(12),
                text(l.text(K::DockerConfirmDetail)).size(13),
                crate::presentation::language_picker(l),
                text(l.text(K::ConfirmHint)).size(12).color(design::MUTED),
                row![
                    control(l.text(K::Cancel), id, Event::Confirm(false)),
                    control(l.text(K::Confirm), id, Event::Confirm(true)).style(design::danger)
                ]
                .spacing(8),
            ]
            .spacing(12),
        )
        .padding(24)
        .width((state.window_size.width - 64.0).min(580.0))
        .style(|theme| {
            let mut style = design::surface(theme);
            style.shadow = design::shadow();
            style
        })
        .into(),
    )
}

fn action_name(action: &Action, l: Language) -> &'static str {
    l.text(match action {
        Action::Start => K::Start,
        Action::Stop => K::Stop,
        Action::Restart => K::Restart,
        Action::Pause => K::Pause,
        Action::Unpause => K::Unpause,
        Action::Remove { .. } => K::Remove,
    })
}

fn containers<'a>(
    d: &'a Docker,
    p: &'a PaneState,
    id: ViewId,
    l: Language,
    narrow: bool,
) -> Element<'a, Message> {
    let sort_item = |s| {
        Item::new(
            s,
            l.text(match s {
                Sort::Name => K::Name,
                Sort::Status => K::Status,
                Sort::Image => K::Image,
                Sort::Ports => K::Ports,
            }),
        )
    };
    let mut table = column![
        text(l.text(K::DockerKeys)).size(11).color(design::MUTED),
        text_input(l.text(K::DockerSearch), &p.query)
            .id(docker::search_id(id))
            .on_input(move |q| Message::Docker(id, Event::Query(q)))
            .size(12),
        row![
            text(l.text(K::Sort)).size(12),
            pick_list(
                [Sort::Name, Sort::Status, Sort::Image, Sort::Ports].map(sort_item),
                Some(sort_item(p.sort)),
                move |sort| Message::Docker(id, Event::Sort(sort.value))
            )
            .text_size(12)
        ]
        .spacing(6),
    ]
    .spacing(6);
    let mut details = column![].spacing(6);
    if let Some(target) = p.selected.as_ref().filter(|_| !narrow || p.show_details) {
        if narrow {
            details = details.push(control(l.text(K::Back), id, Event::Back));
        }
        if let Some(selected) = d.snapshot.containers.iter().find(|r| &r.id == target) {
            details = details
                .push(text(docker::name(selected)).size(16))
                .push(text(format!("{} · {}", selected.state, selected.status)).size(12))
                .push(text(&selected.image).size(12))
                .push(text(docker::ports(selected)).size(12));
        }
        let enabled = d.snapshot.state == ConnectionState::Connected;
        let mut views = row![].spacing(4);
        for (label, kind) in [
            (l.text(K::OpenLogs), PaneView::DockerLogs),
            (l.text(K::OpenInspect), PaneView::DockerInspect),
        ] {
            let event = Event::OpenAt(d.generation, d.snapshot.incarnation, target.clone(), kind);
            views = views.push(
                control(label, id, event.clone())
                    .on_press_maybe(enabled.then_some(Message::Docker(id, event))),
            );
        }
        let running = d
            .snapshot
            .containers
            .iter()
            .any(|r| &r.id == target && r.state == "running");
        let exec = Event::ExecAt(d.generation, d.snapshot.incarnation, target.clone(), false);
        views = views.push(
            control(l.text(K::ExecSh), id, exec.clone())
                .on_press_maybe((enabled && running).then_some(Message::Docker(id, exec))),
        );
        details = details.push(views);
        let mut actions = column![].spacing(4);
        if enabled && running {
            actions = actions.push(
                control(
                    l.text(K::ExecBash),
                    id,
                    Event::ExecAt(d.generation, d.snapshot.incarnation, target.clone(), true),
                )
                .width(Length::Fill),
            );
        }
        details = details.push(control(l.text(K::Actions), id, Event::Actions));
        for (label, action) in [
            (l.text(K::Start), Action::Start),
            (l.text(K::Stop), Action::Stop),
            (l.text(K::Restart), Action::Restart),
            (l.text(K::Pause), Action::Pause),
            (l.text(K::Unpause), Action::Unpause),
            (
                l.text(K::Remove),
                Action::Remove {
                    confirmed: target.clone(),
                },
            ),
        ] {
            let state = d
                .snapshot
                .containers
                .iter()
                .find(|r| &r.id == target)
                .map(|r| r.state.as_str())
                .unwrap_or("");
            let available = match &action {
                Action::Start => matches!(state, "created" | "exited"),
                Action::Stop | Action::Restart | Action::Pause => state == "running",
                Action::Unpause => state == "paused",
                Action::Remove { .. } => matches!(state, "created" | "exited" | "dead"),
            };
            if !available {
                continue;
            }
            let event = Event::ActAt(d.generation, d.snapshot.incarnation, target.clone(), action);
            actions = actions.push(
                control(label, id, event.clone())
                    .width(Length::Fill)
                    .on_press_maybe(enabled.then_some(Message::Docker(id, event))),
            );
        }
        if p.actions {
            details = details.push(actions);
        }
    }
    if narrow && p.show_details && p.selected.is_some() {
        return details.into();
    }
    let rows = docker::rows(&d.snapshot, p);
    if rows.is_empty() {
        table = table.push(
            text(l.text(if d.snapshot.state == ConnectionState::Connected {
                K::NoContainers
            } else {
                K::NoDockerData
            }))
            .size(13),
        );
    }
    for r in rows {
        let selected = p.selected.as_ref() == Some(&r.id);
        table = table.push(
            button(
                row![
                    text(if selected { "▶" } else { " " }).size(12),
                    text(docker::name(r)).size(13).width(Length::FillPortion(2)),
                    text(&r.state).size(12).width(Length::Fill)
                ]
                .spacing(8),
            )
            .width(Length::Fill)
            .style(if selected {
                iced::widget::button::primary
            } else {
                design::control
            })
            .on_press(Message::Docker(id, Event::Select(r.id.clone()))),
        );
    }
    if narrow {
        table.into()
    } else {
        row![
            table.width(Length::FillPortion(3)),
            details.width(Length::FillPortion(2))
        ]
        .spacing(12)
        .into()
    }
}
fn logs(p: &PaneState, id: ViewId, l: Language) -> Element<'_, Message> {
    let Some(target) = &p.target else {
        return text(l.text(K::SelectLogs)).size(13).into();
    };
    let mut content = column![
        text(&target.name).size(12).wrapping(text::Wrapping::None),
        row![
            control(
                l.text(if p.paused { K::Resume } else { K::Pause }),
                id,
                Event::Pause
            ),
            control(l.text(K::Follow), id, Event::Follow(!p.follow)).style(if p.follow {
                design::selected
            } else {
                design::control
            }),
            iced::widget::tooltip(
                control(l.text(K::CopyCompact), id, Event::Copy),
                text(l.text(K::CopyFilteredHelp)).size(12),
                iced::widget::tooltip::Position::Bottom
            )
        ]
        .spacing(4),
        row![
            text_input(l.text(K::LogFilter), &p.query)
                .id(docker::search_id(id))
                .on_input(move |q| Message::Docker(id, Event::Query(q)))
                .size(12),
            pick_list(
                [
                    Channel::All,
                    Channel::Stdout,
                    Channel::Stderr,
                    Channel::Combined
                ]
                .map(|c| channel_item(c, l)),
                Some(channel_item(p.channel, l)),
                move |c| Message::Docker(id, Event::Channel(c.value))
            )
            .text_size(12)
        ]
        .spacing(5),
    ]
    .spacing(4);
    if p.frame
        .as_ref()
        .is_none_or(|frame| !matches!(frame.state, tervix_application::docker::LogState::Following))
    {
        content = content.push(control(l.text(K::Resubscribe), id, Event::Subscribe));
    }
    if let Some(frame) = &p.frame {
        let summary = l.format(
            K::LogSummaryCompact,
            &[
                &log_state(&frame.state, l),
                if p.paused { l.text(K::Frozen) } else { "" },
                &frame.entries.len().to_string(),
                &frame.dropped_entries.to_string(),
                &frame.truncated_chunks.to_string(),
            ],
        );
        // Reserve one line for the summary so font metrics cannot consume the log viewport.
        let footer = tooltip(
            text(summary.clone())
                .size(11)
                .wrapping(text::Wrapping::None),
            text(summary).size(11),
            tooltip::Position::Top,
        );
        let filter = p.log_filter();
        let mut entries = column![].spacing(3);
        for e in frame.entries.iter().filter(|e| filter.matches(e)) {
            entries = entries.push(
                text(format!(
                    "{} [{:?}] {}{}",
                    e.timestamp.as_deref().unwrap_or(l.text(K::NotAvailable)),
                    e.channel,
                    e.text,
                    if e.truncated {
                        l.text(K::Truncated)
                    } else {
                        ""
                    }
                ))
                .size(12)
                .font(iced::Font::MONOSPACE)
                .color(
                    if e.channel == tervix_application::docker::LogChannel::Stderr {
                        design::DANGER
                    } else {
                        design::TEXT
                    },
                ),
            );
        }
        content = content
            .push(
                scrollable(entries)
                    .id(docker::scroll_id(id))
                    .height(Length::Fill)
                    .on_scroll(move |v| {
                        Message::Docker(id, Event::Scrolled(v.relative_offset().y >= 0.999))
                    }),
            )
            .push(footer);
    } else {
        content = content.push(text(l.text(K::LogWaiting)).size(12));
    }
    content.height(Length::Fill).into()
}
fn inspect<'a>(d: &Docker, p: &'a PaneState, id: ViewId, l: Language) -> Element<'a, Message> {
    let Some(target) = &p.target else {
        return text(l.text(K::SelectInspect)).into();
    };
    let mut content = column![
        text(format!("{} · {}", target.name, target.id.as_str())).size(12),
        control(l.text(K::RefreshInspect), id, Event::Inspect),
        text(l.text(K::InspectPrivacy))
            .size(11)
            .color(design::MUTED)
    ]
    .spacing(8);
    if d.snapshot.state == ConnectionState::Connected && target.generation == d.generation {
        if let Some(details) = &p.details {
            let boolean = |v: Option<bool>| {
                l.text(match v {
                    Some(true) => K::Yes,
                    Some(false) => K::No,
                    None => K::NotAvailable,
                })
            };
            content = content.push(
                text(
                    l.format(
                        K::InspectDetails,
                        &[
                            &details.name,
                            &details.image,
                            &details.platform,
                            boolean(details.running),
                            boolean(details.paused),
                            &details
                                .exit_code
                                .map(|v| v.to_string())
                                .unwrap_or(l.text(K::NotAvailable).into()),
                            details
                                .started_at
                                .as_deref()
                                .unwrap_or(l.text(K::NotAvailable)),
                            details
                                .finished_at
                                .as_deref()
                                .unwrap_or(l.text(K::NotAvailable)),
                        ],
                    ),
                )
                .size(13),
            );
        }
        if let Some(stats) = &p.stats {
            content = content.push(
                text(
                    l.format(
                        K::InspectStats,
                        &[
                            &stats
                                .cpu_percent
                                .map(|v| format!("{v:.1}%"))
                                .unwrap_or(l.text(K::NotAvailable).into()),
                            &stats
                                .memory_bytes
                                .map(|v| v.to_string())
                                .unwrap_or(l.text(K::NotAvailable).into()),
                            &stats
                                .memory_limit
                                .map(|v| v.to_string())
                                .unwrap_or(l.text(K::NotAvailable).into()),
                            &format!("{:?}", stats.observed_at),
                        ],
                    ),
                )
                .size(13),
            );
        } else {
            content = content.push(text(l.text(K::StatsUnavailable)).size(12));
        }
    } else {
        content = content.push(text(l.text(K::DetailsUnavailable)).size(12));
    }
    content.into()
}

fn channel_item(c: Channel, l: Language) -> Item<Channel> {
    Item::new(
        c,
        match c {
            Channel::All => l.text(K::AllChannels),
            Channel::Stdout => "stdout",
            Channel::Stderr => "stderr",
            Channel::Combined => l.text(K::Combined),
        },
    )
}

fn endpoint_label(endpoint: Option<&tervix_application::docker::Endpoint>, l: Language) -> &str {
    use tervix_application::docker::Endpoint;
    match endpoint {
        Some(Endpoint::NamedPipe(path) | Endpoint::UnixSocket(path)) => path,
        None => l.text(K::NotAvailable),
    }
}

fn log_state(state: &tervix_application::docker::LogState, l: Language) -> String {
    use tervix_application::docker::LogState;
    match state {
        LogState::Following => l.text(K::LogStreaming).into(),
        LogState::Cancelled => l.text(K::LogCancelled).into(),
        LogState::Ended => l.text(K::LogEnded).into(),
        LogState::Failed(error) => l.format(K::Error, &[&error.to_string()]),
    }
}
