//! Tests for `web::services`.

use chrono::Utc;
use serde_json::{Value, json};

use super::*;
use crate::commands::history::pair_task_activities;
use crate::test_support::TestEnv;

fn payload(value: Value) -> Dict {
    value.as_object().cloned().expect("payload is an object")
}

fn read_json(env: &TestEnv, relative: &str) -> Value {
    serde_json::from_str(&env.read(relative)).expect("valid JSON file")
}

/// A config root with tasks, deferrals and an empty task log.
fn task_env(tasks: Value, procrastinated: Value) -> TestEnv {
    let mut env = TestEnv::new();
    env.set_now("2026-11-04T10:00:00");
    env.write("tasks/tasks.json", &tasks.to_string());
    env.write("tasks/procrastinate.json", &procrastinated.to_string());
    env
}

fn descriptions(tasks: &Value) -> Vec<&str> {
    tasks.as_array().unwrap().iter().map(|t| t["description"].as_str().unwrap()).collect()
}

fn code(result: WebResult) -> String {
    result.unwrap_err().code
}

#[test]
fn status_snapshot_returns_daily_panels() {
    let mut env = TestEnv::with_fixture();
    env.set_now("2026-11-09T10:30:00");
    let snapshot = status_snapshot(&Dict::new()).unwrap();
    serde_json::to_string(&snapshot).unwrap();
    assert_eq!(snapshot["config"]["rootDir"], json!(paths::resolve(env.root()).display().to_string()));
    assert_eq!(snapshot["config"]["activeId"], json!(0));
    assert_eq!(snapshot["config"]["error"], Value::Null);
    assert_eq!(snapshot["today"], json!({"date": "2026-11-09", "weekday": "monday", "parity": "even"}));
    assert_eq!(snapshot["schedule"]["isSkipped"], json!(false));
    let events = snapshot["schedule"]["events"].as_array().unwrap();
    assert!(!events.is_empty());
    for key in ["time", "label", "block", "syncable"] {
        assert!(events[0].get(key).is_some(), "event has {key}");
    }
    for key in ["tasks", "deadlines", "habits", "history", "timedDone"] {
        assert!(snapshot[key].is_array(), "{key} is a list");
    }
    assert_eq!(snapshot["taskTypes"], json!({"1": "read papers", "2": "gym work", "3": "coding", "4": "other"}));
    assert_eq!(snapshot["taskTypeColors"], json!({}));
}

#[test]
fn missing_schedule_still_opens_browser_setup() {
    let _env = TestEnv::new();
    let snapshot = status_snapshot(&Dict::new()).unwrap();
    assert!(snapshot["config"]["error"].as_str().unwrap().starts_with("[Errno 2] No such file or directory"));
    assert_eq!(snapshot["schedule"]["events"], json!([]));
    assert_eq!(snapshot["tasks"], json!([]));
}

#[test]
fn snapshot_sorts_tasks_by_section_and_priority() {
    let env = task_env(
        json!([
            {"description": "Later", "priority": 9, "type": "2", "alarm_from": "2026-11-10"},
            {"description": "Low", "priority": 2},
            {"description": "Deferred", "priority": 1, "type": "3", "due": " 2026-11-05 18:30 "},
            {"description": "High", "priority": "8", "type": null},
            {"description": "  "},
            "not a task",
        ]),
        json!([{"description": "Deferred", "since": "2026-11-01"}]),
    );
    let _ = &env;
    let rows = sorted_tasks();
    assert_eq!(
        rows.iter().map(|r| r["description"].as_str().unwrap()).collect::<Vec<_>>(),
        ["Deferred", "High", "Low", "Later"]
    );
    assert_eq!(rows[0]["procrastinateDays"], json!(3));
    assert_eq!(rows[0]["due"], json!("2026-11-05T18:30"));
    // A null type is shown as Python's str(None) and named after the default type.
    assert_eq!((&rows[1]["type"], &rows[1]["typeName"]), (&json!("None"), &json!("read papers")));
    assert_eq!((&rows[3]["alarmFrom"], &rows[3]["procrastinated"]), (&json!("2026-11-10"), &json!(false)));
}

#[test]
fn task_add_updates_existing_task_and_logs() {
    let env = task_env(json!([{"description": "Draft proposal", "priority": 5}]), json!([]));
    let result = task_add(&payload(json!({"description": "Draft proposal", "priority": 9}))).unwrap();
    assert_eq!(result, json!({"description": "Draft proposal", "priority": 9, "type": "1"}));
    assert_eq!(
        read_json(&env, "tasks/tasks.json"),
        json!([{"description": "Draft proposal", "priority": 9, "type": "1"}])
    );
    let log = read_json(&env, "tasks/tasks.log");
    assert_eq!(log[0]["action"], json!("updated"));
    assert_eq!(log[0]["metadata"], json!({"old_priority": 5}));
    assert_eq!(log[0]["timestamp"], json!(py::utc_now_isoformat()));
}

#[test]
fn task_add_validates_its_input() {
    let _env = task_env(json!([]), json!([]));
    let message = |value: Value| task_add(&payload(value)).unwrap_err().message;
    assert_eq!(message(json!({"priority": 5})), "description is required.");
    assert_eq!(message(json!({"description": "x", "priority": 0})), "priority must be between 1 and 10.");
    assert_eq!(message(json!({"description": "x", "priority": "high"})), "priority must be an integer.");
    assert_eq!(message(json!({"description": "x", "priority": 5, "type": "99"})), "unknown task type: 99");
    assert_eq!(
        message(json!({"description": "x", "priority": 5, "postpone": -1})),
        "postpone days must be non-negative."
    );
    assert_eq!(message(json!({"description": "x", "priority": 5, "postpone": "soon"})), "postpone must be an integer.");
    let added = task_add(&payload(json!({"description": " x ", "priority": "7", "postpone": 3}))).unwrap();
    assert_eq!(added, json!({"description": "x", "priority": 7, "type": "1", "alarm_from": "2026-11-07"}));
}

