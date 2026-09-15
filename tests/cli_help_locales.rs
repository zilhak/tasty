//! Run the shipped CLI entry point, including clap exits, without a live instance.
use std::{
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    path::Path,
    process::{Command, Output},
};

fn cli(home: &Path, args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_tasty"));
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("TASTY_") {
            cmd.env_remove(key);
        }
    }
    cmd.env("TASTY_HOME", home)
        .env("NO_COLOR", "1")
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn help_and_parse_errors_follow_locale_while_wire_messages_stay_unchanged() {
    let root = tempfile::tempdir().unwrap();
    for (locale, usage, error, details) in [
        ("en", "Usage:", "error:", "for full details"),
        ("ko", "사용법:", "오류:", "자세한 내용"),
        ("ja", "使い方:", "エラー:", "詳細は"),
    ] {
        let home = root.path().join(locale);
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(
            home.join("config.toml"),
            format!("[general]\nlanguage='{locale}'\n"),
        )
        .unwrap();
        for args in [
            &["--help"][..],
            &["new", "--help"],
            &["surface", "completion", "--help"],
        ] {
            let output = cli(&home, args);
            assert_eq!(output.status.code(), Some(0), "{locale} {args:?}");
            let out = String::from_utf8_lossy(&output.stdout);
            assert!(out.contains(usage), "{locale} {args:?}: {out}");
            assert!(!out.contains("cli.help_frame."), "{out}");
        }
        let output = cli(&home, &["-a"]);
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("completion"));
        for args in [
            &["new", "--unknown-option"][..],
            &["surface", "completion"],
            &["surface", "completion", "--surface", "not-a-number"],
        ] {
            let output = cli(&home, args);
            assert_eq!(output.status.code(), Some(2));
            let err = String::from_utf8_lossy(&output.stderr);
            assert!(err.contains(error), "{locale} {args:?}: {err}");
            assert!(err.contains(details), "{locale} {args:?}: {err}");
        }
        // A fixture server speaks the existing wire format. Only the CLI presentation runs.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let port_file = home.join("fixture.port");
        std::fs::write(&port_file, port.to_string()).unwrap();
        let server = std::thread::spawn(move || {
            listener.set_nonblocking(true).unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            && std::time::Instant::now() < deadline =>
                    {
                        std::thread::sleep(std::time::Duration::from_millis(10))
                    }
                    Err(e) => panic!("fixture accept failed: {e}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(10)))
                .unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            let request: serde_json::Value = serde_json::from_str(&line).unwrap();
            let response = serde_json::json!({"jsonrpc":"2.0", "id":request["id"], "error":{"code":-32602,"message":"fixture wire error"}});
            writeln!(stream, "{response}").unwrap();
        });
        let output = cli(
            &home,
            &[
                "--port-file",
                port_file.to_str().unwrap(),
                "surface",
                "completion",
                "--surface",
                "999",
            ],
        );
        server.join().unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&output.stderr).contains("fixture wire error"));
    }
}
