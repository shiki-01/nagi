//! 学習ファイルの flush が、他プロセスの flock に変換経路（learner の Mutex）を巻き込まれないことの検査。

use nagi_config::Config;
use nagi_learn::{FlushLock, LearnConfig, Learner};
use nagi_profile::SystemInfo;
use nagi_server::{Runtime, Tunables};
use std::sync::Arc;
use std::time::{Duration, Instant};

const SYSTEM: SystemInfo = SystemInfo {
    total_memory_mb: 16384,
    cpu_cores: 8,
    has_gpu: false,
};

fn runtime(name: &str) -> (Arc<Runtime>, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("nagi-learn-flush-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("learning.tsv");
    let rt = Runtime::new(
        SYSTEM,
        false,
        Tunables::from_config(&Config::default()),
        Learner::new(LearnConfig::default()),
        Some(path.clone()),
    );
    (rt, path)
}

#[test]
fn flush_waiting_for_another_process_lock_does_not_block_learner_access() {
    let (rt, path) = runtime("blocked");
    rt.learn_record("こうえん", "公園", 0);
    // 他プロセスが学習ファイルのロックを保持している状態
    let held = FlushLock::acquire(&path, None).unwrap().unwrap();

    let rt2 = rt.clone();
    let flusher = std::thread::spawn(move || rt2.flush_learning(true));
    std::thread::sleep(Duration::from_millis(100));
    // flush がロック待ちの間も、変換経路（学習の参照・記録）は止まらない
    let start = Instant::now();
    let _ = rt.learn_lookup("こうえん", 0);
    rt.learn_record("こうえん", "講演", 0);
    assert!(
        start.elapsed() < Duration::from_millis(500),
        "learner の Mutex がロック待ちの間も握られている: {:?}",
        start.elapsed()
    );
    flusher.join().unwrap();
    // ロックが取れなかった flush は先送りで、データは dirty のまま残る
    assert!(!path.exists());
    drop(held);
    rt.flush_learning(true);
    let file = std::fs::read_to_string(&path).unwrap();
    assert!(file.contains("公園") && file.contains("講演"), "{file}");
}