#[test]
fn task_add_wraps_save_failure() {
    let env = TestEnv::new();
    std::fs::create_dir_all(env.root().join("tasks/tasks.json")).unwrap();
    let error = task_add(&payload(json!({"description": "Draft proposal", "priority": 9}))).unwrap_err();
    assert_eq!(error.code, "storage_error");
    assert!(error.message.starts_with("failed to save tasks: [Errno 21]"), "{}", error.message);
}

#[test]
fn task_delete_removes_by_description() {
    let env = task_env(
        json!([{"description": "Draft proposal", "priority": 9}, {"description": "Read notes", "priority": 4}]),
        json!([]),
    );
    let result = task_delete(&payload(json!({"description": "Read notes"}))).unwrap();
    assert_eq!(result["deleted"], json!(1));
    assert_eq!(read_json(&env, "tasks/tasks.json"), json!([{"description": "Draft proposal", "priority": 9}]));
    assert_eq!(code(task_delete(&payload(json!({"description": "Nope"})))), "not_found");
    assert_eq!(
        code(task_delete(&payload(json!({"description": "Draft proposal", "action": "eaten"})))),
        "invalid_input"
    );
    assert_eq!(code(task_delete(&payload(json!({"description": "Draft proposal", "action": []})))), "internal_error");
}

#[test]
fn task_restore_undoes_a_removal() {
    for action in ["deleted", "cancelled", "dropped"] {
        let tasks = json!([
            {"description": "Draft proposal", "priority": 9, "type": "2"},
            {"description": "Read notes", "priority": 4, "type": "3", "alarm_from": "2026-01-02"},
            {"description": "Email Sam", "priority": 6},
        ]);
        let env = task_env(
            tasks.clone(),
            json!([{"description": "Read notes", "since": "2026-01-01"}, {"description": "Email Sam", "since": "2026-01-03"}]),
        );
        log_task_action("added", &tasks[1], None).unwrap();
        let log_before = read_json(&env, "tasks/tasks.log");

        let result = task_delete(&payload(json!({"description": "Read notes", "action": action}))).unwrap();
        assert_eq!(descriptions(&read_json(&env, "tasks/tasks.json")), ["Draft proposal", "Email Sam"]);
        let expected = if action == "deleted" { "completed" } else { action };
        let log = read_json(&env, "tasks/tasks.log");
        assert_eq!(pair_task_activities(log.as_array().unwrap())[0].status, expected);
        assert_eq!(result["undo"]["procrastinated"], json!({"since": "2026-01-01"}));

        let restored = task_restore(result["undo"].as_object().unwrap()).unwrap();
        assert_eq!(restored, json!({"description": "Read notes", "restored": 1}));
        // Back in its old place with every field, still deferred since the same day, and gone from the history.
        assert_eq!(read_json(&env, "tasks/tasks.json"), tasks);
        assert_eq!(
            read_json(&env, "tasks/procrastinate.json"),
            json!([{"description": "Email Sam", "since": "2026-01-03"}, {"description": "Read notes", "since": "2026-01-01"}])
        );
        let log = read_json(&env, "tasks/tasks.log");
        assert_eq!(log, log_before);
        assert!(pair_task_activities(log.as_array().unwrap()).is_empty());
    }
}

#[test]
fn task_restore_leaves_deferrals_alone_for_tasks_that_were_not_deferred() {
    let env = task_env(json!([{"description": "Read notes", "priority": 4}]), json!([]));
    let result = task_delete(&payload(json!({"description": "Read notes"}))).unwrap();
    assert_eq!(result["undo"]["procrastinated"], Value::Null);
    task_restore(result["undo"].as_object().unwrap()).unwrap();
    assert_eq!(read_json(&env, "tasks/tasks.json"), json!([{"description": "Read notes", "priority": 4}]));
    assert_eq!(read_json(&env, "tasks/procrastinate.json"), json!([]));
}

#[test]
fn task_restore_refuses_a_task_that_is_already_back() {
    let env = task_env(json!([{"description": "Read notes", "priority": 4, "type": "1"}]), json!([]));
    let result = task_delete(&payload(json!({"description": "Read notes"}))).unwrap();
    task_add(&payload(json!({"description": "Read notes", "priority": 7, "type": "1"}))).unwrap();
    let error = task_restore(result["undo"].as_object().unwrap()).unwrap_err();
    assert_eq!((error.code.as_str(), error.message.as_str()), ("duplicate", "“Read notes” is already on your list."));
    assert_eq!(read_json(&env, "tasks/tasks.json"), json!([{"description": "Read notes", "priority": 7, "type": "1"}]));
}

#[test]
fn task_restore_rejects_malformed_tokens() {
    let _env = task_env(json!([]), json!([]));
    for token in [
        json!({"action": "deleted", "tasks": []}),
        json!({"action": "finished", "tasks": [{"index": 0, "task": {"description": "Read notes"}}]}),
        json!({"action": "deleted", "tasks": [{"index": 0, "task": {"description": "Other"}}]}),
        json!({"action": "deleted", "tasks": [{"index": -1, "task": {"description": "Read notes"}}]}),
        json!({"action": "deleted", "tasks": [{"index": 1.0, "task": {"description": "Read notes"}}]}),
        json!({"action": "deleted", "tasks": [{"index": true, "task": {"description": "Read notes"}}]}),
    ] {
        let mut request = payload(token);
        request.insert("description".into(), json!("Read notes"));
        assert_eq!(code(task_restore(&request)), "invalid_input");
    }
}

#[test]
fn task_update_rejects_duplicate_description() {
    let tasks = json!([{"description": "Draft proposal", "priority": 9}, {"description": "Read notes", "priority": 4}]);
    let env = task_env(tasks.clone(), json!([]));
    let request = json!({"originalDescription": "Read notes", "description": "Draft proposal", "priority": 6});
    assert_eq!(code(task_update(&payload(request))), "duplicate");
    assert_eq!(read_json(&env, "tasks/tasks.json"), tasks);
    let missing = json!({"originalDescription": "Nope", "description": "Nope", "priority": 6});
    assert_eq!(task_update(&payload(missing)).unwrap_err().message, "task not found: Nope");
}

