use tervix_application::{
    Axis, Command, Direction, Layout, LayoutCommand, LayoutController, NavigationCommand, Pane,
    PaneId, Size, SplitRatio, ViewId,
};
use uuid::Uuid;

fn fresh_pane() -> Pane {
    Pane {
        id: PaneId::from_uuid(Uuid::new_v4()),
        view: ViewId::from_uuid(Uuid::new_v4()),
    }
}

pub(crate) fn layout_demo() -> Result<(), tervix_application::CommandError> {
    let first = fresh_pane();
    let second = fresh_pane();
    let mut controller = LayoutController::new(Layout::single(first));
    for (target, pane, axis) in [
        (first.id, second, Axis::Vertical),
        (second.id, fresh_pane(), Axis::Horizontal),
    ] {
        controller.dispatch(Command::Layout(LayoutCommand::Split {
            target,
            pane,
            axis,
            ratio: SplitRatio::HALF,
        }))?;
    }
    let viewport = Size::new(1000, 800)?;
    let minimum = Size::new(160, 80)?;
    print_geometry(&controller, viewport, minimum)?;
    let tiny = controller.layout().measure(Size::new(100, 50)?, minimum)?;
    println!(
        "Small viewport: canvas {:?}, overflow x={}, y={}",
        tiny.canvas(),
        tiny.overflows_x(),
        tiny.overflows_y()
    );
    let navigate = |action| {
        Command::Layout(LayoutCommand::Navigate {
            action,
            viewport,
            minimum,
        })
    };
    controller.dispatch(navigate(NavigationCommand::Focus(Direction::Up)))?;
    controller.dispatch(Command::FocusPrevious)?;
    controller.dispatch(navigate(NavigationCommand::Move(Direction::Left)))?;
    controller.dispatch(navigate(NavigationCommand::Resize {
        direction: Direction::Right,
        step_bps: 500,
    }))?;
    println!("After directional move and resize (same pane/view identities):");
    print_geometry(&controller, viewport, minimum)?;
    controller.dispatch(Command::Undo)?;
    controller.dispatch(Command::Undo)?;
    println!("Undo move/resize: original three-pane layout restored.");
    controller.dispatch(Command::Undo)?;
    println!(
        "Undo: {} panes remain. No shells were started.",
        controller.layout().panes().len()
    );
    Ok(())
}

fn print_geometry(
    controller: &LayoutController,
    viewport: Size,
    minimum: Size,
) -> Result<(), tervix_application::LayoutError> {
    let geometry = controller.layout().measure(viewport, minimum)?;
    for tile in geometry.panes() {
        println!(
            "Pane {} / view {}: {:?}{}",
            tile.pane.id.as_uuid(),
            tile.pane.view.as_uuid(),
            tile.rect,
            if tile.pane.id == controller.layout().focused() {
                " [focused]"
            } else {
                ""
            }
        );
    }
    Ok(())
}
