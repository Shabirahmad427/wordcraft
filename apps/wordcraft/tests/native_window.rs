//! Opt-in end-to-end smoke test for a real native app window.
//!
//! Run only where a desktop session is available:
//! `WORDCRAFT_NATIVE_WINDOW_SMOKE=1 cargo test -p wordcraft --test native_window -- isolated_native_ --ignored --nocapture --test-threads=1`.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

struct Profile(PathBuf);

impl Profile {
    fn create() -> Self {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).expect("clock is after the Unix epoch").as_nanos();
        #[cfg(target_os = "macos")]
        let temp_root = Path::new("/private/tmp");
        #[cfg(not(target_os = "macos"))]
        let temp_root = std::env::temp_dir();
        for attempt in 0..32u8 {
            let path = temp_root.join(format!("wordcraft-native-smoke-{}-{stamp}-{attempt}", std::process::id()));
            match std::fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("could not create isolated WordCraft profile {}: {error}", path.display()),
            }
        }
        panic!("could not allocate a unique isolated WordCraft profile");
    }

    fn settings(&self) -> PathBuf {
        if cfg!(target_os = "macos") {
            self.0.join("Library/Application Support/WordCraft")
        } else if cfg!(target_os = "windows") {
            self.0.join("AppData/WordCraft")
        } else {
            self.0.join("xdg/wordcraft")
        }
    }
}

impl Drop for Profile {
    fn drop(&mut self) {
        if std::env::var("WORDCRAFT_PRESERVE_NATIVE_PROFILE").as_deref() == Ok("1") {
            eprintln!("preserving isolated native smoke profile for diagnosis: {}", self.0.display());
        } else {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

struct AppProcess(Child);

impl Drop for AppProcess {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(None)) {
            eprintln!("native smoke harness cleanup: force-terminating child pid {} after test failure", self.0.id());
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

struct Control {
    port: u16,
    key: String,
    next_id: u64,
}

impl Control {
    fn call(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], self.port));
        let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2))
            .unwrap_or_else(|error| panic!("could not connect to WordCraft control channel: {error}"));
        stream.set_read_timeout(Some(Duration::from_secs(5))).expect("set control timeout");
        serde_json::to_writer(&mut stream, &json!({"id": self.next_id, "key": self.key, "method": method, "params": params}))
            .expect("serialize control request");
        stream.write_all(b"\n").expect("send control request");
        let mut response = String::new();
        BufReader::new(stream).read_line(&mut response).expect("read control response");
        let response: Value = serde_json::from_str(&response).expect("parse control response");
        assert_eq!(response.get("ok").and_then(Value::as_bool), Some(true), "{method} failed: {response}");
        response.get("result").cloned().unwrap_or(Value::Null)
    }

    fn execute(&mut self, command: &str, params: Value) -> Value {
        self.call("engine.execute", json!({"command": command, "params": params}))
    }
}

fn free_port() -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("reserve an ephemeral loopback port");
    listener.local_addr().expect("read ephemeral port").port()
}

fn spawn_native_app(profile: &Profile, port: u16, executable: &str, stderr_path: &Path, no_prefs: bool) -> Child {
    #[cfg(target_os = "macos")]
    {
        spawn_macos_app_bundle(profile, port, executable, stderr_path, no_prefs)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let stderr = std::fs::File::create(stderr_path).expect("create isolated app stderr log");
        let mut command = Command::new(executable);
        command
            .arg("--control")
            .arg(port.to_string())
            .current_dir(&profile.0)
            .stdout(Stdio::null())
            .stderr(Stdio::from(stderr))
            .env("HOME", &profile.0)
            .env("XDG_CONFIG_HOME", profile.0.join("xdg"))
            .env("APPDATA", profile.0.join("AppData"));
        if no_prefs {
            command.env("WORDCRAFT_NO_PREFS", "1");
        } else {
            command.env_remove("WORDCRAFT_NO_PREFS");
        }
        #[cfg(target_os = "windows")]
        command.env("USERPROFILE", &profile.0);
        command.spawn().expect("launch native WordCraft GUI in the isolated test profile")
    }
}