#[test]
fn task_edit_retains_metadata_and_removal_status() {
    let env = task_env(
        json!([{"description": "Write", "priority": 5, "type": "2", "alarm_from": "2026-10-07", "added": "saved"}]),
        json!([{"description": "Write", "since": "2026-10-01"}]),
    );
    let task =
        task_update(&payload(json!({"originalDescription": "Write", "description": "Write better", "priority": 8})))
            .unwrap();
    assert_eq!(
        task,
        json!({"description": "Write better", "priority": 8, "type": "2", "alarm_from": "2026-10-07", "added": "saved"})
    );
    // The deferral follows the rename.
    assert_eq!(
        read_json(&env, "tasks/procrastinate.json"),
        json!([{"description": "Write better", "since": "2026-10-01"}])
    );
    task_delete(&payload(json!({"description": "Write better", "action": "dropped"}))).unwrap();
    let log = read_json(&env, "tasks/tasks.log");
    assert_eq!(log.as_array().unwrap().last().unwrap()["action"], json!("dropped"));
    assert_eq!(log[0]["metadata"]["old_task"]["description"], json!("Write"));
}

#[test]
fn task_update_changes_type_and_deferral() {
    let env = task_env(
        json!([{"description": "Book flights", "priority": 8, "type": "4", "alarm_from": "2026-11-11"}]),
        json!([]),
    );
    let request = json!({"originalDescription": "Book flights", "description": "Book flights", "priority": 8, "type": "1", "postpone": 0});
    let task = task_update(&payload(request)).unwrap();
    assert_eq!(task, json!({"description": "Book flights", "priority": 8, "type": "1"}));
    let request =
        json!({"originalDescription": "Book flights", "description": "Book flights", "priority": 8, "postpone": 2});
    assert_eq!(task_update(&payload(request)).unwrap()["alarm_from"], json!("2026-11-06"));
    for bad in [json!(-1), json!("x"), json!(null)] {
        let request = json!({"originalDescription": "Book flights", "description": "Book flights", "priority": 8, "postpone": bad});
        assert_eq!(task_update(&payload(request)).unwrap_err().message, "postpone must be a non-negative integer.");
    }
    let request =
        json!({"originalDescription": "Book flights", "description": "Book flights", "priority": 8, "type": " 2"});
    assert_eq!(task_update(&payload(request)).unwrap_err().message, "unknown task type:  2");
    assert_eq!(read_json(&env, "tasks/tasks.json")[0]["alarm_from"], json!("2026-11-06"));
}

#[test]
fn timed_tasks_keep_their_finish_time_and_progress() {
    let env = task_env(json!([{"description": "Read notes", "priority": 4, "type": "1"}]), json!([]));
    let tasks = || read_json(&env, "tasks/tasks.json");

    task_add(&payload(json!({"description": "Ship slides", "priority": 7, "type": "1", "due": "2026-11-05T18:30"})))
        .unwrap();
    assert_eq!(
        tasks()[1],
        json!({"description": "Ship slides", "priority": 7, "type": "1", "due": "2026-11-05T18:30"})
    );
    assert_eq!(
        task_progress(&payload(json!({"description": "Ship slides", "progress": 40}))).unwrap(),
        json!({"description": "Ship slides", "progress": 40})
    );
    // Progress moves often, so it is saved without a history entry.
    let actions: Vec<Value> =
        read_json(&env, "tasks/tasks.log").as_array().unwrap().iter().map(|e| e["action"].clone()).collect();
    assert_eq!(actions, [json!("added")]);

    let rows = sorted_tasks();
    let row = |name: &str| rows.iter().find(|r| r["description"] == json!(name)).unwrap().clone();
    assert_eq!(
        (row("Ship slides")["due"].clone(), row("Ship slides")["progress"].clone()),
        (json!("2026-11-05T18:30"), json!(40))
    );
    assert_eq!((row("Read notes")["due"].clone(), row("Read notes")["progress"].clone()), (Value::Null, json!(0)));

    // Editing keeps the progress; an empty finish time makes it a normal task again.
    let edit = |value: Value| task_update(&payload(value)).unwrap();
    edit(
        json!({"originalDescription": "Ship slides", "description": "Ship slides", "priority": 7, "due": "2026-11-06T09:00"}),
    );
    assert_eq!(tasks()[1]["due"], json!("2026-11-06T09:00"));
    edit(json!({"originalDescription": "Ship slides", "description": "Ship slides", "priority": 7, "due": ""}));
    assert_eq!(tasks()[1], json!({"description": "Ship slides", "priority": 7, "type": "1", "progress": 40}));
    // Editing without a due field leaves the timer alone.
    edit(
        json!({"originalDescription": "Read notes", "description": "Read notes", "priority": 4, "due": "2026-11-05T08:00"}),
    );
    edit(json!({"originalDescription": "Read notes", "description": "Read notes", "priority": 5}));
    assert_eq!(tasks()[0]["due"], json!("2026-11-05T08:00"));

    task_progress(&payload(json!({"description": "Ship slides", "progress": "0"}))).unwrap();
    assert!(tasks()[1].get("progress").is_none());
    task_progress(&payload(json!({"description": "Ship slides", "progress": 100.0}))).unwrap();
    assert_eq!(tasks()[1]["progress"], json!(100));

    for bad in [json!(-1), json!(101), json!(12.5), json!(true), json!("half"), json!(null)] {
        let error = task_progress(&payload(json!({"description": "Ship slides", "progress": bad}))).unwrap_err();
        assert_eq!(
            (error.code.as_str(), error.message.as_str()),
            ("invalid_input", "progress must be a whole number from 0 to 100.")
        );
    }
    assert_eq!(code(task_progress(&payload(json!({"description": "Missing", "progress": 10})))), "not_found");
    let error = task_add(&payload(json!({"description": "Bad time", "priority": 5, "due": "tonight"}))).unwrap_err();
    assert_eq!(error.message, "due must be a local date and time like 2026-11-05T18:00.");
}

