//! Observe pane presses before interactive children capture them, without widget focus operations.
use crate::workspace::Message;
use iced::advanced::{
    Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer,
    widget::{Operation, Tree},
};
use iced::{Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector, event};
use tervix_application::PaneId;

pub(super) struct PaneFocus<'a> {
    pub docking: bool,
    pub content: Element<'a, Message>,
    pub id: PaneId,
}

impl Widget<Message, Theme, Renderer> for PaneFocus<'_> {
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
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }
    fn operate(
        &self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
            .as_widget()
            .operate(&mut tree.children[0], layout, renderer, operation);
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
        // Publish first so any child action in the same event sees the correct
        // domain pane. Let text selection, buttons and pick lists process the
        // original event and retain their widget focus before capturing it.
        let pressed = match &event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => cursor.position(),
            Event::Touch(iced::touch::Event::FingerPressed { position, .. }) => Some(*position),
            _ => None,
        };
        let pane_pressed = pressed
            .is_some_and(|point| layout.bounds().contains(point) && viewport.contains(point));
        if self.docking
            && pane_pressed
            && pressed.is_some_and(|point| {
                Rectangle {
                    x: layout.bounds().x + 14.0,
                    y: layout.bounds().y + 14.0,
                    width: 28.0,
                    height: 28.0,
                }
                .contains(point)
            })
        {
            shell.publish(Message::DockStart(self.id));
            return event::Status::Captured;
        }
        if pane_pressed {
            shell.publish(Message::PanePressed(self.id));
        }
        let status = self.content.as_widget_mut().on_event(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
        // A plain background press must not fall through to an overlapping pane
        // during geometry animation. Interactive children have already seen it.
        if pane_pressed {
            event::Status::Captured
        } else {
            status
        }
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        if self.docking
            && cursor.position().is_some_and(|point| {
                viewport.contains(point)
                    && Rectangle {
                        x: layout.bounds().x + 14.0,
                        y: layout.bounds().y + 14.0,
                        width: 28.0,
                        height: 28.0,
                    }
                    .contains(point)
            })
        {
            return mouse::Interaction::Grab;
        }
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }
    fn overlay<'a>(
        &'a mut self,
        tree: &'a mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        translation: Vector,
    ) -> Option<overlay::Element<'a, Message, Theme, Renderer>> {
        self.content
            .as_widget_mut()
            .overlay(&mut tree.children[0], layout, renderer, translation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::{
        Point,
        widget::{button, checkbox},
    };

    #[test]
    fn captured_checkbox_and_button_presses_focus_pane_before_child_action() {
        let id = PaneId::from_uuid(uuid::Uuid::from_u128(1));
        let controls: [Element<'_, Message>; 2] = [
            checkbox("Reduced motion", false)
                .on_toggle(Message::ReducedMotion)
                .into(),
            button("Range")
                .on_press(Message::ReducedMotion(true))
                .into(),
        ];
        let renderer = crate::test_renderer::renderer(iced::Font::default());
        let bounds = Rectangle::with_size(Size::new(300.0, 100.0));
        for content in controls {
            let mut element = Element::new(PaneFocus {
                content,
                id,
                docking: false,
            });
            let mut tree = Tree::new(&element);
            let node = element.as_widget().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(Size::ZERO, bounds.size()),
            );
            let cursor = mouse::Cursor::Available(Point::new(10.0, 10.0));
            let mut messages = Vec::new();
            for event in [
                mouse::Event::ButtonPressed(mouse::Button::Left),
                mouse::Event::ButtonReleased(mouse::Button::Left),
            ] {
                let pressed = matches!(event, mouse::Event::ButtonPressed(_));
                let status = element.as_widget_mut().on_event(
                    &mut tree,
                    Event::Mouse(event),
                    Layout::new(&node),
                    cursor,
                    &renderer,
                    &mut iced::advanced::clipboard::Null,
                    &mut Shell::new(&mut messages),
                    &bounds,
                );
                if pressed {
                    assert_eq!(status, event::Status::Captured);
                }
            }
            assert!(
                matches!(messages.as_slice(), [Message::PanePressed(pane), Message::ReducedMotion(true)] if *pane == id)
            );
        }
    }
}
