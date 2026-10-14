//! プロセスのメモリ上限（`ServerOptions::memory_budget_mb`）による LLM 読み込みの抑止のテスト。
//! ローダーは差し替えた偽物なので、推論ライブラリ（feature `llama`）もモデルファイルも要らない。

use nagi_infer::ConversionModel;
use nagi_server::{
    MIN_MEMORY_BUDGET_FOR_LLM_MB, ModelLoader, ServerOptions, build_with_loader, parse_args,
};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

mod common;
use common::{FakeModel, dict};

fn options(tag: &str, memory_budget_mb: Option<u64>) -> ServerOptions {
    let root: PathBuf =
        std::env::temp_dir().join(format!("nagi-memory-budget-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("data/dict")).unwrap();
    std::fs::create_dir_all(root.join("user")).unwrap();
    std::fs::write(root.join("data/dict/nagi-dict.bin"), dict().to_bytes()).unwrap();
    std::fs::write(
        root.join("user/config.toml"),
        "[update]\nauto_check = false\n",
    )
    .unwrap();
    ServerOptions {
        data_dir: Some(root.join("data")),
        user_dir: Some(root.join("user")),
        memory_budget_mb,
        ..ServerOptions::default()
    }
}

fn spy_loader(called: Arc<AtomicBool>) -> ModelLoader {
    Box::new(move || {
        called.store(true, Ordering::SeqCst);
        Ok(
            Box::new(FakeModel::new(&[("こうえんにいく", "公園に行く")]))
                as Box<dyn ConversionModel>,
        )
    })
}

#[test]
fn small_budget_skips_llm_and_runs_dictionary_only() {
    let called = Arc::new(AtomicBool::new(false));
    let built = build_with_loader(
        &options("small", Some(MIN_MEMORY_BUDGET_FOR_LLM_MB - 1)),
        Some(spy_loader(called.clone())),
    );
    assert!(
        !called.load(Ordering::SeqCst),
        "上限未満でモデルを読み込んだ"
    );
    let notes = built.notes.join("\n");
    assert!(notes.contains("メモリ上限"), "{notes}");
    assert_eq!(built.runtime.profile().as_str(), "minimal");
}

#[test]
fn enough_budget_or_no_budget_loads_llm() {
    for (tag, budget) in [
        ("exact", Some(MIN_MEMORY_BUDGET_FOR_LLM_MB)),
        ("large", Some(3072)),
        ("none", None),
    ] {
        let called = Arc::new(AtomicBool::new(false));
        let built = build_with_loader(&options(tag, budget), Some(spy_loader(called.clone())));
        assert!(
            called.load(Ordering::SeqCst),
            "{tag}: モデルを読み込まなかった"
        );
        assert!(!built.notes.join("\n").contains("メモリ上限"));
    }
}

#[test]
fn memory_budget_flag_is_parsed() {
    let args = |v: &[&str]| {
        v.iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
            .into_iter()
    };
    assert_eq!(
        parse_args(args(&["--memory-budget-mb", "177"]))
            .unwrap()
            .memory_budget_mb,
        Some(177)
    );
    assert_eq!(parse_args(args(&[])).unwrap().memory_budget_mb, None);
    assert!(parse_args(args(&["--memory-budget-mb", "abc"])).is_err());
    assert!(parse_args(args(&["--memory-budget-mb"])).is_err());
}