#[test]
fn subtasks_drive_a_tasks_progress() {
    let env = task_env(
        json!([
            {"description": "Write paper", "priority": 8, "type": "1", "progress": 30},
            {"description": "Read notes", "priority": 4, "type": "1"},
        ]),
        json!([]),
    );
    let tasks = || read_json(&env, "tasks/tasks.json");
    let set = |description: &str, subtasks: Value| {
        task_subtasks(&payload(json!({"description": description, "subtasks": subtasks})))
    };

    let result = set(
        "Write paper",
        json!([{"text": "  Outline ", "done": true}, {"text": "Draft", "done": false}, {"text": "Proofread"}]),
    )
    .unwrap();
    assert_eq!(
        result,
        json!({"description": "Write paper", "subtasks": [
            {"text": "Outline", "done": true}, {"text": "Draft", "done": false}, {"text": "Proofread", "done": false}
        ], "progress": 33})
    );
    // The derived progress is saved too; no history entry is written.
    assert_eq!(tasks()[0]["progress"], json!(33));
    assert_eq!(tasks()[0]["subtasks"][0], json!({"text": "Outline", "done": true}));
    assert!(!env.root().join("tasks/tasks.log").exists());

    let rows = sorted_tasks();
    assert_eq!(rows[0]["progress"], json!(33));
    assert_eq!(rows[1]["subtasks"], json!([]));

    // Halves round up, as in the browser: one of eight done is 13%.
    let pages: Vec<Value> = (0..8).map(|i| json!({"text": format!("Page {i}"), "done": i == 0})).collect();
    set("Read notes", Value::Array(pages)).unwrap();
    assert_eq!(tasks()[1]["progress"], json!(13));

    // While subtasks set the pace, the progress slider has no say.
    let error = task_progress(&payload(json!({"description": "Write paper", "progress": 90}))).unwrap_err();
    assert_eq!(error.message, "this task's progress follows its subtasks.");

    // Renaming, completing, and restoring a task keep its subtasks.
    task_update(&payload(
        json!({"originalDescription": "Write paper", "description": "Write the paper", "priority": 8}),
    ))
    .unwrap();
    let undo = task_delete(&payload(json!({"description": "Write the paper", "action": "deleted"}))).unwrap();
    task_restore(undo["undo"].as_object().unwrap()).unwrap();
    assert_eq!(tasks()[0]["subtasks"][2], json!({"text": "Proofread", "done": false}));

    // Every subtask done is 100%; clearing them keeps the last progress as the task's own.
    set("Write the paper", json!([{"text": "Outline", "done": true}, {"text": "Draft", "done": true}])).unwrap();
    assert_eq!(tasks()[0]["progress"], json!(100));
    assert_eq!(set("Write the paper", json!([])).unwrap()["progress"], json!(100));
    assert!(tasks()[0].get("subtasks").is_none());
    assert_eq!(
        task_progress(&payload(json!({"description": "Write the paper", "progress": 40}))).unwrap()["progress"],
        json!(40)
    );

    // None done drops the saved progress, like a slider at zero.
    set("Write the paper", json!([{"text": "Again", "done": false}])).unwrap();
    assert!(tasks()[0].get("progress").is_none());

    let long = "x".repeat(201);
    let many: Vec<Value> = (0..51).map(|i| json!({"text": i.to_string()})).collect();
    for bad in [
        json!("Outline"),
        json!([{"text": ""}]),
        json!([{"done": true}]),
        json!(["Outline"]),
        json!([{"text": long}]),
        json!([{"text": "Fine", "done": "yes"}]),
        Value::Array(many),
    ] {
        assert_eq!(code(set("Write the paper", bad)), "invalid_input");
    }
    assert_eq!(code(set("Missing", json!([]))), "not_found");
}

#[test]
fn timed_tasks_completed_today_still_count_toward_today() {
    let mut env = task_env(json!([{"description": "Open again", "priority": 5}]), json!([]));
    env.set_now("2026-11-05T16:00:00");
    let at = |day: u32| {
        let local = NaiveDate::from_ymd_opt(2026, 11, day).unwrap().and_hms_opt(15, 0, 0).unwrap();
        py::isoformat_aware(&crate::clock::local_from_naive(local).with_timezone(&Utc))
    };
    let entry = |when: String, action: &str, task: Value| json!({"timestamp": when, "action": action, "task": task});
    let log = json!([
        entry(
            at(5),
            "deleted",
            json!({"description": "Ship slides", "priority": 7, "type": "2", "due": "2026-11-05T18:00"})
        ),
        entry(at(5), "deleted", json!({"description": "Overdue essay", "priority": 6, "due": "2026-11-03T12:00"})),
        // Not today's: due tomorrow, finished yesterday, cancelled, not timed, or open again.
        entry(at(5), "deleted", json!({"description": "Tomorrow", "priority": 5, "due": "2026-11-06T09:00"})),
        entry(at(4), "deleted", json!({"description": "Yesterday", "priority": 5, "due": "2026-11-04T09:00"})),
        entry(at(5), "cancelled", json!({"description": "Cancelled", "priority": 5, "due": "2026-11-05T09:00"})),
        entry(at(5), "deleted", json!({"description": "Untimed", "priority": 5})),
        entry(at(5), "deleted", json!({"description": "Open again", "priority": 5, "due": "2026-11-05T10:00"})),
    ]);
    env.write("tasks/tasks.log", &log.to_string());

    let done = timed_done_today(NaiveDate::from_ymd_opt(2026, 11, 5).unwrap());
    assert_eq!(
        done.iter().map(|d| d["description"].clone()).collect::<Vec<_>>(),
        [json!("Ship slides"), json!("Overdue essay")]
    );
    assert_eq!(
        done[0],
        json!({"description": "Ship slides", "priority": 7, "type": "2", "due": "2026-11-05T18:00", "endedAt": "2026-11-05T15:00:00"})
    );
    assert_eq!(done[1]["type"], Value::Null);
}

fn deadline_env(deadlines: Value, task_types: &str) -> TestEnv {
    let mut env = TestEnv::new();
    env.set_now("2026-11-04T10:00:00");
    env.write("user_config_0/ddl.json", &deadlines.to_string());
    env.write("user_config_0/settings.toml", task_types);
    env
}