#[cfg(target_os = "macos")]
fn spawn_macos_app_bundle(profile: &Profile, port: u16, executable: &str, stderr_path: &Path, no_prefs: bool) -> Child {
    use std::os::unix::fs::PermissionsExt;

    // A raw executable spawned by a non-interactive test process can enter NSApplication::run
    // without receiving the initial winit resume callback. Use LaunchServices with an isolated
    // app bundle, the same launch form used by the installed macOS application.
    let bundle = profile.0.join(format!("WordCraft-Native-Smoke-{port}.app"));
    let contents = bundle.join("Contents");
    let macos = contents.join("MacOS");
    std::fs::create_dir_all(&macos).expect("create isolated macOS app bundle");
    let executable_path = Path::new(executable).to_string_lossy();
    let profile_path = profile.0.to_string_lossy();
    let stderr_display = stderr_path.to_string_lossy();
    let empty_fonts = profile.0.join("empty-fonts");
    std::fs::create_dir(&empty_fonts).expect("create isolated empty font directory");
    let empty_fonts = empty_fonts.to_string_lossy();
    let no_prefs_env = if no_prefs { "export WORDCRAFT_NO_PREFS=1\n" } else { "unset WORDCRAFT_NO_PREFS\n" };
    let launcher = format!(
        "#!/bin/sh\n{no_prefs_env}exec /usr/bin/env HOME={} CFFIXED_USER_HOME={} XDG_CONFIG_HOME={} APPDATA={} WORDCRAFT_FONT_DIRS={} {} --control {} 2>>{}\n",
        shell_quote(&profile_path),
        shell_quote(&profile_path),
        shell_quote(&format!("{profile_path}/xdg")),
        shell_quote(&format!("{profile_path}/AppData")),
        shell_quote(&empty_fonts),
        shell_quote(&executable_path),
        port,
        shell_quote(&stderr_display),
    );
    let launcher_path = macos.join("wordcraft-launch");
    std::fs::write(&launcher_path, launcher).expect("write isolated macOS app launcher");
    let mut permissions = std::fs::metadata(&launcher_path).expect("read launcher permissions").permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&launcher_path, permissions).expect("make isolated app launcher executable");
    let bundle_id = format!("org.storytold.wordcraft.native-smoke.{port}");
    let plist = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict><key>CFBundleExecutable</key><string>wordcraft-launch</string><key>CFBundleIdentifier</key><string>{bundle_id}</string><key>CFBundleName</key><string>WordCraft Native Smoke</string><key>CFBundlePackageType</key><string>APPL</string><key>CFBundleVersion</key><string>1</string></dict></plist>\n"
    );
    std::fs::write(contents.join("Info.plist"), plist).expect("write isolated macOS app metadata");
    let launch_stderr = std::fs::OpenOptions::new().create(true).append(true).open(stderr_path).expect("capture LaunchServices and app stderr");
    Command::new("/usr/bin/open")
        .arg("-n")
        .arg("-W")
        .arg(&bundle)
        .stdout(Stdio::null())
        .stderr(Stdio::from(launch_stderr))
        .spawn()
        .expect("launch isolated WordCraft app bundle through LaunchServices")
}

#[cfg(target_os = "macos")]
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn wait_for_key(child: &mut Child, key_path: &Path, stderr_path: &Path) -> String {
    let deadline = Instant::now() + Duration::from_secs(45);
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait().expect("check WordCraft startup") {
            panic!("WordCraft exited before its control channel started: {status}; stderr: {}", read_log(stderr_path));
        }
        if let Ok(key) = std::fs::read_to_string(key_path)
            && !key.trim().is_empty()
        {
            return key.trim().to_owned();
        }
        thread::sleep(Duration::from_millis(100));
    }
    panic!("WordCraft did not create its isolated control key at {}; stderr: {}", key_path.display(), read_log(stderr_path));
}

