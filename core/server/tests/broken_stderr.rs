//! stderr が壊れていても imserver が起動し、`initialize` に応答することの確認。
//!
//! GUI アプリ（IME のホスト）が子プロセスの stderr に不正なハンドルを渡す状況の再現として、
//! Linux では書き込みが常に失敗する `/dev/full` を stderr にする（`eprintln!` なら panic して終了コード 101）。

#![cfg(target_os = "linux")]

use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

#[test]
fn initialize_succeeds_when_stderr_writes_fail() {
    let Ok(full) = OpenOptions::new().write(true).open("/dev/full") else {
        eprintln!("skip: /dev/full を開けない");
        return;
    };
    let dir = std::env::temp_dir().join(format!("nagi-broken-stderr-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_imserver"))
        .args(["--no-llm", "--data-dir"])
        .arg(&dir)
        .arg("--user-dir")
        .arg(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(full)
        .spawn()
        .unwrap();

    let mut stdin = child.stdin.take().unwrap();
    writeln!(
        stdin,
        r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocol_version":1,"client_name":"t","client_version":"0","os":"test"}}}}"#
    )
    .unwrap();
    stdin.flush().unwrap();

    // 応答は別スレッドで読み、固まった場合はテストを失敗させる
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let _ = BufReader::new(stdout).read_line(&mut line);
        let _ = tx.send(line);
    });
    let line = rx.recv_timeout(Duration::from_secs(20));
    // 生きているか（panic で終了していれば try_wait が Some）
    let exited = child.try_wait().unwrap();
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&dir);

    let line = line.expect("initialize の応答が来ない");
    assert!(line.contains("\"protocol_version\""), "応答: {line:?}");
    assert!(exited.is_none(), "imserver が終了した: {exited:?}");
}