#[test]
fn deadline_add_accepts_iso_and_month_day_dates() {
    let env = deadline_env(json!([]), "");
    let result = deadline_add(&payload(json!({"event": "Submit paper", "date": "2026-05-10"}))).unwrap();
    assert_eq!((&result["event"], &result["deadline"]), (&json!("Submit paper"), &json!("2026-05-10")));
    assert_eq!(result["added"], json!(py::utc_now_isoformat()));
    assert_eq!(read_json(&env, "user_config_0/ddl.json")[0]["event"], json!("Submit paper"));
    // M.D picks the next such day: this year if it is still ahead, else next year.
    assert_eq!(
        deadline_add(&payload(json!({"event": "Taxes", "date": "4.15"}))).unwrap()["deadline"],
        json!("2027-04-15")
    );
    assert_eq!(
        deadline_add(&payload(json!({"event": "Gift", "date": "12.24"}))).unwrap()["deadline"],
        json!("2026-12-24")
    );
}

#[test]
fn deadline_dates_report_python_errors() {
    let _env = deadline_env(json!([]), "");
    let message = |date: &str| deadline_add(&payload(json!({"event": "Bad", "date": date}))).unwrap_err().message;
    assert_eq!(message("2026-02-30"), "invalid deadline date: day is out of range for month");
    assert_eq!(message("2026-13-01"), "invalid deadline date: time data '2026-13-01' does not match format '%Y-%m-%d'");
    assert_eq!(message("2026-05-100"), "invalid deadline date: unconverted data remains: 0");
    assert_eq!(message("0000-01-01"), "invalid deadline date: year 0 is out of range");
    assert_eq!(message("tomorrow"), "invalid deadline date: date must be YYYY-MM-DD or M.D");
    assert_eq!(message("a.5"), "invalid deadline date: invalid literal for int() with base 10: 'a'");
    assert_eq!(message("13.1"), "invalid deadline date: month must be in 1..12");
    assert_eq!(message("2.30"), "invalid deadline date: day is out of range for month");
    let overflow = deadline_add(&payload(json!({"event": "Bad", "date": "99999999999.1"}))).unwrap_err();
    assert_eq!(
        (overflow.code.as_str(), overflow.message.as_str()),
        ("internal_error", "signed integer is greater than maximum")
    );
    assert_eq!(parse_deadline_date("2026-1-5").unwrap(), "2026-01-05");
    assert_eq!(parse_deadline_date(" 3.5 ").unwrap(), "2027-03-05");
}

#[test]
fn deadline_type_round_trip_and_legacy_updates() {
    let env = deadline_env(json!([]), "[task_types]\n1 = \"Work\"\n2 = \"Personal\"\n");
    let file = || read_json(&env, "user_config_0/ddl.json");
    let created = deadline_add(&payload(json!({"event": "Submit paper", "date": "2026-10-27", "type": "1"}))).unwrap();
    assert_eq!(created["type"], json!("1"));
    assert_eq!(file()[0]["type"], json!("1"));

    let updated = deadline_update(&payload(
        json!({"originalEvent": "Submit paper", "event": "Submit draft", "date": "2026-10-28", "type": "2"}),
    ))
    .unwrap();
    assert_eq!(updated["type"], json!("2"));
    assert_eq!(updated["added"], created["added"]);

    // Existing callers can still change dates without supplying a type.
    deadline_update(&payload(json!({"originalEvent": "Submit draft", "event": "Submit draft", "date": "2026-10-29"})))
        .unwrap();
    deadline_add(&payload(json!({"event": "Submit draft", "date": "2026-10-30"}))).unwrap();
    assert_eq!(file()[0]["type"], json!("2"));

    deadline_update(&payload(
        json!({"originalEvent": "Submit draft", "event": "Submit draft", "date": "2026-10-30", "type": ""}),
    ))
    .unwrap();
    assert!(file()[0].get("type").is_none());
}

#[test]
fn deadline_rejects_unknown_type_without_writing() {
    let original = json!([{"event": "Submit paper", "deadline": "2026-10-27", "type": "1"}]);
    let env = deadline_env(original.clone(), "[task_types]\n1 = \"Work\"\n");
    let before = env.read("user_config_0/ddl.json");
    let request =
        payload(json!({"originalEvent": "Submit paper", "event": "Submit paper", "date": "2026-10-28", "type": "99"}));
    for handler in [deadline_add, deadline_update] {
        let error = handler(&request).unwrap_err();
        assert_eq!((error.code.as_str(), error.message.as_str()), ("invalid_input", "unknown deadline type: 99"));
        assert_eq!(env.read("user_config_0/ddl.json"), before);
    }
}

#[test]
fn deadline_update_preserves_removed_type() {
    let _env = deadline_env(
        json!([{"event": "Legacy deadline", "deadline": "2026-10-27", "type": "99"}]),
        "[task_types]\n1 = \"Work\"\n",
    );
    let updated = deadline_update(&payload(
        json!({"originalEvent": "Legacy deadline", "event": "Renamed deadline", "date": "2026-10-28", "type": "99"}),
    ))
    .unwrap();
    assert_eq!(updated["type"], json!("99"));
}

#[test]
fn deadline_snapshot_types_and_urgency_are_deterministic() {
    let _env = deadline_env(
        json!([
            {"event": "Typed", "deadline": "2026-10-27", "type": "1"},
            {"event": "Legacy", "deadline": "2026-10-26"},
            {"event": "Removed type", "deadline": "2026-10-25", "type": "99"},
            {"event": "Bad date", "deadline": "invalid", "type": "1"},
        ]),
        "[task_types]\n1 = \"Work\"\n",
    );
    let rows = deadline_rows(NaiveDate::from_ymd_opt(2026, 10, 26).unwrap()).unwrap();
    let row = |event: &str| rows.iter().find(|r| r["event"] == json!(event)).unwrap().clone();
    let typed = row("Typed");
    assert_eq!(
        [&typed["type"], &typed["typeName"], &typed["daysLeft"], &typed["status"]],
        [&json!("1"), &json!("Work"), &json!(1), &json!("urgent")]
    );
    assert_eq!((row("Legacy")["type"].clone(), row("Legacy")["typeName"].clone()), (Value::Null, Value::Null));
    assert_eq!(row("Legacy")["status"], json!("today"));
    assert_eq!(
        (row("Removed type")["typeName"].clone(), row("Removed type")["status"].clone()),
        (json!("99"), json!("overdue"))
    );
    assert_eq!(
        (row("Bad date")["typeName"].clone(), row("Bad date")["status"].clone()),
        (json!("Work"), json!("invalid"))
    );
    // Sorted by the deadline text, with invalid dates last here.
    assert_eq!(
        rows.iter().map(|r| r["event"].as_str().unwrap()).collect::<Vec<_>>(),
        ["Removed type", "Legacy", "Typed", "Bad date"]
    );
}