fn wait_for_document_text(control: &mut Control, expected: &str) -> Value {
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut last = Value::Null;
    while Instant::now() < deadline {
        last = control.call("document.inspect", json!({}));
        let found =
            last.get("blocks").and_then(Value::as_array).is_some_and(|blocks| {
                blocks.iter().any(|block| block.get("text").and_then(Value::as_str).is_some_and(|text| text.contains(expected)))
                    || blocks.iter().filter_map(|block| block.get("cells").and_then(Value::as_array)).flatten().any(|row| {
                        row.as_array().is_some_and(|cells| cells.iter().any(|cell| cell.as_str().is_some_and(|text| text.contains(expected))))
                    })
            });
        if found {
            return last;
        }
        thread::sleep(Duration::from_millis(100));
    }
    panic!("native UI did not insert {expected:?}; last document state: {last}");
}

fn wait_for_page(control: &mut Control) -> [f64; 4] {
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut last = Value::Null;
    while Instant::now() < deadline {
        last = control.call("ui.inspect", json!({}));
        if let Some(rect) = last.get("pages").and_then(Value::as_array).and_then(|pages| pages.first()).and_then(Value::as_array)
            && let (Some(x), Some(y), Some(w), Some(h)) = (
                rect.first().and_then(Value::as_f64),
                rect.get(1).and_then(Value::as_f64),
                rect.get(2).and_then(Value::as_f64),
                rect.get(3).and_then(Value::as_f64),
            )
        {
            return [x, y, w, h];
        }
        thread::sleep(Duration::from_millis(100));
    }
    panic!("native window did not lay out a visible page: {last}");
}

fn ui(control: &mut Control, method: &str, params: Value) -> Value {
    let result = control.call(method, params);
    // The control channel queues UI events; give the window loop a frame to consume each event.
    thread::sleep(Duration::from_millis(180));
    result
}

