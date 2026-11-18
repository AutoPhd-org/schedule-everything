//! Schedule Everything and LaunchX's shared memory, end to end: a scratch
//! LaunchX runs `rmd serve`, the user switches rmd's memory access, and rmd
//! keeps its section of the `schedule` page current without losing what
//! another writer added to the page.
//!
//! It needs LaunchX's `lx` (with shared memory), named by `RMD_TEST_LX`, and
//! is skipped without it:
//!
//! ```sh
//! RMD_TEST_LX=../LaunchX/target/debug/lx cargo test -p rmd --test launchx_memory
//! ```
//!
//! The launcher gets a home, a gateway port and a `HOME` of its own in a
//! temporary directory, and is stopped through its API (`POST
//! /api/shutdown`), never with `lx down`, which would unregister the user's
//! own launcher from their login.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

const RMD: &str = env!("CARGO_BIN_EXE_rmd");

struct Scratch {
    root: tempfile::TempDir,
    home: PathBuf,
    port: u16,
    lx: PathBuf,
    daemon: Option<Child>,
}

fn free_port() -> u16 {
    std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap().local_addr().unwrap().port()
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .proxy(None)
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(10)))
        .build()
        .into()
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn wait_for<T>(what: &str, timeout: Duration, mut check: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(found) = check() {
            return found;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(200));
    }
}

