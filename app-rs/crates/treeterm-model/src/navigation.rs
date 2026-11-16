//! What the window shows when the session it showed goes away (`disconnectSession`'s
//! navigation in `store/app.ts`).

/// Where the window goes after the session it showed (its workspace or its session pane)
/// was disconnected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AfterDisconnect<Id> {
    /// The next session's workspace view: its active workspace, highlighted in its tree.
    Workspace(Id),
    /// The next session has no workspace: its session pane.
    SessionPane(Id),
    /// No session is left.
    Nothing,
}

/// Each session left, in sidebar order, and whether it has any workspace (one still loading
/// or failed counts).
pub fn after_disconnect<Id: Clone>(remaining: &[(Id, bool)]) -> AfterDisconnect<Id> {
    match remaining.first() {
        Some((next, true)) => AfterDisconnect::Workspace(next.clone()),
        Some((next, false)) => AfterDisconnect::SessionPane(next.clone()),
        None => AfterDisconnect::Nothing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_next_session_with_workspaces_shows_its_workspace() {
        assert_eq!(
            after_disconnect(&[("a", true), ("b", false)]),
            AfterDisconnect::Workspace("a")
        );
    }

    #[test]
    fn the_next_session_without_workspaces_opens_its_session_pane() {
        assert_eq!(
            after_disconnect(&[("a", false), ("b", true)]),
            AfterDisconnect::SessionPane("a")
        );
    }

    #[test]
    fn with_no_session_left_nothing_is_shown() {
        assert_eq!(after_disconnect::<&str>(&[]), AfterDisconnect::Nothing);
    }
}
