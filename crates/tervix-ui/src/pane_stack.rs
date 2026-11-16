//! Reconcile pane trees by identity while delegating layout/input/drawing to Iced.
//!
//! Hidden (stretched-away) panes retain trees but do not receive events, operations
//! or overlays. Closed panes are pruned; layout undo does not resurrect widget state.

use std::collections::HashMap;

use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::{Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector, event};
use tervix_application::PaneId;

use crate::workspace::Message;

pub(super) struct PaneStack<'a> {
    pub docking_enabled: bool,
    pub docking_active: bool,
    pub docking_valid: bool,
    stack: Element<'a, Message>,
    keys: Vec<Option<PaneId>>,
    retained: Vec<PaneId>,
}

#[derive(Default)]
struct State {
    keys: Vec<Option<PaneId>>,
    hidden: HashMap<PaneId, Tree>,
}

impl<'a> PaneStack<'a> {
    pub(super) fn new(
        stack: Element<'a, Message>,
        keys: Vec<Option<PaneId>>,
        retained: Vec<PaneId>,
    ) -> Self {
        Self {
            docking_enabled: false,
            docking_active: false,
            docking_valid: false,
            stack,
            keys,
            retained,
        }
    }
}

impl Widget<Message, Theme, Renderer> for PaneStack<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State {
            keys: self.keys.clone(),
            ..State::default()
        })
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.stack)]
    }

    fn diff(&self, tree: &mut Tree) {
        let state = tree.state.downcast_mut::<State>();
        let stack = &mut tree.children[0];
        state.hidden.retain(|id, _| self.retained.contains(id));
        if state.keys == self.keys {
            // Geometry/accent frames preserve ordering. Let Iced reconcile in
            // place instead of draining and rehashing every pane on every frame.
            self.stack.as_widget().diff(stack);
            return;
        }
        for (key, child) in state.keys.iter().zip(stack.children.drain(..)) {
            if let Some(id) = key {
                state.hidden.insert(*id, child);
            }
        }
        state.hidden.retain(|id, _| self.retained.contains(id));
        // New/unkeyed layers start fresh. Existing pane trees are moved before
        // Stack's positional diff, so nested scroll/input state follows PaneId.
        stack.children = self
            .keys
            .iter()
            .map(|key| {
                key.and_then(|id| state.hidden.remove(&id))
                    .unwrap_or_else(Tree::empty)
            })
            .collect();
        self.stack.as_widget().diff(stack);
        state.keys.clone_from(&self.keys);
    }

    fn size(&self) -> Size<Length> {
        self.stack.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.stack.as_widget().size_hint()
    }

    fn layout(
        &self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.stack
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
        self.stack.as_widget().draw(
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
        self.stack
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
        if self.docking_enabled
            && (self.docking_active
                || matches!(
                    event,
                    Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                ))
        {
            let point = match &event {
                Event::Mouse(mouse::Event::CursorMoved { .. }) => cursor.position(),
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => cursor.position(),
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                    cursor.position()
                }
                _ => None,
            };
            if self.docking_active
                && point.is_none()
                && matches!(
                    event,
                    Event::Mouse(mouse::Event::CursorMoved { .. })
                        | Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                )
            {
                shell.publish(Message::Cancel);
                return event::Status::Captured;
            }
            if let Some(point) = point {
                let inside = layout.bounds().contains(point) && viewport.contains(point);
                if self.docking_active && !inside {
                    shell.publish(Message::Cancel);
                    return event::Status::Captured;
                }
                if inside {
                    // Iced supplies a scroller-translated cursor: this is canvas
                    // space, independent of chrome, viewport clipping and scroll.
                    shell.publish(Message::DockPointer(iced::Point::new(
                        point.x - layout.bounds().x,
                        point.y - layout.bounds().y,
                    )));
                }
            }
            if self.docking_active {
                if matches!(event, Event::Mouse(mouse::Event::WheelScrolled { .. })) {
                    // Let the enclosing scroller process the wheel, but never
                    // retain a proposal measured before its viewport moves.
                    // Capturing here prevents Scrollable's on_scroll callback.
                    shell.publish(Message::Cancel);
                    return event::Status::Ignored;
                }
                if matches!(
                    event,
                    Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                ) {
                    shell.publish(Message::DockRelease);
                    return event::Status::Captured;
                }
                if matches!(event, Event::Mouse(_)) {
                    return event::Status::Captured;
                }
            }
        }
        self.stack.as_widget_mut().on_event(
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

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        if self.docking_active {
            return if self.docking_valid {
                mouse::Interaction::Grabbing
            } else {
                mouse::Interaction::NotAllowed
            };
        }
        self.stack.as_widget().mouse_interaction(
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
        self.stack
            .as_widget_mut()
            .overlay(&mut tree.children[0], layout, renderer, translation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::{self, Action};
    use iced::widget::scrollable;
    use tervix_application::Axis;

    #[test]
    fn real_scroller_maps_handle_and_captures_drag_release_outside_without_focusing()
    -> Result<(), &'static str> {
        use iced::{
            Padding, Point,
            widget::{Stack, button, container},
        };
        let id = PaneId::from_uuid(uuid::Uuid::from_u128(42));
        let make = |active| {
            let pane = crate::pane_focus::PaneFocus {
                docking: true,
                id,
                content: container(iced::widget::column![
                    iced::widget::Space::new(28, 28),
                    button("Close").on_press(Message::ReducedMotion(true))
                ])
                .padding(14)
                .width(200)
                .height(150)
                .into(),
            };
            let stack = Stack::new().push(iced::widget::Space::new(300, 1000)).push(
                container(Element::new(pane)).padding(Padding {
                    top: 300.,
                    left: 50.,
                    right: 0.,
                    bottom: 0.,
                }),
            );
            let mut stack = PaneStack::new(stack.into(), vec![None, Some(id)], vec![id]);
            stack.docking_enabled = true;
            stack.docking_active = active;
            stack.docking_valid = true;
            Element::from(scrollable(Element::new(stack)).width(300).height(200))
        };
        struct Scroll;
        impl Operation for Scroll {
            fn container(
                &mut self,
                _: Option<&iced::advanced::widget::Id>,
                _: Rectangle,
                children: &mut dyn FnMut(&mut dyn Operation),
            ) {
                children(self);
            }
            fn scrollable(
                &mut self,
                state: &mut dyn iced::advanced::widget::operation::Scrollable,
                _: Option<&iced::advanced::widget::Id>,
                _: Rectangle,
                _: Rectangle,
                _: Vector,
            ) {
                state.scroll_to(scrollable::AbsoluteOffset { x: 0., y: 300. });
            }
        }
        let renderer = crate::test_renderer::renderer(iced::Font::default());
        let bounds = Rectangle::with_size(Size::new(300., 200.));
        let mut element = make(false);
        let mut tree = Tree::new(&element);
        let node = element.as_widget().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, bounds.size()),
        );
        element
            .as_widget()
            .operate(&mut tree, Layout::new(&node), &renderer, &mut Scroll);
        let mut messages = Vec::new();
        let send = |element: &mut Element<'_, Message>,
                    tree: &mut Tree,
                    messages: &mut Vec<Message>,
                    event,
                    point| {
            element.as_widget_mut().on_event(
                tree,
                event,
                Layout::new(&node),
                mouse::Cursor::Available(point),
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut Shell::new(messages),
                &bounds,
            )
        };
        send(
            &mut element,
            &mut tree,
            &mut messages,
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            Point::new(70., 20.),
        );
        assert!(
            matches!(messages.as_slice(),[Message::DockPointer(p),Message::DockStart(pane)]
            if *p == Point::new(70.,320.) && *pane == id),
            "{messages:?}"
        );
        // Control below the handle still receives a normal press and release.
        messages.clear();
        for event in [
            mouse::Event::ButtonPressed(mouse::Button::Left),
            mouse::Event::ButtonReleased(mouse::Button::Left),
        ] {
            send(
                &mut element,
                &mut tree,
                &mut messages,
                Event::Mouse(event),
                Point::new(80., 55.),
            );
        }
        assert!(
            messages
                .iter()
                .any(|m| matches!(m, Message::ReducedMotion(true)))
        );
        assert!(!messages.iter().any(|m| matches!(m, Message::DockStart(_))));
        element = make(true);
        tree.diff(&element);
        messages.clear();
        send(
            &mut element,
            &mut tree,
            &mut messages,
            Event::Mouse(mouse::Event::CursorMoved {
                position: Point::new(80., 40.),
            }),
            Point::new(80., 40.),
        );
        assert!(
            matches!(messages.as_slice(),[Message::DockPointer(p)] if *p == Point::new(80.,340.))
        );
        messages.clear();
        send(
            &mut element,
            &mut tree,
            &mut messages,
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            Point::new(90., 60.),
        );
        assert!(
            matches!(messages.as_slice(),[Message::DockPointer(p),Message::DockRelease] if *p == Point::new(90.,360.))
        );
        messages.clear();
        send(
            &mut element,
            &mut tree,
            &mut messages,
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            Point::new(90., 250.),
        );
        assert!(matches!(messages.as_slice(), [Message::Cancel]));
        messages.clear();
        send(
            &mut element,
            &mut tree,
            &mut messages,
            Event::Mouse(mouse::Event::WheelScrolled {
                delta: mouse::ScrollDelta::Lines { x: 0., y: -1. },
            }),
            Point::new(90., 60.),
        );
        assert!(matches!(messages.as_slice(), [Message::Cancel]));
        Ok(())
    }

    fn find(tree: &mut Tree, tag: tree::Tag) -> Option<&mut Tree> {
        if tree.tag == tag {
            Some(tree)
        } else {
            tree.children.iter_mut().find_map(|child| find(child, tag))
        }
    }

    fn stack(tree: &mut Tree) -> Result<&mut Tree, &'static str> {
        find(tree, tree::Tag::of::<State>()).ok_or("presentation contains pane stack")
    }

    fn pane(tree: &mut Tree, id: PaneId) -> Result<&mut Tree, &'static str> {
        let tree = stack(tree)?;
        let state = tree.state.downcast_mut::<State>();
        if let Some(index) = state.keys.iter().position(|key| *key == Some(id)) {
            Ok(&mut tree.children[0].children[index])
        } else {
            state.hidden.get_mut(&id).ok_or("hidden pane retained")
        }
    }

    fn scroll(tree: &mut Tree, id: PaneId, offset: Option<f32>) -> Result<f32, &'static str> {
        use iced::advanced::widget::{Id, operation::Scrollable};

        struct Probe {
            offset: Option<f32>,
            translation: f32,
        }
        impl Operation for Probe {
            fn container(
                &mut self,
                _: Option<&Id>,
                _: Rectangle,
                children: &mut dyn FnMut(&mut dyn Operation),
            ) {
                children(self);
            }

            fn scrollable(
                &mut self,
                state: &mut dyn Scrollable,
                _: Option<&Id>,
                _: Rectangle,
                _: Rectangle,
                translation: Vector,
            ) {
                self.translation = translation.y;
                if let Some(y) = self.offset {
                    state.scroll_to(scrollable::AbsoluteOffset { x: 0.0, y });
                }
            }
        }
        // Operate on the actual presentation scroll tree. A fixed overflowing
        // measurement makes offsets observable even when a stretched pane's
        // current content would fit; Iced's scroll state is private.
        let probe: Element<'_, Message> = scrollable(iced::widget::Space::new(300, 2000))
            .width(300)
            .height(100)
            .into();
        let scroll =
            find(pane(tree, id)?, probe.as_widget().tag()).ok_or("real pane scrollable")?;
        let renderer = crate::test_renderer::renderer(iced::Font::default());
        let node = probe.as_widget().layout(
            scroll,
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::new(300.0, 100.0)),
        );
        let mut operation = Probe {
            offset,
            translation: 0.0,
        };
        probe
            .as_widget()
            .operate(scroll, Layout::new(&node), &renderer, &mut operation);
        Ok(operation.translation)
    }

    fn reconcile(state: &mut workspace::Workspace, tree: &mut Tree, message: Message) {
        let _ = workspace::update(state, message);
        tree.diff(workspace::view(state));
    }

    fn action(state: &mut workspace::Workspace, tree: &mut Tree, action: Action) {
        reconcile(state, tree, Message::Action(action));
    }

    #[test]
    fn presentation_tree_keeps_independent_scroll_through_focus_config_and_resize()
    -> Result<(), &'static str> {
        for reduced in [false, true] {
            let (mut state, _) = workspace::initialize();
            let _ = workspace::update(&mut state, Message::ReducedMotion(reduced));
            let first = state.controller.layout().focused();
            state.configuring = true;
            let _ = workspace::update(&mut state, Message::Action(Action::Split(Axis::Vertical)));
            let second = state.controller.layout().focused();
            // Help is genuinely scrollable; terminal grids now fill their allocation.
            for pane in state.controller.layout().panes() {
                state.views.insert(pane.view, workspace::PaneView::Help);
            }
            let mut tree = Tree::new(workspace::view(&state));
            scroll(&mut tree, first, Some(37.0))?;
            scroll(&mut tree, second, Some(119.0))?;
            for message in [
                Message::Action(Action::Focus(first)),
                Message::Action(Action::Focus(second)),
                Message::Action(Action::Configure),
                Message::Resized(Size::new(680.0, 480.0)),
                Message::Action(Action::Configure),
                Message::Frame(std::time::Instant::now() + std::time::Duration::from_secs(1)),
            ] {
                reconcile(&mut state, &mut tree, message);
                assert_eq!(scroll(&mut tree, first, None)?, 37.0);
                assert_eq!(scroll(&mut tree, second, None)?, 119.0);
            }
        }
        Ok(())
    }

    #[test]
    fn presentation_tree_retains_hidden_scroll_across_stretch_and_prunes_closed_panes()
    -> Result<(), &'static str> {
        for reduced in [false, true] {
            let (mut state, _) = workspace::initialize();
            let _ = workspace::update(&mut state, Message::ReducedMotion(reduced));
            let first = state.controller.layout().focused();
            state.configuring = true;
            let _ = workspace::update(&mut state, Message::Action(Action::Split(Axis::Horizontal)));
            let second = state.controller.layout().focused();
            for pane in state.controller.layout().panes() {
                state.views.insert(pane.view, workspace::PaneView::Help);
            }
            let mut tree = Tree::new(workspace::view(&state));
            scroll(&mut tree, first, Some(73.0))?;
            scroll(&mut tree, second, Some(181.0))?;
            for command in [
                Action::Zoom,
                Action::Focus(first),
                Action::Focus(second),
                Action::Zoom,
            ] {
                action(&mut state, &mut tree, command);
                assert_eq!(scroll(&mut tree, first, None)?, 73.0);
                assert_eq!(scroll(&mut tree, second, None)?, 181.0);
                assert_eq!(
                    stack(&mut tree)?.state.downcast_ref::<State>().hidden.len(),
                    usize::from(state.zoomed.is_some())
                );
            }
            action(&mut state, &mut tree, Action::Zoom);
            action(&mut state, &mut tree, Action::Close);
            let cache = stack(&mut tree)?.state.downcast_ref::<State>();
            assert!(!cache.hidden.contains_key(&second));
            assert_eq!(scroll(&mut tree, first, None)?, 73.0);
        }
        Ok(())
    }
}
