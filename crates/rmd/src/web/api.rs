//! Structured command dispatch for the local browser app.
//!
//! A request is `{"command": str, "payload": object}`; the response is
//! `{"ok": true, "data": ...}` or `{"ok": false, "error": {"code", "message",
//! "details"}}`. `rmd __api [JSON]` runs one request from the command line.

use std::io::Read;
use std::panic::{AssertUnwindSafe, catch_unwind};

use serde_json::{Value, json};

use crate::py::{self, Dict};
use crate::web::services::{self, WebError, WebResult};
use crate::web::{insights, pyjson, suggestions, workspace};

/// A browser API command.
pub type Handler = fn(&Dict) -> WebResult;

fn sync_clear(_payload: &Dict) -> WebResult {
    crate::commands::sync::clear_sync_plan().map_err(|e| {
        WebError::internal(services::os_error(&e, &crate::synced_schedule::resolve_synced_schedule_path()))
    })
}

/// Every command the browser can send (except `server_shutdown`, which only
/// the HTTP server handles).
pub const COMMANDS: [(&str, Handler); 29] = [
    ("config_read", workspace::config_read),
    ("config_save", workspace::config_save),
    ("workspace_info", workspace::workspace_info),
    ("language_set", workspace::language_set),
    ("service_action", workspace::service_action),
    ("export_pdf", workspace::export_pdf),
    ("status_snapshot", services::status_snapshot),
    ("task_add", services::task_add),
    ("task_update", services::task_update),
    ("task_progress", services::task_progress),
    ("task_subtasks", services::task_subtasks),
    ("task_delete", services::task_delete),
    ("task_restore", services::task_restore),
    ("task_history", services::task_history),
    ("task_type_save", services::task_type_save),
    ("task_type_delete", services::task_type_delete),
    ("suggestion_accept", suggestions::suggestion_accept),
    ("suggestion_reject", suggestions::suggestion_reject),
    ("suggestion_undo", suggestions::suggestion_undo),
    ("weekly_overview", insights::weekly_overview),
    ("daily_overview", insights::daily_overview),
    ("conference_calendar", crate::conferences::conference_calendar),
    ("deadline_add", services::deadline_add),
    ("deadline_update", services::deadline_update),
    ("deadline_delete", services::deadline_delete),
    ("habit_mark", services::habit_mark),
    ("settings_get_task_types", services::settings_get_task_types),
    ("settings_set_task_types", services::settings_set_task_types),
    ("sync_clear", sync_clear),
];

fn error_response(code: &str, message: impl Into<String>) -> Value {
    json!({"ok": false, "error": WebError::new(code, message).to_value()})
}

fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
    panic
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_else(|| "unexpected error".to_string())
}

/// Run `handler`, reporting failures (and panics) as error responses.
pub fn run_handler(handler: Handler, payload: &Dict) -> Value {
    match catch_unwind(AssertUnwindSafe(|| handler(payload))) {
        Ok(Ok(data)) => json!({"ok": true, "data": data}),
        Ok(Err(error)) => json!({"ok": false, "error": error.to_value()}),
        Err(panic) => error_response("internal_error", panic_message(panic.as_ref())),
    }
}

/// Validate a request and run its command.
pub fn dispatch(request: &Value) -> Value {
    dispatch_with(request, |name| COMMANDS.iter().find(|(command, _)| *command == name).map(|(_, handler)| *handler))
}

fn dispatch_with(request: &Value, lookup: impl Fn(&str) -> Option<Handler>) -> Value {
    let Value::Object(request) = request else {
        return error_response("invalid_request", "request must be an object.");
    };
    let command = match request.get("command") {
        Some(Value::String(command)) if !services::py_strip(command).is_empty() => command,
        _ => return error_response("invalid_request", "command is required."),
    };
    let empty = Value::Object(Dict::new());
    let Value::Object(payload) = request.get("payload").unwrap_or(&empty) else {
        return error_response("invalid_request", "payload must be an object.");
    };
    match lookup(command) {
        Some(handler) => run_handler(handler, payload),
        None => error_response("unknown_command", format!("unknown command: {command}")),
    }
}

