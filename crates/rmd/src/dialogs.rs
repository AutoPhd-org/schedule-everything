//! Large-font modal dialogs (the Python version used tkinter). OWNER: daemon agent.
//!
//! On macOS the dialog is an AppKit panel built by a JavaScript for Automation
//! script (`osascript -l JavaScript`, using the Objective-C bridge), so it
//! needs nothing beyond the OS. It mirrors the tkinter layout: a Helvetica
//! 16pt left-justified message with 24pt/20pt padding, Helvetica 13pt buttons
//! at least 14 characters wide packed from the right edge in list order, the
//! window centred on the main screen and kept above other windows. Return
//! clicks the default button, Escape or the close button dismiss it without a
//! choice, and an optional timeout clicks the default button.
//!
//! Other platforms have no large-font backend: [`show_large_dialog`] returns
//! `None` and callers fall back to zenity/kdialog.

/// Show a modal dialog with a readable font (Helvetica 16pt message, 13pt
/// buttons, centred, always on top; Return clicks `default_button`; Escape or
/// closing returns `None`; after `timeout` seconds `default_button` is clicked).
/// Returns `None` when no large-font backend is available so callers can fall
/// back to plain AppleScript / zenity dialogs.
pub fn show_large_dialog(
    message: &str,
    buttons: &[&str],
    default_button: &str,
    title: &str,
    timeout: Option<u64>,
) -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        macos::show(message, buttons, default_button, title, timeout)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (message, buttons, default_button, title, timeout);
        None
    }
}

/// Map the script's one-line result to the clicked label.
///
/// `button:N` is the N-th button, `timeout` is the default button (clicked by
/// the timer even when it is not one of `buttons`, like the tkinter version),
/// and anything else (`closed`, errors) is `None`.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn parse_outcome(output: &str, buttons: &[&str], default_button: &str) -> Option<String> {
    let output = output.trim();
    if output == "timeout" {
        return Some(default_button.to_string());
    }
    let index: usize = output.strip_prefix("button:")?.parse().ok()?;
    buttons.get(index).map(|label| (*label).to_string())
}

#[cfg(target_os = "macos")]
mod macos {
    use std::process::{Command, Stdio};
    use std::time::Duration;

    /// Environment variable carrying the dialog spec (JSON) to the script, so
    /// no text ever needs escaping for AppleScript or the command line.
    const SPEC_ENV: &str = "RMD_DIALOG_SPEC";

    /// Extra time allowed after the dialog's own timeout before the script is
    /// killed (osascript start-up takes a fraction of a second).
    const KILL_GRACE: Duration = Duration::from_secs(5);

    /// Builds and runs the panel; prints `button:N`, `timeout`, or `closed`.
    const DIALOG_SCRIPT: &str = r#"
ObjC.import('Cocoa');

function run() {
  const spec = JSON.parse(ObjC.unwrap($.NSProcessInfo.processInfo.environment.objectForKey('RMD_DIALOG_SPEC')));
  const app = $.NSApplication.sharedApplication;
  app.setActivationPolicy($.NSApplicationActivationPolicyAccessory);

  let outcome = 'closed';
  const finish = function (value, fromTimer) {
    outcome = value;
    if (fromTimer) {
      app.abortModal;
    } else {
      app.stopModal;
    }
  };

  ObjC.registerSubclass({
    name: 'RmdDialogTarget',
    methods: {
      'clicked:': {
        types: ['void', ['id']],
        implementation: function (sender) { finish('button:' + sender.tag, false); },
      },
      'timedOut:': {
        types: ['void', ['id']],
        implementation: function (timer) { finish('timeout', true); },
      },
      'windowShouldClose:': {
        types: ['bool', ['id']],
        implementation: function (sender) { finish('closed', false); return true; },
      },
    },
  });
  ObjC.registerSubclass({
    name: 'RmdDialogPanel',
    superclass: 'NSPanel',
    methods: {
      'cancelOperation:': {
        types: ['void', ['id']],
        implementation: function (sender) { finish('closed', false); },
      },
    },
  });

  const helvetica = function (size) {
    const font = $.NSFont.fontWithNameSize('Helvetica', size);
    return font.isNil() ? $.NSFont.systemFontOfSize(size) : font;
  };
  const target = $.RmdDialogTarget.alloc.init;
  const screen = $.NSScreen.mainScreen.frame;

  // Message: left-justified lines, 24pt horizontal and 20pt vertical padding.
  const label = $.NSTextField.wrappingLabelWithString(spec.message);
  label.font = helvetica(16);
  label.selectable = false;
  const maxTextWidth = Math.max(240, Math.floor(screen.size.width * 0.8) - 48);
  label.preferredMaxLayoutWidth = maxTextWidth;
  const textSize = label.cell.cellSizeForBounds($.NSMakeRect(0, 0, maxTextWidth, 100000));
  const textWidth = Math.ceil(textSize.width);
  const textHeight = Math.ceil(textSize.height);

  // Buttons: at least 14 characters wide, packed from the right edge in list
  // order (the first button is rightmost), 4pt apart from each other.
  const buttonFont = helvetica(13);
  const attributes = $.NSDictionary.dictionaryWithObjectForKey(buttonFont, $.NSFontAttributeName);
  const minButtonWidth = Math.ceil($.NSString.alloc.initWithString('0').sizeWithAttributes(attributes).width * 14) + 24;
  const buttons = [];
  let buttonsWidth = 0;
  let buttonHeight = 0;
  for (let i = 0; i < spec.buttons.length; i++) {
    const button = $.NSButton.buttonWithTitleTargetAction(spec.buttons[i], target, 'clicked:');
    button.font = buttonFont;
    button.tag = i;
    button.sizeToFit;
    const fitted = button.frame.size;
    const width = Math.max(minButtonWidth, Math.ceil(fitted.width));
    buttonHeight = Math.max(buttonHeight, Math.ceil(fitted.height));
    if (i === spec.default_index) {
      button.keyEquivalent = '\r';
    }
    buttons.push([button, width]);
    buttonsWidth += width + 8;
  }

  const width = Math.max(textWidth + 48, buttonsWidth + 48);
  const height = 20 + textHeight + 20 + buttonHeight + 16;
  const panel = $.RmdDialogPanel.alloc.initWithContentRectStyleMaskBackingDefer(
    $.NSMakeRect(0, 0, width, height),
    $.NSWindowStyleMaskTitled | $.NSWindowStyleMaskClosable,
    $.NSBackingStoreBuffered,
    false,
  );
  panel.title = spec.title;
  panel.releasedWhenClosed = false;
  panel.hidesOnDeactivate = false;
  panel.level = $.NSFloatingWindowLevel;
  panel.collectionBehavior =
    $.NSWindowCollectionBehaviorCanJoinAllSpaces | $.NSWindowCollectionBehaviorFullScreenAuxiliary;
  panel.delegate = target;

  const content = panel.contentView;
  label.frame = $.NSMakeRect(Math.floor((width - textWidth) / 2), 16 + buttonHeight + 20, textWidth, textHeight);
  content.addSubview(label);
  let right = width - 24;
  for (const [button, buttonWidth] of buttons) {
    right -= 4;
    button.frame = $.NSMakeRect(right - buttonWidth, 16, buttonWidth, buttonHeight);
    right -= buttonWidth + 4;
    content.addSubview(button);
  }

  // Centre on the main screen.
  const frame = panel.frame;
  panel.setFrameOrigin($.NSMakePoint(
    screen.origin.x + Math.floor((screen.size.width - frame.size.width) / 2),
    screen.origin.y + Math.floor((screen.size.height - frame.size.height) / 2),
  ));

  if (spec.timeout !== null) {
    const timer = $.NSTimer.timerWithTimeIntervalTargetSelectorUserInfoRepeats(spec.timeout, target, 'timedOut:', $(), false);
    $.NSRunLoop.currentRunLoop.addTimerForMode(timer, $.NSModalPanelRunLoopMode);
  }

  try {
    app.activateIgnoringOtherApps(true);
  } catch (error) {
    // Activation is best effort; the panel still floats above other windows.
  }
  panel.makeKeyAndOrderFront($());
  panel.orderFrontRegardless;
  if (spec.default_index >= 0 && spec.default_index < buttons.length) {
    panel.makeFirstResponder(buttons[spec.default_index][0]);
  }
  app.runModalForWindow(panel);
  panel.orderOut($());
  return outcome;
}
"#;

