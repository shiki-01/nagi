//! 辞書バイナリを保持するバッファ。ファイルの mmap とヒープ上のバイト列を同じ API で扱う。
//!
//! このクレートで `unsafe` を使うのはここ（`Mmap::map` の 1 箇所）だけ。

use std::path::Path;

/// 辞書バイナリのバッファ。`AsRef<[u8]>` でバイト列として参照できる
pub enum DictBuffer {
    /// ヒープ上のバイト列
    Owned(Vec<u8>),
    /// メモリマップされたファイル（ページ境界に揃う。読まれたページだけがメモリに載る）
    Mapped(memmap2::Mmap),
}

impl DictBuffer {
    /// ファイルをメモリマップする。マップに失敗した場合はヒープに読み込む
    pub fn map_file(path: &Path) -> std::io::Result<DictBuffer> {
        let file = std::fs::File::open(path)?;
        match map(&file) {
            Ok(m) => Ok(DictBuffer::Mapped(m)),
            Err(_) => std::fs::read(path).map(DictBuffer::Owned),
        }
    }
}

impl From<Vec<u8>> for DictBuffer {
    fn from(v: Vec<u8>) -> Self {
        DictBuffer::Owned(v)
    }
}

impl AsRef<[u8]> for DictBuffer {
    fn as_ref(&self) -> &[u8] {
        match self {
            DictBuffer::Owned(v) => v,
            DictBuffer::Mapped(m) => m,
        }
    }
}

#[allow(unsafe_code)]
fn map(file: &std::fs::File) -> std::io::Result<memmap2::Mmap> {
    // SAFETY: 読み取り専用マップ。マップ中に他プロセスがファイルを切り詰めると SIGBUS になりうるが、
    // 辞書ファイルは取得・ビルド時にだけ書かれ、更新は別名書き込み + リネームで行う前提（`Dictionary::load` の doc）。
    unsafe { memmap2::Mmap::map(file) }
}
