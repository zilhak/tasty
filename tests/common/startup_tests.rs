//! Exercise the real common startup/IPC path with an owned fake executable.
use super::common;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const MODE: &str = "TASTY_STARTUP_DIAGNOSTIC_TEST_MODE";
const PORT_FILE: &str = "TASTY_STARTUP_DIAGNOSTIC_TEST_PORT_FILE";

#[test]
fn fake_child() {
    let Ok(mode) = std::env::var(MODE) else {
        return;
    };
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();
    let port_file = std::env::var_os(PORT_FILE).expect("owned port path");
    let port_delay = matches!(mode.as_str(), "port_delay" | "port_delay_parent_late");
    if port_delay {
        // Publication is held until the parent has observed the closed gate.
        std::io::stdout().write_all(b"\nPORT_HELD\n").unwrap();
        std::io::stdout().flush().unwrap();
        let mut release = [0];
        std::io::stdin().read_exact(&mut release).unwrap();
        assert_eq!(release, [b'!']);
    }
    if mode == "early_exit" {
        for i in 0..400 {
            tracing::info!("noise-{i}");
        }
        std::process::exit(23);
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    std::fs::write(port_file, listener.local_addr().unwrap().port().to_string()).unwrap();
    if port_delay {
        std::io::stdout().write_all(b"\nPORT_PUBLISHED\n").unwrap();
        std::io::stdout().flush().unwrap();
    }
    let mut first = true;
    for incoming in listener.incoming() {
        let mut stream = incoming.unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut line = String::new();
        BufReader::new(&stream).read_line(&mut line).unwrap();
        let request: serde_json::Value = serde_json::from_str(&line).unwrap();
        if first && matches!(mode.as_str(), "response_delay" | "response_failure") {
            std::thread::sleep(Duration::from_millis(200));
        }
        for i in 0..400 {
            tracing::info!("noise-{i}");
        }
        if first && mode == "response_failure" {
            writeln!(stream, "invalid-json").unwrap();
            first = false;
            continue;
        }
        let method = request["method"].as_str().unwrap();
        let response = if method == "surface.screen_text" && mode == "shell_failure" {
            serde_json::json!({"jsonrpc":"2.0", "id":1, "error":{"code":-1}})
        } else {
            let result = if method == "surface.list" {
                serde_json::json!([{"id":1}])
            } else {
                serde_json::json!({"text":"prompt"})
            };
            serde_json::json!({"jsonrpc":"2.0", "id":1, "result":result})
        };
        writeln!(stream, "{response}").unwrap();
        first = false;
        if method == "system.shutdown" {
            break;
        }
    }
}

#[test]
fn scenario_driver() {
    let Ok(mode) = std::env::var(MODE) else {
        return;
    };
    let home = tempfile::tempdir().unwrap();
    let port_file = home.path().join("owned.port");
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "startup_tests::fake_child", "--nocapture"])
        .env(MODE, &mode)
        .env(PORT_FILE, &port_file)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let started = Instant::now();
    let child = common::spawn_diag::spawn_child(command).unwrap();
    let mut child = common::spawn_diag::ChildReaper::new(child);
    let stdout = child.child().stdout.take().unwrap();
    let (send, events) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let line = line.unwrap();
            if send.send(line).is_err() {
                break; // The driver failed; its reaper owns child cleanup.
            }
        }
    });
    if matches!(mode.as_str(), "port_delay" | "port_delay_parent_late") {
        if mode == "port_delay_parent_late" {
            // Model the review's delayed parent; elapsed time is never a verdict.
            std::thread::sleep(Duration::from_millis(350));
        }
        wait_for_event(&events, "PORT_HELD");
        assert!(!port_file.exists(), "port published before release");
        child.child().stdin.take().unwrap().write_all(b"!").unwrap();
        if mode == "port_delay_parent_late" {
            // Force publication before finish_startup records the parent's origin.
            wait_for_event(&events, "PORT_PUBLISHED");
            assert!(
                port_file.exists(),
                "publication acknowledgement without port"
            );
        }
    }
    let instance = common::TastyInstance::finish_startup(
        child.release(),
        port_file,
        home.path().join("unused-home"),
        started,
    );
    let first = instance.startup_diagnostics();
    instance.first_surface_id();
    let second = instance.startup_diagnostics();
    assert_eq!(
        field(&first, "first_ipc_response_ms"),
        field(&second, "first_ipc_response_ms")
    );
    tracing::info!("OBSERVED {second}");
    drop(instance);
    reader.join().unwrap();
}

fn wait_for_event(events: &std::sync::mpsc::Receiver<String>, expected: &str) {
    loop {
        let line = events
            .recv_timeout(common::spawn_diag::SPAWN_PORT_TIMEOUT)
            .expect("fake child publication handshake");
        if line == expected {
            return;
        }
    }
}

fn field<'a>(line: &'a str, key: &str) -> &'a str {
    line.split_whitespace()
        .find_map(|p| p.strip_prefix(&format!("{key}=")))
        .unwrap()
}

#[test]
fn startup_stages_survive_noise_and_distinguish_failures() {
    common::spawn_diag::init_test_tracing();
    for mode in [
        "port_delay",
        "port_delay_parent_late",
        "response_delay",
        "early_exit",
        "response_failure",
        "shell_failure",
    ] {
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "startup_tests::scenario_driver", "--nocapture"])
            .env(MODE, mode)
            .output()
            .unwrap();
        let text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        tracing::info!("SCENARIO {mode} exit={}\n{text}", output.status);
        if matches!(
            mode,
            "port_delay" | "port_delay_parent_late" | "response_delay"
        ) {
            assert!(output.status.success(), "{text}");
            let line = text
                .lines()
                .find_map(|l| l.find("OBSERVED startup ").map(|i| &l[i..]))
                .unwrap();
            assert_eq!(field(line, "pending"), "none");
            let values: Vec<f64> = [
                "spawn_returned_ms",
                "port_found_ms",
                "first_ipc_response_ms",
                "shell_ready_ms",
            ]
            .iter()
            .map(|key| field(line, key).parse().unwrap())
            .collect();
            assert!(values.windows(2).all(|v| v[0] <= v[1]));
            if mode == "response_delay" {
                assert!(
                    values[2] - values[1] >= 150.0,
                    "injected response delay missing: {values:?}"
                );
            }
        } else {
            assert!(!output.status.success());
            let pending = match mode {
                "early_exit" => "port",
                "response_failure" => "first_ipc_response",
                _ => "shell_ready",
            };
            let line = text
                .lines()
                .rev()
                .find_map(|l| l.find("startup failure: startup ").map(|i| &l[i..]))
                .expect("retained failure snapshot");
            assert_eq!(field(line, "pending"), pending);
            assert_ne!(field(line, "spawn_returned_ms"), "pending");
            assert!(
                mode != "early_exit" || text.contains("noise-399"),
                "stderr tail missing: {text}"
            );
            assert!(
                !text.contains("noise-0\n"),
                "expected early stderr to be evicted"
            );
        }
    }
}