    pub(super) fn show(
        message: &str,
        buttons: &[&str],
        default_button: &str,
        title: &str,
        timeout: Option<u64>,
    ) -> Option<String> {
        let default_index = buttons.iter().position(|label| *label == default_button).map_or(-1, |i| i as i64);
        let spec = serde_json::json!({
            "message": message,
            "buttons": buttons,
            "default_index": default_index,
            "title": title,
            "timeout": timeout,
        });
        let mut command = Command::new("osascript");
        command.args(["-l", "JavaScript", "-e", DIALOG_SCRIPT]).env(SPEC_ENV, spec.to_string());
        let stdout = match timeout {
            None => {
                let output = command.stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
                if !output.status.success() {
                    return None;
                }
                String::from_utf8_lossy(&output.stdout).into_owned()
            }
            Some(seconds) => {
                let limit = Duration::from_secs(seconds).saturating_add(KILL_GRACE);
                match crate::platform::run_with_timeout(&mut command, Some(limit)) {
                    Ok(Some((true, stdout))) => stdout,
                    // The dialog outlived its timeout: the default was due anyway.
                    Ok(None) => return Some(default_button.to_string()),
                    Ok(Some((false, _))) | Err(_) => return None,
                }
            }
        };
        super::parse_outcome(&stdout, buttons, default_button)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcome_maps_to_clicked_label() {
        let buttons = ["Stop", "No", "Yes"];
        assert_eq!(parse_outcome("button:0\n", &buttons, "Yes").as_deref(), Some("Stop"));
        assert_eq!(parse_outcome("button:2", &buttons, "Yes").as_deref(), Some("Yes"));
        assert_eq!(parse_outcome("timeout\n", &buttons, "Yes").as_deref(), Some("Yes"));
        assert_eq!(parse_outcome("closed\n", &buttons, "Yes"), None);
        assert_eq!(parse_outcome("button:7", &buttons, "Yes"), None);
        assert_eq!(parse_outcome("", &buttons, "Yes"), None);
    }

    #[test]
    fn timeout_clicks_default_even_when_not_a_button() {
        assert_eq!(parse_outcome("timeout", &["OK"], "Later").as_deref(), Some("Later"));
    }

    /// Without a large-font backend callers fall back to zenity/kdialog.
    #[test]
    #[cfg(not(target_os = "macos"))]
    fn no_large_dialog_off_macos() {
        assert_eq!(show_large_dialog("Time for a break!", &["OK"], "OK", "Reminder", Some(1)), None);
    }

    /// Shows a real dialog on this Mac; it closes itself after two seconds.
    #[test]
    #[ignore = "opens a GUI dialog"]
    #[cfg(target_os = "macos")]
    fn gui_dialog_times_out_to_default_button() {
        let clicked = show_large_dialog(
            "Did you complete this task?\n\nWrite the report\nPriority: 9",
            &["Stop", "No", "Yes"],
            "Yes",
            "Urgent Task (1/1)",
            Some(2),
        );
        assert_eq!(clicked.as_deref(), Some("Yes"));
    }
}