/// The response to one raw JSON request.
pub fn respond(raw_request: &str) -> Value {
    match pyjson::loads(raw_request) {
        Err(message) => error_response("invalid_json", format!("invalid JSON request: {message}")),
        Ok(request @ Value::Object(_)) => dispatch(&request),
        Ok(_) => error_response("invalid_request", "request must be an object."),
    }
}

/// `rmd __api [JSON]`: read one request (the first argument, else standard
/// input), print the response, and exit 0.
pub fn cli_main(argv: Vec<String>) -> i32 {
    let raw_request = match argv.into_iter().next() {
        Some(request) => request,
        None => {
            let mut bytes = Vec::new();
            let _ = std::io::stdin().read_to_end(&mut bytes);
            String::from_utf8_lossy(&bytes).into_owned()
        }
    };
    println!("{}", py::dumps(&respond(&raw_request)));
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestEnv;

    fn echo(payload: &Dict) -> WebResult {
        Ok(json!({"echo": payload["value"]}))
    }

    fn broken(_payload: &Dict) -> WebResult {
        let mut details = Dict::new();
        details.insert("field".into(), json!("name"));
        Err(WebError::with_details("invalid_input", "bad input", details))
    }

    fn panics(_payload: &Dict) -> WebResult {
        panic!("not implemented yet")
    }

    fn lookup(name: &str) -> Option<Handler> {
        match name {
            "example" => Some(echo),
            "broken" => Some(broken),
            "panics" => Some(panics),
            _ => None,
        }
    }

    #[test]
    fn dispatch_returns_success_and_structured_errors() {
        let ok = dispatch_with(&json!({"command": "example", "payload": {"value": "ok"}}), lookup);
        assert_eq!(ok, json!({"ok": true, "data": {"echo": "ok"}}));
        let error = dispatch_with(&json!({"command": "broken", "payload": {}}), lookup);
        assert_eq!(
            error,
            json!({"ok": false, "error": {"code": "invalid_input", "message": "bad input", "details": {"field": "name"}}})
        );
        let crashed = dispatch_with(&json!({"command": "panics"}), lookup);
        assert_eq!(crashed["error"]["code"], json!("internal_error"));
        assert_eq!(crashed["error"]["message"], json!("not implemented yet"));
    }

    #[test]
    fn dispatch_validates_the_request_shape() {
        let code = |request: Value| dispatch(&request)["error"]["message"].as_str().unwrap().to_string();
        assert_eq!(code(json!([1])), "request must be an object.");
        assert_eq!(code(json!({"payload": {}})), "command is required.");
        assert_eq!(code(json!({"command": "  "})), "command is required.");
        assert_eq!(code(json!({"command": "status_snapshot", "payload": []})), "payload must be an object.");
        assert_eq!(code(json!({"command": "status_snapshot", "payload": null})), "payload must be an object.");
        assert_eq!(code(json!({"command": "missing", "payload": {}})), "unknown command: missing");
        for command in ["sync_generate", "sync_accept", "setup_turn", "setup_accept", "server_shutdown"] {
            assert_eq!(
                dispatch(&json!({"command": command, "payload": {}}))["error"]["code"],
                json!("unknown_command")
            );
        }
    }

    #[test]
    fn raw_requests_report_invalid_json_like_python() {
        assert_eq!(
            respond("{not json"),
            json!({"ok": false, "error": {"code": "invalid_json", "message": "invalid JSON request: Expecting property name enclosed in double quotes: line 1 column 2 (char 1)", "details": {}}})
        );
        assert_eq!(respond("[1]")["error"]["code"], json!("invalid_request"));
    }

    #[test]
    fn sync_clear_removes_todays_plan() {
        let mut env = TestEnv::new();
        let overlay = env.root().join("synced_schedule.toml");
        env.set_var("REMINDER_SYNCED_SCHEDULE_PATH", overlay.to_str().unwrap());
        std::fs::write(&overlay, "date = \"2026-11-04\"\n[schedule]\n").unwrap();
        let request = json!({"command": "sync_clear", "payload": {}});
        assert_eq!(dispatch(&request), json!({"ok": true, "data": {"cleared": true}}));
        assert!(!overlay.exists());
        assert_eq!(dispatch(&request), json!({"ok": true, "data": {"cleared": false}}));
    }
}
