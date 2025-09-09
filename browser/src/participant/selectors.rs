/// Selectors for UI elements in the classic frontend
pub mod classic {
    /// Selector for the leave button
    pub const LEAVE_BUTTON: &str = r#"[data-testid="trigger-leave-call"]"#;

    /// Selector for the mute/unmute button
    pub const MUTE_BUTTON: &str = r#"[data-testid="toggle-audio"]"#;

    /// Selector for the video on/off button
    pub const VIDEO_BUTTON: &str = r#"[data-testid="toggle-video"]"#;

    /// Selector for the screen share button
    pub const SCREEN_SHARE_BUTTON: &str = r#"[data-testid="toggle-screen-share"]"#;

    /// Selector for the name input field
    pub const NAME_INPUT: &str = r#"[data-testid="trigger-join-name"]"#;

    /// Selector for the join button
    pub const JOIN_BUTTON: &str = r#"button[type="submit"]:not([disabled])"#;
}

/// Selectors for UI elements in the lite frontend
pub mod lite {
    /// Selector for the join button
    pub const JOIN_BUTTON: &str = r#"button[data-test-id="join-button"]:not([disabled])"#;

    /// Selector for the leave button
    pub const LEAVE_BUTTON: &str = r#"[data-test-id="trigger-leave-call"]"#;

    /// Selector for the mute/unmute button
    pub const MUTE_BUTTON: &str = r#"[data-test-id="toggle-audio"]"#;

    /// Selector for the video on/off button
    pub const VIDEO_BUTTON: &str = r#"[data-test-id="toggle-video"]"#;

    /// Selector for the screen share button
    pub const SCREEN_SHARE_BUTTON: &str = r#"[data-test-id="toggle-screen-share"]"#;
}
