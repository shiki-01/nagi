//! `cargo xtask build-dict` — Mozc / SudachiDict 辞書から Nagi バイナリ辞書を生成。

use std::fs;
use std::path::PathBuf;
use std::time::Instant;

/// `build-dict` で選択できる辞書ソース。
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum DictSource {
    Mozc,
    Sudachi,
}

/// リポジトリルートを求める。
fn find_repo_root() -> Result<PathBuf, String> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .map_err(|e| format!("CARGO_MANIFEST_DIR が未設定: {e}"))?;
    let path = PathBuf::from(manifest_dir)
        .parent()
        .ok_or("Cargo.toml の親ディレクトリが取得できない")?
        .parent()
        .ok_or("core/ の親ディレクトリが取得できない")?
        .to_path_buf();
    Ok(path)
}

/// Mozc 辞書のディレクトリを取得。
fn mozc_dict_dir() -> Result<PathBuf, String> {
    let repo_root = find_repo_root()?;
    let dir = repo_root.join("data").join("dict").join("mozc");
    if !dir.exists() {
        return Err(format!(
            "Mozc 辞書ディレクトリが見つかりません: {}。\n\
            先に `cargo xtask fetch-dict` を実行してください。",
            dir.display()
        ));
    }
    Ok(dir)
}

/// SudachiDict 辞書のディレクトリを取得。
fn sudachi_dict_dir() -> Result<PathBuf, String> {
    let repo_root = find_repo_root()?;
    let dir = repo_root.join("data").join("dict").join("sudachi");
    if !dir.exists() {
        return Err(format!(
            "SudachiDict 辞書ディレクトリが見つかりません: {}。\n\
            {} に SudachiDict の語彙 CSV（*.csv）と matrix.def を配置してください。",
            dir.display(),
            dir.display()
        ));
    }
    Ok(dir)
}

/// 出力ファイルのパスを取得。
fn output_path(source: DictSource) -> Result<PathBuf, String> {
    let repo_root = find_repo_root()?;
    let filename = match source {
        DictSource::Mozc => "nagi-dict.bin",
        DictSource::Sudachi => "nagi-dict-sudachi.bin",
    };
    Ok(repo_root.join("data").join("dict").join(filename))
}

/// SudachiDict 辞書ディレクトリ内の語彙 CSV（ファイル名順）と matrix.def のパスを求める。
fn sudachi_inputs(dir: &std::path::Path) -> Result<(Vec<PathBuf>, PathBuf), String> {
    let mut lex_paths: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|e| format!("ディレクトリ読み込み失敗 {}: {}", dir.display(), e))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("csv"))
        })
        .collect();
    lex_paths.sort();
    if lex_paths.is_empty() {
        return Err(format!("{} に *.csv がありません", dir.display()));
    }
    let matrix_def_path = dir.join("matrix.def");
    if !matrix_def_path.exists() {
        return Err(format!("{} が見つかりません", matrix_def_path.display()));
    }
    Ok((lex_paths, matrix_def_path))
}

/// タスク実行。
pub fn run(source: DictSource) -> Result<(), String> {
    let output_file = output_path(source)?;

    eprintln!("出力ファイル: {}", output_file.display());
    eprintln!();

    eprintln!("Nagi バイナリ辞書を構築中...");
    let start = Instant::now();

    let dictionary = match source {
        DictSource::Mozc => {
            let mozc_dir = mozc_dict_dir()?;
            eprintln!("Mozc 辞書ディレクトリ: {}", mozc_dir.display());
            // Mozc の連接コストは「前の語の右 ID × 次の語の左 ID」の順（RidMajor。実データの変換結果で検証済み）
            nagi_dict::build_from_mozc_dir(&mozc_dir, nagi_dict::ConnectionLayout::RidMajor)
                .map_err(|e| format!("辞書ビルド失敗: {e}"))?
        }
        DictSource::Sudachi => {
            let sudachi_dir = sudachi_dict_dir()?;
            eprintln!("SudachiDict 辞書ディレクトリ: {}", sudachi_dir.display());
            let (lex_paths, matrix_def_path) = sudachi_inputs(&sudachi_dir)?;
            for p in &lex_paths {
                eprintln!("  語彙CSV: {}", p.display());
            }
            eprintln!("  連接コスト表: {}", matrix_def_path.display());
            nagi_dict::build_from_sudachi_files(&lex_paths, &matrix_def_path)
                .map_err(|e| format!("辞書ビルド失敗: {e}"))?
        }
    };
    let binary_data = dictionary.as_bytes();

    let build_time = start.elapsed();
    let binary_size = binary_data.len();

    // バイナリを書き込む
    fs::create_dir_all(
        output_file
            .parent()
            .ok_or("出力ディレクトリが特定できない")?,
    )
    .map_err(|e| format!("ディレクトリ作成失敗: {}", e))?;
    // 辞書は mmap で参照されるため、その場で上書きせず一時ファイルに書いてからリネームする
    // （原子的置換。旧ファイルをマップ中のプロセスは旧 inode を見続ける。nagi-dict の mmap.rs 参照）
    let tmp_file = output_file.with_extension("bin.tmp");
    fs::write(&tmp_file, binary_data).map_err(|e| format!("ファイル書き込み失敗: {}", e))?;
    fs::rename(&tmp_file, &output_file).map_err(|e| format!("ファイル置換失敗: {}", e))?;

    eprintln!();
    eprintln!("完了");
    eprintln!("  構築時間: {:.2}s", build_time.as_secs_f64());
    eprintln!(
        "  バイナリサイズ: {} bytes ({:.2} MB)",
        binary_size,
        binary_size as f64 / 1_000_000.0
    );
    eprintln!("  出力先: {}", output_file.display());

    Ok(())
}