#[test]
fn deadline_repeat_rules_round_trip() {
    let env = deadline_env(json!([]), "");
    let created = deadline_add(&payload(json!({"event": "Rent", "date": "2026-11-30", "repeat": "Monthly"}))).unwrap();
    assert_eq!((&created["repeat"], &created["repeat_day"]), (&json!("monthly"), &json!(30)));
    // Moving the date resets the remembered day; keeping it does not.
    let moved =
        deadline_update(&payload(json!({"originalEvent": "Rent", "event": "Rent", "date": "2026-12-01"}))).unwrap();
    assert_eq!(moved["repeat_day"], json!(1));
    let weekly = deadline_update(&payload(
        json!({"originalEvent": "Rent", "event": "Rent", "date": "2026-12-01", "repeat": "weekly"}),
    ))
    .unwrap();
    assert!(weekly.get("repeat_day").is_none());
    let error = deadline_add(&payload(json!({"event": "Rent", "date": "2026-12-01", "repeat": "daily"}))).unwrap_err();
    assert_eq!(error.message, "unknown repeat rule: daily");
    deadline_update(&payload(json!({"originalEvent": "Rent", "event": "Rent", "date": "2026-12-01", "repeat": null})))
        .unwrap();
    assert!(read_json(&env, "user_config_0/ddl.json")[0].get("repeat").is_none());
}

#[test]
fn deadline_update_rejects_duplicate_event() {
    let deadlines = json!([{"event": "Submit paper", "deadline": "2026-05-10"}, {"event": "Pay invoice", "deadline": "2026-05-11"}]);
    let env = deadline_env(deadlines.clone(), "");
    let request = json!({"originalEvent": "Pay invoice", "event": "Submit paper", "date": "2026-05-12"});
    assert_eq!(code(deadline_update(&payload(request))), "duplicate");
    assert_eq!(read_json(&env, "user_config_0/ddl.json"), deadlines);
}

#[test]
fn deadline_delete_reports_missing_event() {
    let _env =
        deadline_env(json!([{"event": "A", "deadline": "2026-05-10"}, {"event": "A", "deadline": "2026-05-11"}]), "");
    assert_eq!(code(deadline_delete(&payload(json!({"event": "Missing"})))), "not_found");
    assert_eq!(deadline_delete(&payload(json!({"event": "A"}))).unwrap(), json!({"event": "A", "deleted": 2}));
}

#[test]
fn deadline_colors_round_trip_and_validate() {
    let env = deadline_env(json!([]), "[task_types]\n1 = \"Work\"\n");
    let file = || read_json(&env, "user_config_0/ddl.json");
    let created = deadline_add(&payload(json!({"event": "Launch", "date": "2026-11-20", "color": "Orange"}))).unwrap();
    assert_eq!(created["color"], json!("orange"));
    // Moving a deadline (as the calendar drag does) keeps its color.
    deadline_update(&payload(json!({"originalEvent": "Launch", "event": "Launch", "date": "2026-11-21"}))).unwrap();
    assert_eq!(file()[0]["color"], json!("orange"));

    let before = env.read("user_config_0/ddl.json");
    let error = deadline_update(&payload(
        json!({"originalEvent": "Launch", "event": "Launch", "date": "2026-11-21", "color": "url(x)"}),
    ))
    .unwrap_err();
    assert_eq!(error.message, "unknown deadline color: url(x)");
    assert_eq!(env.read("user_config_0/ddl.json"), before);

    deadline_update(&payload(json!({"originalEvent": "Launch", "event": "Launch", "date": "2026-11-21", "color": ""})))
        .unwrap();
    assert!(file()[0].get("color").is_none());

    env.write(
        "user_config_0/ddl.json",
        &json!([{"event": "Marked", "deadline": "2026-11-21", "color": "violet"}, {"event": "Tampered", "deadline": "2026-11-22", "color": "<b>"}])
            .to_string(),
    );
    let rows = deadline_rows(NaiveDate::from_ymd_opt(2026, 11, 4).unwrap()).unwrap();
    assert_eq!(rows[0]["color"], json!("violet"));
    assert_eq!(rows[1]["color"], Value::Null);
}

#[test]
fn habit_mark_writes_today_record() {
    let mut env = TestEnv::new();
    env.set_now("2026-04-28T09:00:00");
    env.write("user_config_0/habits.toml", "[habits]\n1 = \"Read\"\n2 = \"Exercise\"\n");
    let result = habit_mark(&payload(json!({"habitIds": ["1"]}))).unwrap();
    assert_eq!((&result["date"], &result["completed"]), (&json!("2026-04-28"), &json!({"1": "Read"})));
    assert_eq!(read_json(&env, "tasks/record.json")[0]["completed"], json!({"1": "Read"}));
    // Marking again replaces today's record; numbers are habit IDs too.
    habit_mark(&payload(json!({"habitIds": [2, "1"]}))).unwrap();
    let records = read_json(&env, "tasks/record.json");
    assert_eq!(records.as_array().unwrap().len(), 1);
    assert_eq!(records[0]["completed"], json!({"2": "Exercise", "1": "Read"}));

    let error = habit_mark(&payload(json!({"habitIds": ["1", "42"]}))).unwrap_err();
    assert_eq!(
        error.to_value(),
        json!({"code": "invalid_input", "message": "one or more habit IDs are invalid.", "details": {"invalidIds": ["42"]}})
    );
    assert_eq!(habit_mark(&payload(json!({"habitIds": "1"}))).unwrap_err().message, "habitIds must be a list.");
}

