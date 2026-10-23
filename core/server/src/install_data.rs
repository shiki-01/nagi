//! `imserver --install-data`: 辞書とモデルを利用者の手元で取得して組み立てる。
//!
//! `<data_dir>/dict/mozc/` へ Mozc OSS 辞書を取得し、`<data_dir>/dict/nagi-dict.bin` を組み立て、
//! `<data_dir>/models/` へ zenz モデルを取得し、`<data_dir>/english/words.fst`（英語の自動判定の語彙。
//! SCOWL から組み立て）を取得する。SHA-256 が一致する取得済みファイルはスキップする。
//! 進捗は標準出力に 1 行 1 JSON で出す（フロントがパースする）。人向けのログは標準エラー。
//!
//! 取得処理は feature `data-install`（`nagi-datafetch`）が必要。引数解析は feature によらず行い、
//! 無効なビルドでは [`run`] がエラーを返す。

use std::path::PathBuf;

/// 取得するモデルの選択
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelChoice {
    /// zenz-v3.1-small（既定）
    Small,
    /// zenz-v3.1-xsmall
    Xsmall,
    /// モデルを取得しない
    None,
}

/// `--install-data` の引数
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallArgs {
    /// データディレクトリ（`--data-dir` または `NAGI_DATA_DIR`）
    pub data_dir: PathBuf,
    /// 取得するモデル（`--model`。既定 small）
    pub model: ModelChoice,
}

/// 引数に `--install-data` があれば [`InstallArgs`] を返す（無ければ `None`）。
/// 受け付ける引数は `--install-data` `--data-dir` `--model small|xsmall|none` のみ。
/// `--data-dir` が無ければ環境変数 `NAGI_DATA_DIR` を使う
pub fn parse_install_args<I: Iterator<Item = String>>(
    args: I,
) -> Result<Option<InstallArgs>, String> {
    parse_install_args_with_env(args, std::env::var_os("NAGI_DATA_DIR").map(PathBuf::from))
}

/// [`parse_install_args`] の環境変数を引数で与える版（テスト用）
pub fn parse_install_args_with_env<I: Iterator<Item = String>>(
    args: I,
    env_data_dir: Option<PathBuf>,
) -> Result<Option<InstallArgs>, String> {
    let args: Vec<String> = args.collect();
    if !args.iter().any(|a| a == "--install-data") {
        return Ok(None);
    }
    let mut data_dir = env_data_dir;
    let mut model = ModelChoice::Small;
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--install-data" => {}
            "--data-dir" => {
                data_dir = Some(PathBuf::from(
                    it.next()
                        .ok_or_else(|| "--data-dir に値がありません".to_string())?,
                ));
            }
            "--model" => {
                let v = it
                    .next()
                    .ok_or_else(|| "--model に値がありません".to_string())?;
                model = match v.as_str() {
                    "small" => ModelChoice::Small,
                    "xsmall" => ModelChoice::Xsmall,
                    "none" => ModelChoice::None,
                    _ => return Err(format!("--model は small / xsmall / none のいずれか: {v}")),
                };
            }
            other => return Err(format!("--install-data と併用できない引数: {other}")),
        }
    }
    let data_dir = data_dir.ok_or_else(|| {
        "--install-data には --data-dir（または NAGI_DATA_DIR）が必要です".to_string()
    })?;
    Ok(Some(InstallArgs { data_dir, model }))
}