#[test]
#[ignore = "requires a native desktop session; opt in with WORDCRAFT_NATIVE_WINDOW_SMOKE=1"]
fn isolated_native_window_smoke() {
    assert_eq!(
        std::env::var("WORDCRAFT_NATIVE_WINDOW_SMOKE").as_deref(),
        Ok("1"),
        "set the opt-in variable only in a disposable desktop test environment"
    );
    eprintln!("native desktop smoke target: {}-{}", std::env::consts::OS, std::env::consts::ARCH);

    let profile = Profile::create();
    let port = free_port();
    let executable = env!("CARGO_BIN_EXE_wordcraft");
    let stderr_path = profile.0.join("wordcraft.stderr.log");
    // Keep preferences, recovery, keys, and logs inside this disposable profile. On macOS the
    // bundle launcher also redirects HOME and Cocoa's fixed home before starting the binary.
    let mut process = AppProcess(spawn_native_app(&profile, port, executable, &stderr_path, true));
    let key_path = profile.settings().join(format!("control-key.local-{port}"));
    let key = wait_for_key(&mut process.0, &key_path, &stderr_path);
    let mut control = Control { port, key, next_id: 0 };

    let page = wait_for_page(&mut control);
    let px = page[0] + 105.0;
    let py = page[1] + 105.0;
    ui(&mut control, "ui.click", json!({"x": px, "y": py}));
    ui(&mut control, "ui.focus", json!({}));
    ui(&mut control, "ui.text", json!({"text": "Native desktop smoke — café 東京"}));
    wait_for_document_text(&mut control, "Native desktop smoke");
    ui(&mut control, "ui.key", json!({"key": "Enter"}));
    ui(&mut control, "ui.key", json!({"key": "B", "cmd": true}));
    ui(&mut control, "ui.text", json!({"text": "bold text"}));
    ui(&mut control, "ui.key", json!({"key": "B", "cmd": true}));
    wait_for_document_text(&mut control, "bold text");

    // Exercise the real ribbon event path and the table-grid focus handoff in a native window.
    ui(&mut control, "ui.click", json!({"x": 130.0, "y": 30.0}));
    ui(&mut control, "ui.click", json!({"x": 150.0, "y": 80.0}));
    ui(&mut control, "ui.click", json!({"x": 166.0, "y": 190.0}));
    ui(&mut control, "ui.text", json!({"text": "table cell"}));
    let doc = wait_for_document_text(&mut control, "table cell");
    let table = doc
        .get("blocks")
        .and_then(Value::as_array)
        .expect("blocks")
        .iter()
        .find(|block| block.get("type").and_then(Value::as_str) == Some("table"))
        .unwrap_or_else(|| {
            let state = control.call("ui.inspect", json!({}));
            let screenshot = profile.0.join("native-window-table-ribbon-failure.png");
            let screenshot_result = control.call("ui.screenshot", json!({"path": screenshot}));
            panic!(
                "table was not inserted from the native ribbon; document state: {doc}; UI state: {state}; screenshot result: {screenshot_result}; isolated profile: {}",
                profile.0.display()
            )
        });
    assert_eq!(
        table
            .get("cells")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(Value::as_array)
            .and_then(|cells| cells.first())
            .and_then(Value::as_str),
        Some("table cell")
    );

    ui(&mut control, "ui.key", json!({"key": "Z", "cmd": true}));
    let undone = control.call("document.inspect", json!({}));
    assert!(!serde_json::to_string(&undone).expect("serialize document state").contains("table cell"), "Undo removes the last typed cell text");
    ui(&mut control, "ui.key", json!({"key": "Z", "cmd": true, "shift": true}));
    wait_for_document_text(&mut control, "table cell");

    let docx = profile.0.join("desktop-smoke.docx");
    let pdf = profile.0.join("desktop-smoke.pdf");
    let saved = control.execute("file.saveAs", json!({"path": docx}));
    assert_eq!(saved.get("saved").and_then(Value::as_bool), Some(true));
    control.execute("file.exportPdf", json!({"path": pdf}));
    let pdf_bytes = std::fs::read(&pdf).expect("read PDF export");
    assert!(pdf_bytes.starts_with(b"%PDF-"), "export is a PDF file");
    let reopened = control.execute("file.open", json!({"path": docx}));
    assert_eq!(reopened.get("path").and_then(Value::as_str), docx.to_str());
    let after_reopen = control.call("document.inspect", json!({}));
    let serialized = serde_json::to_string(&after_reopen).expect("serialize reopened document");
    for expected in ["Native desktop smoke", "bold text", "table cell"] {
        assert!(serialized.contains(expected), "reopened document lost {expected:?}: {after_reopen}");
    }
    let bold = after_reopen
        .get("blocks")
        .and_then(Value::as_array)
        .expect("reopened blocks")
        .iter()
        .find(|block| block.get("text").and_then(Value::as_str) == Some("bold text"))
        .expect("reopened bold paragraph");
    assert!(
        bold.get("runs").and_then(Value::as_array).is_some_and(|runs| runs
            .iter()
            .any(|run| { run.get("props").and_then(|props| props.get("bold")).and_then(Value::as_bool) == Some(true) })),
        "DOCX save/reopen preserves bold formatting: {bold}"
    );

    let screenshot = profile.0.join("window.png");
    control.call("ui.screenshot", json!({"path": screenshot}));
    let png = std::fs::read(screenshot).expect("read native window screenshot");
    assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"), "window screenshot is a PNG");
    assert!(png.len() > 10_000, "window screenshot contains rendered UI content");

    control.call("app.quit", json!({}));
    let status = wait_for_exit(&mut process.0, Duration::from_secs(15), &stderr_path);
    assert!(status.success(), "native app launch did not finish successfully: {status}; stderr: {}", read_log(&stderr_path));
    assert_app_exit_callback(&stderr_path);
    assert!(!key_path.exists(), "normal shutdown left its control key at {}", key_path.display());
    for path in [
        profile.settings().join("ui.json"),
        profile.settings().join("recovery"),
        profile.settings().join("logs"),
        profile.settings().join("fonts"),
        profile.settings().join("custom-dictionary.txt"),
    ] {
        assert!(!path.exists(), "WORDCRAFT_NO_PREFS=1 must not create {}", path.display());
    }
}

