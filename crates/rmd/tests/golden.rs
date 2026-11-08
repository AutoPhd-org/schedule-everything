//! Golden parity tests.
//!
//! Replays `tests/golden/cases.json` against the `rmd` binary and compares the
//! exit code, stdout/stderr (as JSON, exact text, or ignored, per case) and
//! every file under the config root with the outputs recorded from the
//! original Python implementation (`tests/golden/golden.py record`).
//!
//! `GOLDEN_FILTER=substring cargo test --test golden` runs a subset.
//! Accepted, documented differences live in `tests/golden/known_differences.json`.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden").canonicalize().expect("tests/golden exists")
}

fn copy_dir(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).unwrap();
        }
    }
}

fn snapshot(root: &Path) -> BTreeMap<String, String> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, String>) {
        let mut entries: Vec<_> = fs::read_dir(dir).unwrap().flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let path = entry.path();
            if path.is_dir() {
                walk(base, &path, out);
            } else {
                let relative = path.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/");
                let bytes = fs::read(&path).unwrap();
                let text = String::from_utf8(bytes.clone()).unwrap_or_else(|_| {
                    use sha2::Digest;
                    format!("<binary sha256:{}>", hex::encode(sha2::Sha256::digest(&bytes)))
                });
                out.insert(relative, text);
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

fn normalize(text: &str, root: &Path, home: &Path) -> String {
    let mut text = text.to_string();
    for (base, token) in [(root, "<ROOT>"), (home, "<HOME>")] {
        let mut forms = vec![base.to_string_lossy().to_string()];
        if let Ok(resolved) = base.canonicalize() {
            forms.push(resolved.to_string_lossy().to_string());
        }
        forms.sort_by_key(|f| std::cmp::Reverse(f.len()));
        forms.dedup();
        for form in forms {
            text = text.replace(&form, token);
        }
    }
    text
}

struct Outcome {
    exit: i64,
    stdout: String,
    stderr: String,
    files: BTreeMap<String, String>,
}

fn run_case(case: &Value, timezone: &str, fixture: &Path) -> Outcome {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    let home = tmp.path().join("home");
    copy_dir(fixture, &root);
    fs::create_dir_all(&home).unwrap();
    if let Some(files) = case["files"].as_object() {
        for (relative, content) in files {
            let target = root.join(relative);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(target, content.as_str().unwrap()).unwrap();
        }
    }
    let root_str = root.to_string_lossy().to_string();
    let argv: Vec<String> =
        case["argv"].as_array().unwrap().iter().map(|a| a.as_str().unwrap().replace("<ROOT>", &root_str)).collect();
    let mut command = Command::new(env!("CARGO_BIN_EXE_rmd"));
    command
        .args(&argv)
        .current_dir(&root)
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into()))
        .env("TZ", timezone)
        .env("HOME", &home)
        .env("REMINDER_CONFIG_DIR", &root)
        .env("RMD_NOW", case["now"].as_str().unwrap())
        .env("NO_COLOR", "1")
        .env("COLUMNS", "100")
        .env("LANG", "en_US.UTF-8")
        .env("TERM", "dumb")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(env) = case["env"].as_object() {
        for (key, value) in env {
            command.env(key, value.as_str().unwrap());
        }
    }
    let stdin = case["stdin"].as_str();
    command.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() });
    let mut child = command.spawn().expect("spawn rmd");
    if let Some(input) = stdin {
        child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    }
    let output = child.wait_with_output().unwrap();
    let files = snapshot(&root).into_iter().map(|(k, v)| (k, normalize(&v, &root, &home))).collect();
    Outcome {
        exit: i64::from(output.status.code().unwrap_or(-1)),
        stdout: normalize(&String::from_utf8_lossy(&output.stdout), &root, &home),
        stderr: normalize(&String::from_utf8_lossy(&output.stderr), &root, &home),
        files,
    }
}

fn clip(text: &str) -> String {
    let flat = text.replace('\n', "⏎");
    if flat.chars().count() > 160 { format!("{}…", flat.chars().take(160).collect::<String>()) } else { flat }
}

/// First path at which two JSON values differ.
fn json_diff(path: &str, expected: &Value, actual: &Value) -> Option<String> {
    match (expected, actual) {
        (Value::Object(e), Value::Object(a)) => {
            let ek: Vec<&String> = e.keys().collect();
            let ak: Vec<&String> = a.keys().collect();
            if ek != ak {
                return Some(format!("{path}: keys {ek:?} != {ak:?}"));
            }
            e.iter().find_map(|(k, v)| json_diff(&format!("{path}.{k}"), v, &a[k]))
        }
        (Value::Array(e), Value::Array(a)) => {
            if e.len() != a.len() {
                return Some(format!("{path}: {} items != {} items", e.len(), a.len()));
            }
            e.iter().zip(a).enumerate().find_map(|(i, (ev, av))| json_diff(&format!("{path}[{i}]"), ev, av))
        }
        _ if expected == actual => None,
        _ => Some(format!("{path}: expected {} got {}", clip(&expected.to_string()), clip(&actual.to_string()))),
    }
}

