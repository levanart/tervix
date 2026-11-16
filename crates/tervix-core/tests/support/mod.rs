use tervix_core::{
    Axis, Layout, LayoutCommand, LayoutError, NavigationCommand, Pane, PaneId, Size, SplitRatio,
    ViewId,
};
use uuid::Uuid;

pub fn pane(value: u128) -> Pane {
    Pane {
        id: PaneId::from_uuid(Uuid::from_u128(value)),
        view: ViewId::from_uuid(Uuid::from_u128(value + 10_000)),
    }
}

pub fn split(target: u128, new: u128, axis: Axis) -> LayoutCommand {
    LayoutCommand::Split {
        target: pane(target).id,
        pane: pane(new),
        axis,
        ratio: SplitRatio::HALF,
    }
}

// 1 occupies the left half; 2/3 are stacked in the right half. Focus on 3.
pub fn three_panes() -> Result<Layout, LayoutError> {
    Layout::single(pane(1))
        .apply(split(1, 2, Axis::Vertical))?
        .apply(split(2, 3, Axis::Horizontal))
}

pub fn navigate(action: NavigationCommand) -> Result<LayoutCommand, LayoutError> {
    Ok(LayoutCommand::Navigate {
        action,
        viewport: Size::new(1000, 800)?,
        minimum: Size::new(80, 40)?,
    })
}