#[test]
#[ignore = "requires a native desktop session; opt in with WORDCRAFT_NATIVE_WINDOW_SMOKE=1"]
fn isolated_native_shutdown_checkpoints_recovery_before_exit() {
    assert_eq!(
        std::env::var("WORDCRAFT_NATIVE_WINDOW_SMOKE").as_deref(),
        Ok("1"),
        "set the opt-in variable only in a disposable desktop test environment"
    );
    eprintln!("native shutdown regression target: {}-{}", std::env::consts::OS, std::env::consts::ARCH);
    let profile = Profile::create();
    let port = free_port();
    let stderr_path = profile.0.join("wordcraft.stderr.log");
    let executable = env!("CARGO_BIN_EXE_wordcraft");
    let mut process = AppProcess(spawn_native_app(&profile, port, executable, &stderr_path, false));
    let key_path = profile.settings().join(format!("control-key.local-{port}"));
    let key = wait_for_key(&mut process.0, &key_path, &stderr_path);
    let mut control = Control { port, key, next_id: 0 };
    wait_for_page(&mut control);
    control.execute("text.insert", json!({"text": "checkpoint before native shutdown"}));
    wait_for_document_text(&mut control, "checkpoint before native shutdown");
    control.call("app.quit", json!({}));

    let status = wait_for_exit(&mut process.0, Duration::from_secs(15), &stderr_path);
    assert!(status.success(), "native app launch did not finish successfully: {status}; stderr: {}", read_log(&stderr_path));
    assert_app_exit_callback(&stderr_path);
    assert!(!key_path.exists(), "normal shutdown left its control key at {}", key_path.display());
    let store = wordcraft_engine::recovery::Store::new(profile.settings().join("recovery")).expect("open isolated recovery store");
    let entries = store.list().expect("read isolated recovery entries");
    assert!(!entries.is_empty(), "native shutdown did not preserve the dirty document in recovery");
    assert!(
        entries.iter().any(|entry| {
            store
                .load(&entry.id)
                .is_ok_and(|snapshot| snapshot.doc.plain_text(wordcraft_doc::StoryRef::Body).contains("checkpoint before native shutdown"))
        }),
        "recovery entries do not contain the latest unsaved text"
    );
}

fn read_log(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|error| format!("could not read {}: {error}", path.display()))
}

fn assert_app_exit_callback(stderr_path: &Path) {
    let log = read_log(stderr_path);
    assert!(log.contains("eframe application exit callback entered"), "WordCraft did not reach eframe's normal exit callback: {log}");
}

fn wait_for_exit(child: &mut Child, timeout: Duration, stderr_path: &Path) -> ExitStatus {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().expect("wait for native app close") {
            let process_label = if cfg!(target_os = "macos") { "LaunchServices open -W waiter" } else { "WordCraft app process" };
            eprintln!("{process_label} exit status: {status}; stderr: {}", read_log(stderr_path));
            return status;
        }
        if Instant::now() >= deadline {
            panic!("native app shutdown timed out after {timeout:?}; harness has not terminated it; stderr: {}", read_log(stderr_path));
        }
        thread::sleep(Duration::from_millis(100));
    }
}

/// Bounded startup-stage matrix. Unlike the document smoke, this reports what initialized without
/// asserting that a blocked native desktop can start in every execution environment.
#[test]
#[ignore = "requires a native desktop session; opt in with WORDCRAFT_NATIVE_STARTUP_MATRIX=1"]
fn native_startup_stage_matrix() {
    assert_eq!(
        std::env::var("WORDCRAFT_NATIVE_STARTUP_MATRIX").as_deref(),
        Ok("1"),
        "set the opt-in variable only in a disposable desktop test environment"
    );
    for (label, with_control, with_prefs, existing_home) in [
        ("normal-existing-home-no-prefs", false, false, true),
        ("normal-temp-home-no-prefs", false, false, false),
        ("normal-temp-home-with-prefs", false, true, false),
        ("control-temp-home-no-prefs", true, false, false),
        ("control-temp-home-with-prefs", true, true, false),
    ] {
        diagnose_startup_case(label, with_control, with_prefs, existing_home);
    }
}

