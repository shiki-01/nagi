//! stderr への診断出力。書き込みに失敗しても panic しない。
//!
//! `eprintln!` は stderr への書き込みが失敗する（ハンドルが壊れている・パイプが閉じている・
//! ディスクが一杯など）と panic する。imserver は GUI アプリ（IME のホスト）の子プロセスとして
//! 起動されるため stderr が不正なことがあり、ログのために起動が失敗してはならない。
//! stdout は JSON-RPC 専用なのでここでは扱わない。入力内容は渡さないこと。

use std::fmt;
use std::io::Write;

/// 1 行を stderr に書く（失敗は無視する）。通常は [`elog!`](crate::elog) 経由で使う
pub fn write_line(args: fmt::Arguments<'_>) {
    write_line_to(&mut std::io::stderr().lock(), args);
}

/// 書き込み先を差し替えられる本体（テスト用）。書き込み・flush の失敗は無視する
pub fn write_line_to(out: &mut dyn Write, args: fmt::Arguments<'_>) {
    let mut line = fmt::format(args);
    line.push('\n');
    let _ = out.write_all(line.as_bytes());
}

/// `eprintln!` の代わり。stderr への書き込み失敗で panic しない
#[macro_export]
macro_rules! elog {
    ($($arg:tt)*) => {
        $crate::stderr_log::write_line(::std::format_args!($($arg)*))
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    struct Failing;
    impl Write for Failing {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("broken"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::other("broken"))
        }
    }

    #[test]
    fn write_failure_does_not_panic() {
        write_line_to(&mut Failing, format_args!("x {}", 1));
    }

    #[test]
    fn writes_one_line() {
        let mut buf = Vec::new();
        write_line_to(&mut buf, format_args!("a{}", "b"));
        assert_eq!(buf, b"ab\n");
    }
}
