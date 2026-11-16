//! Observe the terminal widget's allocated content bounds during events, never during view/draw.
use crate::workspace::Message;
use iced::advanced::{
    Clipboard, Layout, Shell, Widget, layout, mouse, renderer,
    widget::{Tree, tree},
};
use iced::{Element, Event, Length, Rectangle, Renderer, Size, Theme, event};
use tervix_application::{ViewId, terminal::GridSize};

pub(super) struct Measured<'a> {
    pub content: Element<'a, Message>,
    pub id: ViewId,
    pub enabled: bool,
}
impl Widget<Message, Theme, Renderer> for Measured<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Option<GridSize>>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(None::<GridSize>)
    }
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }
    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }
    fn layout(
        &self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget()
            .layout(&mut tree.children[0], renderer, limits)
    }
    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        use iced::advanced::Renderer as _;
        if let Some(clip) = layout.bounds().intersection(viewport) {
            renderer.with_layer(clip, |renderer| {
                self.content.as_widget().draw(
                    &tree.children[0],
                    renderer,
                    theme,
                    style,
                    layout,
                    cursor,
                    viewport,
                );
            });
        }
    }
    fn on_event(
        &mut self,
        tree: &mut Tree,
        event: Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) -> event::Status {
        if self.enabled {
            let bounds = layout.bounds();
            let size = GridSize::new(
                (bounds.width / super::terminal::CELL_WIDTH) as u16,
                (bounds.height / super::terminal::CELL_HEIGHT) as u16,
            );
            let previous = tree.state.downcast_mut::<Option<GridSize>>();
            if *previous != Some(size) {
                *previous = Some(size);
                shell.publish(Message::TerminalMeasured(self.id, size));
            }
        }
        self.content.as_widget_mut().on_event(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        )
    }
}