fn diagnose_startup_case(label: &str, with_control: bool, with_prefs: bool, existing_home: bool) {
    let profile = Profile::create();
    let port = free_port();
    let stderr_path = profile.0.join("wordcraft.stderr.log");
    let stderr = std::fs::File::create(&stderr_path).expect("create isolated diagnostic stderr log");
    let empty_fonts = profile.0.join("empty-fonts");
    std::fs::create_dir(&empty_fonts).expect("create empty isolated font directory");
    let mut command = Command::new(env!("CARGO_BIN_EXE_wordcraft"));
    command.current_dir(&profile.0).stdout(Stdio::null()).stderr(Stdio::from(stderr));
    if with_control {
        command.arg("--control").arg(port.to_string());
    }
    let home = if existing_home { std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| profile.0.clone()) } else { profile.0.clone() };
    command
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", profile.0.join("xdg"))
        .env("APPDATA", profile.0.join("AppData"))
        .env("WORDCRAFT_FONT_DIRS", empty_fonts)
        .env("RUST_LOG", "info,eframe=debug,winit=debug,wgpu=info")
        .env_remove("WORDCRAFT_NATIVE_WINDOW_SMOKE")
        .env_remove("WORDCRAFT_NATIVE_STARTUP_MATRIX")
        .env("CFFIXED_USER_HOME", &profile.0);
    if with_prefs {
        command.env_remove("WORDCRAFT_NO_PREFS");
    } else {
        command.env("WORDCRAFT_NO_PREFS", "1");
    }
    #[cfg(target_os = "windows")]
    command.env("USERPROFILE", &profile.0);

    let mut child = AppProcess(command.spawn().expect("launch isolated startup diagnostic"));
    let deadline = Instant::now() + Duration::from_secs(12);
    let key_path = profile.settings().join(format!("control-key.local-{port}"));
    let mut creator = false;
    let mut first_frame = false;
    let mut endpoint = false;
    let mut exited = None;
    while Instant::now() < deadline {
        let log = read_log(&stderr_path);
        creator |= log.contains("native app creator entered");
        first_frame |= log.contains("native window entered its first application frame");
        if with_control && key_path.is_file() {
            endpoint = TcpStream::connect_timeout(&std::net::SocketAddr::from(([127, 0, 0, 1], port)), Duration::from_millis(100)).is_ok();
        }
        if let Some(status) = child.0.try_wait().expect("check diagnostic child") {
            exited = Some(status);
            break;
        }
        if first_frame && (!with_control || endpoint) {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }

    let mut clean_quit = false;
    if with_control
        && first_frame
        && endpoint
        && let Ok(key) = std::fs::read_to_string(&key_path)
    {
        let mut control = Control { port, key: key.trim().to_owned(), next_id: 0 };
        let _ = control.call("ui.inspect", json!({}));
        let _ = control.call("app.quit", json!({}));
        if let Some(status) = wait_for_exit_bounded(&mut child.0, Duration::from_secs(10)) {
            clean_quit = status.success();
            exited = Some(status);
        }
    }
    if exited.is_none() && matches!(child.0.try_wait(), Ok(None)) {
        eprintln!("startup matrix cleanup: force-terminating diagnostic child for case {label}");
        let _ = child.0.kill();
        exited = child.0.wait().ok();
    }
    let status = exited.map_or_else(|| "unknown".to_owned(), |s| s.to_string());
    eprintln!(
        "startup matrix {label}: creator={creator}, first_frame={first_frame}, control_endpoint={endpoint}, app_quit_success={clean_quit}, child_status={status}; stderr: {}",
        read_log(&stderr_path)
    );
}

fn wait_for_exit_bounded(child: &mut Child, timeout: Duration) -> Option<ExitStatus> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().ok()? {
            return Some(status);
        }
        if Instant::now() >= deadline {
            return None;
        }
        thread::sleep(Duration::from_millis(100));
    }
}
