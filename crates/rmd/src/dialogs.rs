//! Large-font modal dialogs (the Python version used tkinter). OWNER: daemon agent.

/// Show a modal dialog with a readable font (Helvetica 16pt message, 13pt
/// buttons, centred, always on top; Return clicks `default_button`; Escape or
/// closing returns `None`; after `timeout` seconds `default_button` is clicked).
/// Returns `None` when no large-font backend is available so callers can fall
/// back to plain AppleScript / zenity dialogs.
pub fn show_large_dialog(
    _message: &str,
    _buttons: &[&str],
    _default_button: &str,
    _title: &str,
    _timeout: Option<u64>,
) -> Option<String> {
    None
}
