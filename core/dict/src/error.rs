//! エラー型。

use thiserror::Error;

/// 辞書の構築・読み込みで起きるエラー
#[derive(Debug, Error)]
pub enum DictError {
    /// 入出力エラー
    #[error("入出力エラー: {0}")]
    Io(#[from] std::io::Error),
    /// Mozc 形式のテキストの不正（ファイル名と行番号付き）
    #[error("{file}:{line}: {message}")]
    Parse {
        /// 対象ファイル（または識別名）
        file: String,
        /// 1 始まりの行番号
        line: usize,
        /// 内容
        message: String,
    },
    /// バイナリ辞書の不正（マジック不一致・途中切れなど）
    #[error("辞書バイナリが不正です: {0}")]
    Format(String),
    /// 構築時の不整合
    #[error("辞書の構築に失敗: {0}")]
    Build(String),
}

impl DictError {
    pub(crate) fn parse(file: &str, line: usize, message: impl Into<String>) -> Self {
        DictError::Parse {
            file: file.to_string(),
            line,
            message: message.into(),
        }
    }
}