#[test]
fn history_rows_include_task_types() {
    let env = deadline_env(json!([]), "[task_types]\n1 = \"Work\"\n2 = \"Study\"\n");
    let log = json!([
        {"action": "added", "timestamp": "2026-10-05T09:00:00", "task": {"description": "Typed", "priority": 6, "type": "2"}},
        {"action": "deleted", "timestamp": "2026-10-05T10:00:00", "task": {"description": "Typed", "priority": 6, "type": "2"}},
        {"action": "added", "timestamp": "2026-10-05T09:00:00", "task": {"description": "Legacy", "priority": 3}},
        {"action": "dropped", "timestamp": "2026-10-05T11:00:00", "task": {"description": "Legacy", "priority": 3}},
    ]);
    env.write("tasks/tasks.log", &log.to_string());
    let rows = task_history(&payload(json!({"all": true}))).unwrap()["activities"].clone();
    let summary: Vec<[Value; 4]> = rows
        .as_array()
        .unwrap()
        .iter()
        .map(|r| [r["description"].clone(), r["type"].clone(), r["typeName"].clone(), r["status"].clone()])
        .collect();
    assert_eq!(
        summary,
        [
            [json!("Legacy"), json!("1"), json!("Work"), json!("dropped")],
            [json!("Typed"), json!("2"), json!("Study"), json!("completed")],
        ]
    );
    assert_eq!(rows[1]["duration"], json!("1h"));
    assert_eq!(task_history(&payload(json!({"count": "x"}))).unwrap()["activities"].as_array().unwrap().len(), 2);
    assert_eq!(task_history(&payload(json!({"count": 0}))).unwrap()["activities"].as_array().unwrap().len(), 1);
}

fn type_env(types: &str, tasks: Value, deadlines: Value, log: Value) -> TestEnv {
    let env = TestEnv::new();
    env.write("user_config_0/settings.toml", &format!("[settings]\nlanguage = \"en\"\n\n[task_types]\n{types}"));
    env.write("tasks/tasks.json", &tasks.to_string());
    env.write("user_config_0/ddl.json", &deadlines.to_string());
    env.write("tasks/tasks.log", &log.to_string());
    env
}

#[test]
fn task_type_save_adds_types_with_fresh_ids_and_renames() {
    let _env = type_env(
        "1 = \"Work\"\n3 = \"Study\"\n",
        json!([{"description": "Orphan", "priority": 5, "type": "7"}]),
        json!([{"event": "Old", "deadline": "2026-10-10", "type": "5"}]),
        json!([{"action": "deleted", "timestamp": "2026-10-01T09:00:00", "task": {"description": "Done", "priority": 3, "type": "9"}}]),
    );
    let save = |value: Value| task_type_save(&payload(value));

    // IDs 5, 7, and 9 are still used by a deadline, a task, and the history, so the new type gets 10.
    let created = save(json!({"name": "  Reading  "})).unwrap();
    assert_eq!(created["id"], json!("10"));
    assert_eq!(load_task_types(), payload(json!({"1": "Work", "3": "Study", "10": "Reading"})));

    save(json!({"id": "1", "name": "Job"})).unwrap();
    assert_eq!(load_task_types()["1"], json!("Job"));
    // A type only open tasks still use can be named again.
    save(json!({"id": "7", "name": "Errands"})).unwrap();
    assert_eq!(load_task_types()["7"], json!("Errands"));

    let duplicate = save(json!({"name": "study"})).unwrap_err();
    assert_eq!(
        (duplicate.code.as_str(), duplicate.message.as_str()),
        ("duplicate", "a task type named “study” already exists.")
    );
    assert_eq!(save(json!({"id": "42", "name": "Nowhere"})).unwrap_err().code, "not_found");
    let long = save(json!({"name": "x".repeat(61)})).unwrap_err();
    assert_eq!(long.message, "task type names can be at most 60 characters.");
}

#[test]
fn task_type_delete_moves_tasks_and_deadlines() {
    let env = type_env(
        "1 = \"Work\"\n2 = \"Study\"\n3 = \"Home\"\n",
        json!([
            {"description": "Read chapter", "priority": 5, "type": "2"},
            {"description": "Untyped", "priority": 4},
            {"description": "Clean", "priority": 3, "type": "3"},
        ]),
        json!([
            {"event": "Exam", "deadline": "2026-10-10", "type": "2"},
            {"event": "Report", "deadline": "2026-10-12", "type": "1"},
            {"event": "Plain", "deadline": "2026-10-13"},
        ]),
        json!([]),
    );
    let types =
        |values: &[&str]| values.iter().map(|v| if v.is_empty() { Value::Null } else { json!(v) }).collect::<Vec<_>>();
    let task_types_of = |file: &str| -> Vec<Value> {
        read_json(&env, file)
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t.get("type").cloned().unwrap_or(Value::Null))
            .collect()
    };
    for request in [json!({"id": "2"}), json!({"id": "2", "moveTo": "2"}), json!({"id": "2", "moveTo": "9"})] {
        assert!(task_type_delete(&payload(request)).is_err());
    }
    assert_eq!(load_task_types(), payload(json!({"1": "Work", "2": "Study", "3": "Home"})));
    assert_eq!(read_json(&env, "tasks/tasks.json")[0]["type"], json!("2"));

    let result = task_type_delete(&payload(json!({"id": "2", "moveTo": "3"}))).unwrap();
    assert_eq!(result, json!({"taskTypes": {"1": "Work", "3": "Home"}, "movedTasks": 1, "movedDeadlines": 1}));
    assert_eq!(load_task_types(), payload(json!({"1": "Work", "3": "Home"})));
    assert_eq!(task_types_of("tasks/tasks.json"), types(&["3", "", "3"]));
    assert_eq!(task_types_of("user_config_0/ddl.json"), types(&["3", "1", ""]));

    // Tasks saved without a type belong to the default (lowest) type and move with it.
    task_type_delete(&payload(json!({"id": "1", "moveTo": "3"}))).unwrap();
    assert_eq!(task_types_of("tasks/tasks.json"), types(&["3", "3", "3"]));
    assert_eq!(task_types_of("user_config_0/ddl.json"), types(&["3", "3", ""]));

    let error = task_type_delete(&payload(json!({"id": "3", "moveTo": "3"}))).unwrap_err();
    assert_eq!(error.message, "keep at least one task type.");
    assert_eq!(task_type_delete(&payload(json!({"id": "42"}))).unwrap_err().message, "task type not found: 42");
}

