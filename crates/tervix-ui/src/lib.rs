//! Native tiling/terminal spike. Presentation depends on application snapshots, never OS handles.

mod chrome;
mod design;
mod docker;
mod docker_view;
mod docking;
mod i18n;
mod icons;
mod launcher;
mod monitoring;
mod motion;
mod pane_focus;
mod pane_stack;
mod presentation;
mod ssh;
mod terminal;
mod terminal_measure;
mod workspace;
mod wsl;

#[cfg(test)]
mod test_renderer;

/// Opens the native layout workspace and runs its event loop on the calling thread.
/// The composition root supplies one monitoring service and one asynchronous WSL
/// catalog for the application lifetime; duplicate panes never create providers.
/// `docker` is one shared application service. Startup, pane selection and context
/// discovery do not connect; the user must explicitly choose Connect.
/// `resize_window` lends the toolkit's window handle to the composition root's
/// native adapter on the event thread. It must not retain the handle. Edge codes
/// are left/right/top/bottom/top-left/top-right/bottom-left/bottom-right (0..=7).
/// Non-Windows hosts retain system decorations and their system resize behavior.
///
/// # Errors
/// Returns an Iced error if window creation or the renderer/event loop fails.
pub fn run(
    provider: std::sync::Arc<dyn tervix_application::terminal::TerminalProvider>,
    monitoring: tervix_application::monitoring::MonitoringService,
    discovery: Result<tervix_application::wsl::DistributionCatalog, String>,
    docker: std::sync::Arc<dyn tervix_application::docker::DockerService>,
    resize_window: for<'a> fn(iced::window::raw_window_handle::WindowHandle<'a>, u8) -> bool,
) -> iced::Result {
    iced::application(workspace::title, workspace::update, workspace::view)
        .default_font(iced::Font::with_name("Segoe UI"))
        .subscription(workspace::subscription)
        .style(|_, _| iced::application::Appearance {
            background_color: iced::Color::TRANSPARENT,
            text_color: design::TEXT,
        })
        .theme(|_| {
            iced::Theme::custom(
                "Tervix Blue Slate".into(),
                iced::theme::Palette {
                    background: design::PANE,
                    text: design::TEXT,
                    primary: design::TEAL,
                    success: iced::Color::from_rgb8(83, 215, 184),
                    danger: design::DANGER,
                },
            )
        })
        .window(iced::window::Settings {
            size: iced::Size::new(1200.0, 820.0),
            min_size: Some(iced::Size::new(680.0, 480.0)),
            position: iced::window::Position::Centered,
            decorations: !cfg!(windows),
            transparent: cfg!(windows),
            exit_on_close_request: false,
            icon: iced::window::icon::from_rgba(
                include_bytes!("../../../assets/icons/tervix-256.rgba").to_vec(),
                256,
                256,
            )
            .ok(),
            ..Default::default()
        })
        .run_with(move || {
            let (mut state, task) = workspace::initialize();
            state.chrome.resize_window = Some(resize_window);
            state.chrome.icon = Some(iced::widget::image::Handle::from_rgba(
                256,
                256,
                include_bytes!("../../../assets/icons/tervix-256.rgba").to_vec(),
            ));
            state.terminals.set_provider(provider);
            state.monitor = Some(crate::monitoring::Monitor::new(monitoring));
            state.docker = Some(crate::docker::Docker::new(docker));
            match discovery {
                Ok(mut catalog) => {
                    catalog.refresh();
                    state.distributions = Some(catalog);
                }
                Err(error) => state.discovery_error = Some(error),
            }
            terminal::reconcile(&mut state);
            (state, task)
        })
}
