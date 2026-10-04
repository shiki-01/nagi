//! 辞書ファイルの mmap（feature `mmap`）。この crate で唯一 `unsafe` を使う場所。

use memmap2::Mmap;
use std::fs::File;
use std::io;
use std::path::Path;

/// 読み取り専用で `path` を mmap する。
///
/// # 安全性の根拠
///
/// `Mmap::map` が unsafe なのは、マップ中にファイルが他のプロセス・スレッドから変更される
/// （特に切り詰められる）と、マップ領域へのアクセスが未定義動作（実際には SIGBUS）になり得るため。
/// 次の前提でこれを許容する。
///
/// - マップは読み取り専用（`map` のみ。`map_mut` / `map_copy` は使わない）で、この crate は
///   マップ領域を書き換えない。アクセスはすべて境界チェック付きの安全なスライス操作
/// - 辞書ファイルは**更新時に別名で作ってからリネーム（原子的置換）する**運用とする
///   （`cargo xtask build-dict` は一時ファイルに書いてから rename する）。リネームで置き換えられても
///   既存のマップは旧 inode を指し続けるので、マップ中の内容は変わらない。
///   その場で上書き・切り詰めする運用（`fs::write` で直接上書き等）はこの前提に反し、
///   マップ中の `Dictionary` を持つプロセスが SIGBUS で落ち得る
/// - mmap に失敗した場合は呼び出し側（`Dictionary::load_mmap`）がエラーを返し、サーバー / ffi は
///   `Dictionary::load`（`fs::read`）にフォールバックする
pub(crate) fn map_readonly(path: &Path) -> io::Result<Mmap> {
    let file = File::open(path)?;
    // SAFETY: 上記の前提（読み取り専用・非書き換え・原子的置換運用）による。
    unsafe { Mmap::map(&file) }
}
