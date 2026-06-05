/// Selectors for UI elements in the Hyper Core ("classic") frontend.
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

/// Selectors for UI elements in the Hyper Lite frontend.
pub mod lite {
    /// Selector for the display name input in the lobby.
    pub const NAME_INPUT: &str = r#"#meeting-lobby-display-name"#;

    /// Selector for the join button
    pub const JOIN_BUTTON: &str = r#"button[data-testid="join-button"]:not([disabled])"#;

    /// Selector for the leave button.
    pub const LEAVE_BUTTON: &str = r#"[data-testid="trigger-leave-call"], button[aria-label="Leave"]"#;

    /// Selector for the confirmation button in the current Lite leave dialog.
    pub const LEAVE_CONFIRM_BUTTON: &str = r#"[data-slot="alert-dialog-footer"] button:last-child"#;

    /// Selector for the mute/unmute button
    pub const MUTE_BUTTON: &str = r#"[data-testid="toggle-audio"], button[aria-label="Mute"], button[aria-label="Mute microphone"], button[aria-label="Unmute microphone"]"#;

    /// Selector for the video on/off button
    pub const VIDEO_BUTTON: &str = r#"[data-testid="toggle-video"], button[aria-label="Video"], button[aria-label="Turn off camera"], button[aria-label="Turn on camera"], button[aria-label="Turn video off"], button[aria-label="Turn video on"]"#;

    /// Selector for the screen share button
    pub const SCREEN_SHARE_BUTTON: &str = r#"[data-testid="toggle-screen-share"], button[aria-label="Share"], button[aria-label="Start screen share"], button[aria-label="Stop screen share"], button[aria-label="Share screen"]"#;

    /// Lobby audio toggle when the microphone is currently enabled.
    pub const LOBBY_DISABLE_AUDIO_BUTTON: &str = r#"button[aria-label="Mute microphone"]"#;

    /// Lobby video toggle when the camera is currently enabled.
    pub const LOBBY_DISABLE_VIDEO_BUTTON: &str = r#"button[aria-label="Turn off camera"]"#;
}
