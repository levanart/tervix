use uuid::Uuid;

macro_rules! identity {
    ($name:ident, $description:literal) => {
        #[doc = $description]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(Uuid);

        impl $name {
            /// Wraps an identity allocated by the application boundary.
            /// No randomness or OS access occurs in the domain.
            #[must_use]
            pub const fn from_uuid(value: Uuid) -> Self {
                Self(value)
            }

            /// Returns the underlying stable UUID.
            #[must_use]
            pub const fn as_uuid(self) -> Uuid {
                self.0
            }
        }
    };
}

identity!(WindowId, "Identity of a native OS window.");
identity!(WorkspaceId, "Identity of a named workspace.");
identity!(TabId, "Identity of a tab containing a complete layout.");
identity!(
    PaneId,
    "Identity of a tile, independent of its view instance."
);
identity!(
    ViewId,
    "Identity of a view instance; preserved when a pane moves."
);
identity!(
    SessionId,
    "Identity of a terminal session, independent of its tile."
);