impl Scratch {
    fn start(lx: PathBuf) -> Self {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("launchx");
        std::fs::create_dir_all(&home).unwrap();
        let port = free_port();
        let skills = root.path().join("skills");
        std::fs::write(
            home.join("config.toml"),
            format!("gateway_port = {port}\nagent_skill_dirs = [{:?}]\n", skills.display().to_string()),
        )
        .unwrap();
        let mut scratch = Self { root, home, port, lx, daemon: None };
        let log = std::fs::File::create(scratch.root.path().join("daemon.log")).unwrap();
        let daemon = scratch
            .command()
            .arg("daemon")
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .expect("run lx daemon");
        scratch.daemon = Some(daemon);
        wait_for("the launcher", Duration::from_secs(15), || scratch.run_file("launchx"));
        scratch
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.lx);
        command
            .env("LAUNCHX_HOME", &self.home)
            .env("LAUNCHX_INHERIT_ENV", "1")
            .env("HOME", self.root.path())
            .env_remove("LAUNCHX")
            .env_remove("LAUNCHX_MEMORY_URL")
            .env_remove("LAUNCHX_MEMORY_TOKEN");
        command
    }

    fn lx(&self, args: &[&str], input: Option<&str>) -> Output {
        let mut child = self
            .command()
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        if let Some(input) = input {
            use std::io::Write;
            let mut stdin = stdin;
            stdin.write_all(input.as_bytes()).unwrap();
        }
        child.wait_with_output().unwrap()
    }

    fn lx_ok(&self, args: &[&str], input: Option<&str>) -> String {
        let output = self.lx(args, input);
        let text = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        assert!(output.status.success(), "lx {args:?} failed:\n{text}");
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    fn run_file(&self, id: &str) -> Option<Value> {
        let text = std::fs::read_to_string(self.home.join("run").join(format!("{id}.json"))).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// A page of memory as `lx memory show --json` gives it, if it exists.
    fn page(&self, page: &str) -> Option<Value> {
        let output = self.lx(&["memory", "show", page, "--json"], None);
        output.status.success().then(|| serde_json::from_slice(&output.stdout).unwrap())
    }

    /// rmd's direct address and token, from its run file.
    fn rmd(&self) -> (String, String) {
        let run = self.run_file("rmd").expect("rmd runs");
        (run["url"].as_str().unwrap().trim_end_matches('/').to_string(), run["token"].as_str().unwrap().to_string())
    }

    fn rmd_state(&self) -> Value {
        let (url, token) = self.rmd();
        let response =
            agent().get(&format!("{url}/api/state")).header("Authorization", &format!("Bearer {token}")).call();
        let body = response.unwrap().into_body().read_to_string().unwrap();
        serde_json::from_str(&body).unwrap()
    }

    fn rmd_command(&self, command: &str, payload: Value) -> Value {
        let (url, token) = self.rmd();
        let request = json!({"command": command, "payload": payload}).to_string();
        let response = agent()
            .post(&format!("{url}/api"))
            .header("Authorization", &format!("Bearer {token}"))
            .header("Content-Type", "application/json")
            .send(request);
        serde_json::from_str(&response.unwrap().into_body().read_to_string().unwrap()).unwrap()
    }

    fn access_shown(&self, access: &str) {
        wait_for(&format!("rmd's state to show memory access {access}"), Duration::from_secs(20), || {
            (self.rmd_state()["memory"]["access"] == json!(access)).then_some(())
        });
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let Some(mut daemon) = self.daemon.take() else { return };
        if let Some(token) = self.run_file("launchx").and_then(|run| run["token"].as_str().map(str::to_string)) {
            let _ = agent()
                .post(&format!("http://127.0.0.1:{}/api/shutdown", self.port))
                .header("Authorization", &format!("Bearer {token}"))
                .send("{}");
        }
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            if let Ok(Some(_)) = daemon.try_wait() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = daemon.kill();
        let _ = daemon.wait();
    }
}

#[test]
fn rmd_keeps_its_part_of_the_schedule_page_current() {
    let Some(lx) = std::env::var_os("RMD_TEST_LX").map(PathBuf::from) else {
        eprintln!("skipped: set RMD_TEST_LX to LaunchX's lx to run this test");
        return;
    };
    let scratch = Scratch::start(lx);
    let root = scratch.root.path();

    // Schedule Everything's own files: the test fixture, a deadline, a frozen clock.
    let config = root.join("rmd");
    copy_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/config"), &config);
    std::fs::write(
        config.join("user_config_0/ddl.json"),
        r#"[{"event": "Paper submission", "deadline": "2026-12-01", "type": "3"}]"#,
    )
    .unwrap();
    let project = root.join("project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("launchx.toml"),
        format!(
            r#"launchx = "0.1"

[service.rmd]
name  = "Schedule Everything"
level = 2
run   = [{rmd:?}, "serve"]
cli   = {rmd:?}
env   = {{ REMINDER_CONFIG_DIR = {config:?}, RMD_NOW = "2026-11-17T10:00:00" }}

[service.rmd.http]
health = "/api/health"
state  = "/api/state"

[service.rmd.memory]
wants  = "write"
reason = "Keeps your schedule page current"
"#,
            rmd = RMD,
            config = config.display().to_string(),
        ),
    )
    .unwrap();
    scratch.lx_ok(&["add", project.to_str().unwrap()], None);
    scratch.lx_ok(&["start", "rmd"], None);

    // Off until the user says otherwise, and rmd says so.
    scratch.access_shown("none");
    let state = scratch.rmd_state();
    assert_eq!(state["memory"]["available"], json!(true));
    assert_eq!(state["memory"]["change"], json!("lx memory access rmd write"));
    assert!(scratch.page("schedule").is_none());

    // Allowed to write: rmd's section appears on its own, within seconds.
    scratch.lx_ok(&["memory", "access", "rmd", "write"], None);
    scratch.access_shown("write");
    let page = wait_for("the schedule page", Duration::from_secs(20), || scratch.page("schedule"));
    let body = page["body"].as_str().unwrap();
    assert_eq!(page["updated_by"], json!("rmd"));
    assert!(body.starts_with("<!-- rmd:begin"), "{body}");
    assert!(body.contains("- Sunday: no reminders (a skip day)"), "{body}");
    assert!(body.contains("- 2026-12-01 (Tue): Paper submission · coding"), "{body}");
    assert!(!page["summary"].as_str().unwrap().is_empty());

    // Another writer adds a line of their own above it.
    let theirs = "- Tuesdays 11:00: 1:1 with [[people/ann]]";
    scratch.lx_ok(
        &["memory", "write", "schedule", "--base", page["rev"].as_str().unwrap(), "--by", "tester"],
        Some(&format!("{theirs}\n\n{body}")),
    );

    // A new deadline in the workspace: rmd rewrites only its own lines.
    let added = scratch.rmd_command("deadline_add", json!({"event": "Thesis defense", "date": "2027-01-15"}));
    assert_eq!(added["ok"], json!(true), "{added}");
    let page = wait_for("the new deadline on the schedule page", Duration::from_secs(30), || {
        scratch.page("schedule").filter(|page| page["body"].as_str().unwrap().contains("Thesis defense"))
    });
    let body = page["body"].as_str().unwrap();
    assert!(body.starts_with(&format!("{theirs}\n\n<!-- rmd:begin")), "{body}");
    assert_eq!(body.matches("<!-- rmd:begin").count(), 1, "{body}");
    assert!(body.contains("- 2027-01-15 (Fri): Thesis defense"), "{body}");
    assert_eq!(page["updated_by"], json!("rmd"));
    let state = scratch.rmd_state();
    assert_eq!(state["memory"]["schedule"]["rev"], page["rev"], "{}", state["memory"]);

    // Back to reading only: the switch applies at once, without a restart.
    scratch.lx_ok(&["memory", "access", "rmd", "read"], None);
    scratch.access_shown("read");

    // The launcher keeps rmd's output: it says what happened, never the token.
    let saved: Value =
        serde_json::from_str(&std::fs::read_to_string(scratch.home.join("state.json")).unwrap()).unwrap();
    let token = saved["services"]["rmd"]["memory_token"].as_str().expect("rmd's memory token");
    let logs = scratch.lx_ok(&["logs", "rmd", "-n", "200"], None);
    assert!(logs.contains("LaunchX memory: updated the schedule page"), "{logs}");
    assert!(!logs.contains(token), "{logs}");
}