#[test]
fn task_type_delete_without_tasks_leaves_deadlines_uncategorized() {
    let env = type_env(
        "1 = \"Work\"\n2 = \"Study\"\n",
        json!([{"description": "Write", "priority": 5, "type": "1"}]),
        json!([{"event": "Exam", "deadline": "2026-10-10", "type": "2"}]),
        json!([]),
    );
    let result = task_type_delete(&payload(json!({"id": "2"}))).unwrap();
    assert_eq!((&result["movedTasks"], &result["movedDeadlines"]), (&json!(0), &json!(1)));
    assert_eq!(load_task_types(), payload(json!({"1": "Work"})));
    assert!(read_json(&env, "user_config_0/ddl.json")[0].get("type").is_none());
    assert_eq!(read_json(&env, "tasks/tasks.json")[0]["type"], json!("1"));
}

#[test]
fn task_types_keep_their_colors_when_the_list_changes() {
    let env = type_env(
        "1 = \"Work\"\n2 = \"Study\"\n3 = \"Home\"\n5 = \"Garden\"\n",
        json!([{"description": "Orphan", "priority": 5, "type": "7"}]),
        json!([]),
        json!([]),
    );
    // Without saved colors, types show the palette color of their place: blue, orange, aqua, yellow.
    task_type_delete(&payload(json!({"id": "2"}))).unwrap();
    // Deleting Study saves the others as they showed, instead of moving Home and Garden up the palette.
    assert_eq!(load_task_type_colors(), payload(json!({"1": "blue", "3": "aqua", "5": "yellow"})));

    // A new type takes the least-used color, in palette order.
    let created = task_type_save(&payload(json!({"name": "Errands"}))).unwrap();
    assert_eq!(created["color"], json!("orange"));
    assert_eq!(load_task_type_colors()[created["id"].as_str().unwrap()], json!("orange"));

    task_type_save(&payload(json!({"id": "1", "name": "Job"}))).unwrap();
    assert_eq!(load_task_type_colors()["1"], json!("blue"));
    // A type named back from its tasks keeps the hashed color it already showed ("7" -> 55 % 8 -> red).
    task_type_save(&payload(json!({"id": "7", "name": "Errands 2"}))).unwrap();
    assert_eq!(load_task_type_colors()["7"], json!("red"));
    // The settings keep their other sections and list the colors in type order.
    let settings = env.read("user_config_0/settings.toml");
    assert!(settings.starts_with("[settings]\nlanguage = \"en\"\n\n[task_types]\n"), "{settings}");
    assert!(
        settings
            .contains("[task_type_colors]\n1 = \"blue\"\n3 = \"aqua\"\n5 = \"yellow\"\n7 = \"red\"\n8 = \"orange\"\n"),
        "{settings}"
    );
}

#[test]
fn settings_task_types_round_trip() {
    let _env = type_env("1 = \"Work\"\n", json!([]), json!([]), json!([]));
    let result = settings_set_task_types(&payload(json!({"taskTypes": {" 1 ": " papers ", "7": 5}}))).unwrap();
    assert_eq!(result, json!({"taskTypes": {"1": "papers", "7": "5"}}));
    assert_eq!(settings_get_task_types(&Dict::new()).unwrap(), json!({"taskTypes": {"1": "papers", "7": "5"}}));
    let message = |value: Value| settings_set_task_types(&payload(value)).unwrap_err().message;
    assert_eq!(message(json!({"taskTypes": []})), "taskTypes must be an object.");
    assert_eq!(message(json!({"taskTypes": {" ": "x"}})), "task type keys must be non-empty.");
    assert_eq!(message(json!({"taskTypes": {"1": null}})), "task type '1' name must be non-empty.");
}

#[test]
fn saving_task_types_needs_a_settings_file() {
    let _env = TestEnv::new();
    let error = settings_set_task_types(&payload(json!({"taskTypes": {"1": "Work"}}))).unwrap_err();
    assert_eq!(error.code, "config_error");
    assert!(error.message.starts_with("failed to load settings: [Errno 2] No such file or directory"));
}

#[test]
fn snapshot_reports_saved_type_colors() {
    let env = TestEnv::new();
    env.write(
        "user_config_0/settings.toml",
        "[task_types]\n1 = \"Work\"\n\n[task_type_colors]\n1 = \"violet\"\n2 = \"plaid\"\n",
    );
    assert_eq!(load_task_type_colors(), payload(json!({"1": "violet"})));
}

#[test]
fn snapshot_paths_fall_back_to_a_legacy_settings_file() {
    let env = TestEnv::new();
    env.write("settings.toml", "[task_types]\n9 = \"Legacy\"\n");
    let snapshot_paths = snapshot_runtime_paths();
    assert_eq!(snapshot_paths.active_config_dir, paths::resolve(env.root()));
    assert_eq!(snapshot_paths.active_id, 0);
    assert_eq!(load_task_types(), payload(json!({"9": "Legacy"})));
}

#[test]
fn python_number_helpers() {
    assert_eq!(increment_digits("0"), "1");
    assert_eq!(increment_digits("009"), "10");
    assert_eq!(increment_digits("99999999999999999999999"), "100000000000000000000000");
    let mut ids = vec!["10", "x", "2", "999", "1000", "b"];
    ids.sort_by(|a, b| type_order(a, b));
    assert_eq!(ids, ["2", "10", "999", "1000", "b", "x"]);
    let owned: Vec<String> = ["10", "x", "2", "1000"].iter().map(|s| s.to_string()).collect();
    let mut refs: Vec<&String> = owned.iter().collect();
    sort_ids_numeric_or_999(&mut refs);
    assert_eq!(refs, ["2", "10", "x", "1000"]);
    assert_eq!(default_task_type(&Dict::new()), "1");
    assert_eq!(py_strip("\u{1c} a \u{1f}\n"), "a");
    assert_eq!(hashed_type_color("7"), "red");
    assert_eq!(add_days(NaiveDate::from_ymd_opt(9999, 12, 31).unwrap(), 1).unwrap_err(), "date value out of range");
}