/// 取得・組み立てを実行する。進捗 JSON は標準出力、失敗時は `error` を出して `Err`
#[cfg(feature = "data-install")]
pub fn run(args: &InstallArgs) -> Result<(), String> {
    use nagi_datafetch::{ModelKind, Progress};
    use serde_json::json;
    use std::io::Write;

    /// バイト進捗を間引く間隔
    const BYTES_STEP: u64 = 1_000_000;

    fn emit(value: serde_json::Value) {
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{value}");
        let _ = out.flush();
    }

    let result = (|| {
        let dict_dir = args.data_dir.join("dict");
        let mozc_dir = dict_dir.join("mozc");
        let dict_path = dict_dir.join("nagi-dict.bin");

        // 1. Mozc 辞書の取得
        let mut downloaded = false;
        nagi_datafetch::fetch_mozc(&mozc_dir, &mut |p| match p {
            Progress::Skipped { file, index, count } => {
                eprintln!("imserver: スキップ: {file} (SHA-256 一致)");
                emit(
                    json!({"stage": "dict_fetch", "file": file, "index": index, "count": count, "skipped": true}),
                );
            }
            Progress::Download {
                file, index, count, ..
            } => {
                downloaded = true;
                eprintln!("imserver: ダウンロード中: {file} ({index}/{count})");
                emit(json!({"stage": "dict_fetch", "file": file, "index": index, "count": count}));
            }
            _ => {}
        })?;

        // 2. 辞書の組み立て（新しく取得した、または nagi-dict.bin が無いとき）
        let mut changed = downloaded;
        if downloaded || !dict_path.is_file() {
            changed = true;
            emit(json!({"stage": "dict_build"}));
            nagi_datafetch::build_mozc_dict(&mozc_dir, &dict_path, &mut |p| {
                if let Progress::BuildDone { bytes, seconds, .. } = p {
                    eprintln!("imserver: 辞書を組み立てました ({bytes} bytes, {seconds:.1}s)");
                }
            })?;
        } else {
            eprintln!("imserver: 辞書は最新のため組み立てをスキップ");
        }

        // 3. モデルの取得
        let kind = match args.model {
            ModelChoice::Small => Some(ModelKind::Small),
            ModelChoice::Xsmall => Some(ModelKind::Xsmall),
            ModelChoice::None => None,
        };
        if let Some(kind) = kind {
            let mut last_emitted = 0u64;
            nagi_datafetch::fetch_model(kind, &args.data_dir.join("models"), &mut |p| match p {
                Progress::Skipped { file, .. } => {
                    eprintln!("imserver: スキップ: {file} (SHA-256 一致)");
                    let total = kind.info().size;
                    emit(
                        json!({"stage": "model_fetch", "bytes": total, "total": total, "skipped": true}),
                    );
                }
                Progress::Mismatched { file } => {
                    eprintln!("imserver: {file} は SHA-256 が不一致のため再取得します");
                }
                Progress::Download { file, .. } => {
                    changed = true;
                    eprintln!("imserver: ダウンロード中: {file}");
                    last_emitted = 0;
                    emit(json!({"stage": "model_fetch", "bytes": 0, "total": kind.info().size}));
                }
                Progress::Bytes { done, total, .. } => {
                    let total = total.unwrap_or(kind.info().size);
                    if done == total || done - last_emitted >= BYTES_STEP {
                        last_emitted = done;
                        emit(json!({"stage": "model_fetch", "bytes": done, "total": total}));
                    }
                }
                _ => {}
            })?;
        }

        // 4. 英語の語彙（SCOWL → words.fst）。取得済みで索引もあれば何もしない
        let mut english_bytes_emitted = 0u64;
        let english_changed =
            nagi_datafetch::install_english(&args.data_dir.join("english"), &mut |p| match p {
                Progress::Skipped { file, .. } => {
                    eprintln!("imserver: スキップ: {file} (SHA-256 一致)");
                    emit(json!({"stage": "english_fetch", "skipped": true}));
                }
                Progress::Download { file, .. } => {
                    eprintln!("imserver: ダウンロード中: {file}");
                    english_bytes_emitted = 0;
                    emit(json!({"stage": "english_fetch", "bytes": 0}));
                }
                Progress::Bytes { done, total, .. } => {
                    if Some(done) == total || done - english_bytes_emitted >= BYTES_STEP {
                        english_bytes_emitted = done;
                        emit(json!({"stage": "english_fetch", "bytes": done, "total": total}));
                    }
                }
                Progress::BuildStart => emit(json!({"stage": "english_build"})),
                Progress::BuildDone { bytes, seconds, .. } => {
                    eprintln!(
                        "imserver: 英語の語彙を組み立てました ({bytes} bytes, {seconds:.1}s)"
                    );
                }
                _ => {}
            })?;
        changed |= english_changed;
        Ok::<bool, String>(changed)
    })();

    match result {
        Ok(changed) => {
            // changed: 取得・組み立てを 1 つでも行ったか（false ならすべて取得済みだった）
            emit(json!({"stage": "done", "changed": changed}));
            Ok(())
        }
        Err(message) => {
            emit(json!({"stage": "error", "message": message}));
            Err(message)
        }
    }
}

/// feature `data-install` が無効なビルドでは常にエラー
#[cfg(not(feature = "data-install"))]
pub fn run(_args: &InstallArgs) -> Result<(), String> {
    Err("このビルドは --install-data に対応していません（feature data-install が無効）".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str], env: Option<&str>) -> Result<Option<InstallArgs>, String> {
        parse_install_args_with_env(args.iter().map(|s| s.to_string()), env.map(PathBuf::from))
    }

    #[test]
    fn without_flag_is_none() {
        assert_eq!(parse(&["--data-dir", "d", "--no-llm"], None), Ok(None));
    }

    #[test]
    fn defaults_to_small() {
        let a = parse(&["--install-data", "--data-dir", "/x"], None)
            .unwrap()
            .unwrap();
        assert_eq!(a.data_dir, PathBuf::from("/x"));
        assert_eq!(a.model, ModelChoice::Small);
    }

    #[test]
    fn model_choices() {
        for (v, m) in [
            ("small", ModelChoice::Small),
            ("xsmall", ModelChoice::Xsmall),
            ("none", ModelChoice::None),
        ] {
            let a = parse(&["--model", v, "--install-data", "--data-dir", "d"], None)
                .unwrap()
                .unwrap();
            assert_eq!(a.model, m);
        }
        assert!(
            parse(
                &["--install-data", "--data-dir", "d", "--model", "big"],
                None
            )
            .is_err()
        );
        assert!(parse(&["--install-data", "--data-dir", "d", "--model"], None).is_err());
    }

    #[test]
    fn data_dir_falls_back_to_env() {
        let a = parse(&["--install-data"], Some("/env")).unwrap().unwrap();
        assert_eq!(a.data_dir, PathBuf::from("/env"));
        // 引数が優先
        let a = parse(&["--install-data", "--data-dir", "/arg"], Some("/env"))
            .unwrap()
            .unwrap();
        assert_eq!(a.data_dir, PathBuf::from("/arg"));
        assert!(parse(&["--install-data"], None).is_err());
    }

    #[test]
    fn rejects_unrelated_args() {
        assert!(parse(&["--install-data", "--data-dir", "d", "--no-llm"], None).is_err());
    }
}
