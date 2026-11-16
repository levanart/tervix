//! I/O-free workspace identities and transactional tiling rules.

mod geometry;
mod ids;
mod layout;
mod navigation;
mod process;
mod workspace;

pub use process::{ProcessId, ProcessIdentity};

pub use geometry::{LayoutGeometry, PaneGeometry, Rect, Size, SplitBoundary};
pub use ids::{PaneId, SessionId, TabId, ViewId, WindowId, WorkspaceId};
pub use layout::{Axis, Layout, LayoutCommand, LayoutError, LayoutNode, Pane, SplitRatio};
pub use navigation::{Direction, NavigationCommand};
pub use workspace::{Tab, Window, Workspace, WorkspaceCommand, WorkspaceError};
