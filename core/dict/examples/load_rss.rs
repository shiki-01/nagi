//! 辞書ロード前後のメモリ（/proc/self/status の VmRSS / RssAnon / RssFile）とロード時間・
//! 最初の数回の変換時間を測る計測用 example（Linux 専用）。
//!
//! 使い方: `cargo run --release -p nagi-dict [--features mmap] --example load_rss -- <辞書> [read|mmap]`

use nagi_dict::Dictionary;
use std::time::Instant;

fn rss() -> String {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let get = |key: &str| -> String {
        status
            .lines()
            .find(|l| l.starts_with(key))
            .map(|l| l[key.len()..].trim().to_string())
            .unwrap_or_else(|| "?".into())
    };
    format!(
        "VmRSS {} / RssAnon {} / RssFile {}",
        get("VmRSS:"),
        get("RssAnon:"),
        get("RssFile:")
    )
}

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("辞書パス");
    let mode = args.next().unwrap_or_else(|| "read".into());
    println!("before load : {}", rss());
    let t = Instant::now();
    let dict = match mode.as_str() {
        #[cfg(feature = "mmap")]
        "mmap" => Dictionary::load_mmap(std::path::Path::new(&path)).expect("load_mmap"),
        #[cfg(feature = "mmap")]
        "verify" => Dictionary::load_mmap(std::path::Path::new(&path)).expect("load_mmap"),
        _ => Dictionary::load(std::path::Path::new(&path)).expect("load"),
    };
    println!("load ({mode}) : {:?}", t.elapsed());
    println!("after load  : {}", rss());
    if mode == "verify" {
        let t = Instant::now();
        dict.verify().expect("verify");
        println!("verify : {:?}", t.elapsed());
    }
    for text in [
        "きょうはいいてんきですね",
        "わたしのなまえはなかのです",
        "あしたのかいぎのしりょうをじゅんびしないと",
        "にほんごにゅうりょくのへんかんせいどをむじょうけんにあげる",
        "さいきんのでぃーぷらーにんぐのはってんはめざましい",
    ] {
        let t = Instant::now();
        let r = dict.convert(text);
        println!("convert {:?} : {:?}", r.len(), t.elapsed());
    }
    println!("after conv  : {}", rss());
    // 定常状態のレイテンシ（5 文 × 200 回）
    let sentences = [
        "きょうはいいてんきですね",
        "わたしのなまえはなかのです",
        "あしたのかいぎのしりょうをじゅんびしないと",
        "にほんごにゅうりょくのへんかんせいどをむじょうけんにあげる",
        "さいきんのでぃーぷらーにんぐのはってんはめざましい",
    ];
    let mut times = Vec::new();
    for _ in 0..200 {
        for text in sentences {
            let t = Instant::now();
            std::hint::black_box(dict.convert(text));
            times.push(t.elapsed());
        }
    }
    times.sort();
    println!(
        "steady convert: p50={:?} p95={:?} max={:?}",
        times[times.len() / 2],
        times[times.len() * 95 / 100],
        times[times.len() - 1]
    );
    println!("after steady: {}", rss());
}