fn text_diff(label: &str, expected: &str, actual: &str) -> Option<String> {
    if expected == actual {
        return None;
    }
    let (el, al): (Vec<&str>, Vec<&str>) = (expected.lines().collect(), actual.lines().collect());
    for i in 0..el.len().max(al.len()) {
        let (e, a) = (el.get(i).copied().unwrap_or("<missing>"), al.get(i).copied().unwrap_or("<missing>"));
        if e != a {
            return Some(format!("{label} line {}: expected {:?} got {:?}", i + 1, clip(e), clip(a)));
        }
    }
    Some(format!("{label}: differs in trailing whitespace/newlines ({:?} vs {:?})", clip(expected), clip(actual)))
}

fn compare_stream(label: &str, mode: &str, expected: &str, actual: &str) -> Option<String> {
    match mode {
        "json" => match (serde_json::from_str::<Value>(expected), serde_json::from_str::<Value>(actual)) {
            (Ok(e), Ok(a)) => json_diff(label, &e, &a),
            _ => text_diff(label, expected, actual),
        },
        "text" => text_diff(label, expected, actual),
        _ => None,
    }
}

fn compare(case: &Value, expected: &Value, actual: &Outcome) -> Vec<String> {
    let mut diffs = Vec::new();
    let expected_exit = expected["exit"].as_i64().unwrap();
    if expected_exit != actual.exit {
        diffs.push(format!(
            "exit: expected {expected_exit} got {} (stdout {:?}, stderr {:?})",
            actual.exit,
            clip(&actual.stdout),
            clip(&actual.stderr)
        ));
    }
    let stdout_mode = case["stdout"].as_str().unwrap_or("ignore");
    let stderr_mode = case["stderr"].as_str().unwrap_or("ignore");
    diffs.extend(compare_stream("stdout", stdout_mode, expected["stdout"].as_str().unwrap(), &actual.stdout));
    diffs.extend(compare_stream("stderr", stderr_mode, expected["stderr"].as_str().unwrap(), &actual.stderr));

    let expected_files: BTreeMap<String, String> = expected["files"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string()))
        .collect();
    for (path, content) in &expected_files {
        match actual.files.get(path) {
            None => diffs.push(format!("file {path}: missing")),
            Some(got) => {
                let mode = if path.ends_with(".json") || path.ends_with(".log") { "json" } else { "text" };
                diffs.extend(compare_stream(&format!("file {path}"), mode, content, got));
            }
        }
    }
    for path in actual.files.keys() {
        if !expected_files.contains_key(path) {
            diffs.push(format!("file {path}: unexpected"));
        }
    }
    diffs
}

#[test]
fn golden_cases_match_python() {
    let golden = golden_dir();
    let spec: Value = serde_json::from_str(&fs::read_to_string(golden.join("cases.json")).unwrap()).unwrap();
    let known: BTreeMap<String, String> = fs::read_to_string(golden.join("known_differences.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    let timezone = spec["timezone"].as_str().unwrap();
    let fixture = golden.join("fixtures/standard");
    let filter = std::env::var("GOLDEN_FILTER").ok();

    let mut failures = Vec::new();
    let mut accepted = Vec::new();
    let mut ran = 0;
    for case in spec["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        if filter.as_deref().is_some_and(|f| !name.contains(f)) {
            continue;
        }
        ran += 1;
        let expected: Value =
            serde_json::from_str(&fs::read_to_string(golden.join("expected").join(format!("{name}.json"))).unwrap())
                .unwrap();
        let actual = run_case(case, timezone, &fixture);
        let diffs = compare(case, &expected, &actual);
        if diffs.is_empty() {
            continue;
        }
        let report = format!("{name}:\n    {}", diffs.join("\n    "));
        if let Some(reason) = known.get(name) {
            accepted.push(format!("{report}\n    (accepted: {reason})"));
        } else {
            failures.push(report);
        }
    }
    if !accepted.is_empty() {
        eprintln!("{} accepted difference(s):\n{}", accepted.len(), accepted.join("\n"));
    }
    assert!(
        failures.is_empty(),
        "{} of {ran} golden case(s) differ from Python:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
    eprintln!("{ran} golden case(s) match the Python implementation");
}
