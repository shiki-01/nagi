//! 日付・時刻の Rewriter（`きょう` → `2026/10/27` など。H-72）。
//!
//! 現在時刻は UTC の UNIX 秒を注入された時計から取り、ローカル時刻へのずれ（分）を足して求める。
//! タイムゾーン DB は持たない（ずれの分数だけを扱う）。日付計算は自前（days-from-civil）。

use std::sync::Arc;

use crate::Rewriter;

/// [`DateTimeRewriter`] の識別子
pub const DATE_TIME_ID: &str = "date_time";

/// UTC からのずれが未報告のときに使うずれ（分）。+09:00
pub const DEFAULT_UTC_OFFSET_MINUTES: i32 = 540;

/// 現在の UNIX 秒（UTC）を返す時計
pub type UnixClock = Arc<dyn Fn() -> i64 + Send + Sync>;
/// 現在のローカル時刻の UTC からのずれ（分）を返す関数。実行中に変わりうるので呼ぶたびに引く
pub type OffsetSource = Arc<dyn Fn() -> i32 + Send + Sync>;

/// 日付・時刻の候補を出す Rewriter。読みは日付（きょう・あした…）・時刻（いま…）・年（ことし…）・
/// 月（こんげつ…）・曜日（ようび）。重複は除く
#[derive(Clone)]
pub struct DateTimeRewriter {
    now: UnixClock,
    offset_minutes: OffsetSource,
}

impl DateTimeRewriter {
    /// 時計とずれの取得元を指定して作る（テストでは固定時刻を渡す）
    pub fn new(now: UnixClock, offset_minutes: OffsetSource) -> Self {
        DateTimeRewriter {
            now,
            offset_minutes,
        }
    }

    /// システム時計（`SystemTime::now()`、UTC）を使う
    pub fn system(offset_minutes: OffsetSource) -> Self {
        let now: UnixClock =
            Arc::new(
                || match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
                    Ok(d) => d.as_secs() as i64,
                    Err(e) => -(e.duration().as_secs() as i64),
                },
            );
        Self::new(now, offset_minutes)
    }

    /// 今のローカル日時
    fn local_now(&self) -> LocalDateTime {
        let secs = (self.now)() + i64::from((self.offset_minutes)()) * 60;
        let days = secs.div_euclid(86_400);
        let rem = secs.rem_euclid(86_400);
        LocalDateTime {
            days,
            hour: (rem / 3600) as u32,
            minute: (rem % 3600 / 60) as u32,
        }
    }
}

impl Rewriter for DateTimeRewriter {
    fn id(&self) -> &str {
        DATE_TIME_ID
    }

    fn rewrite(&self, reading: &str) -> Vec<String> {
        let Some(kind) = Kind::of(reading) else {
            return Vec::new();
        };
        let now = self.local_now();
        let mut out = Vec::new();
        match kind {
            Kind::Day(delta) => date_forms(now.days + delta, &mut out),
            Kind::Time => time_forms(now.hour, now.minute, &mut out),
            Kind::Year(delta) => {
                let (year, _, _) = civil_from_days(now.days);
                year_forms(year + delta, &mut out);
            }
            Kind::Month(delta) => {
                let (year, month, _) = civil_from_days(now.days);
                let index = year * 12 + (i64::from(month) - 1) + delta;
                let (y, m) = (index.div_euclid(12), (index.rem_euclid(12) + 1) as u32);
                out.push(format!("{m}月"));
                out.push(format!("{y}年{m}月"));
            }
            Kind::Weekday => {
                let name = WEEKDAYS[weekday(now.days)];
                out.push(format!("{name}曜日"));
                out.push(name.to_string());
            }
        }
        let mut unique: Vec<String> = Vec::with_capacity(out.len());
        for s in out {
            if !unique.contains(&s) {
                unique.push(s);
            }
        }
        unique
    }
}

/// `reading` に対して [`DateTimeRewriter`] が出す形の候補か（学習から外すための判定）。
/// 日付・時刻は日が変われば古くなるので、選んで確定しても学習しない。読みが日付・時刻の読みで、
/// 表記が数字（ASCII）を含む・和暦で始まる・曜日の表記のいずれかなら真。辞書の候補（`今日` など）は偽
pub fn is_date_time_candidate(reading: &str, surface: &str) -> bool {
    if Kind::of(reading).is_none() {
        return false;
    }
    surface.bytes().any(|b| b.is_ascii_digit())
        || surface.starts_with("令和")
        || surface.starts_with("平成")
        || WEEKDAYS.contains(&surface)
        || surface
            .strip_suffix("曜日")
            .is_some_and(|d| WEEKDAYS.contains(&d))
}

struct LocalDateTime {
    /// 1970-01-01 からの日数（ローカル日付）
    days: i64,
    hour: u32,
    minute: u32,
}

/// 読みの種別
enum Kind {
    /// 今日からの日数
    Day(i64),
    Time,
    /// 今年からの年数
    Year(i64),
    /// 今月からの月数
    Month(i64),
    Weekday,
}

impl Kind {
    fn of(reading: &str) -> Option<Kind> {
        Some(match reading {
            "きょう" | "ほんじつ" => Kind::Day(0),
            "あした" | "あす" | "みょうにち" => Kind::Day(1),
            "あさって" => Kind::Day(2),
            "きのう" | "さくじつ" => Kind::Day(-1),
            "おととい" | "おとつい" => Kind::Day(-2),
            "いま" | "げんざい" | "じこく" => Kind::Time,
            "ことし" => Kind::Year(0),
            "らいねん" => Kind::Year(1),
            "きょねん" | "さくねん" => Kind::Year(-1),
            "こんげつ" => Kind::Month(0),
            "らいげつ" => Kind::Month(1),
            "せんげつ" => Kind::Month(-1),
            "ようび" => Kind::Weekday,
            _ => return None,
        })
    }
}

/// 日曜始まりの曜日名
const WEEKDAYS: [&str; 7] = ["日", "月", "火", "水", "木", "金", "土"];

/// 0 = 日曜（1970-01-01 は木曜）
fn weekday(days: i64) -> usize {
    (days + 4).rem_euclid(7) as usize
}

/// 1970-01-01 からの日数 → (年, 月, 日)（Howard Hinnant の civil_from_days）
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// 和暦の表記（`令和8年`・`令和元年`）。令和（2019-05-01〜）・平成（1989-01-08〜2019-04-30）のみ。
/// それより前は `None`
fn japanese_era(year: i64, month: u32, day: u32) -> Option<String> {
    let key = (year, month, day);
    let (name, start_year) = if key >= (2019, 5, 1) {
        ("令和", 2018)
    } else if key >= (1989, 1, 8) {
        ("平成", 1988)
    } else {
        return None;
    };
    let n = year - start_year;
    Some(if n == 1 {
        format!("{name}元年")
    } else {
        format!("{name}{n}年")
    })
}

fn date_forms(days: i64, out: &mut Vec<String>) {
    let (y, m, d) = civil_from_days(days);
    let wd = WEEKDAYS[weekday(days)];
    out.push(format!("{y}/{m:02}/{d:02}"));
    out.push(format!("{y}-{m:02}-{d:02}"));
    out.push(format!("{y}年{m}月{d}日"));
    out.push(format!("{m}月{d}日"));
    if let Some(era) = japanese_era(y, m, d) {
        out.push(format!("{era}{m}月{d}日"));
    }
    out.push(format!("{m}/{d}"));
    out.push(format!("{m}月{d}日({wd})"));
}

fn time_forms(hour: u32, minute: u32, out: &mut Vec<String>) {
    out.push(format!("{hour:02}:{minute:02}"));
    out.push(format!("{hour}時{minute}分"));
    let (ampm, h12) = if hour < 12 {
        ("午前", hour)
    } else {
        ("午後", hour - 12)
    };
    out.push(format!("{ampm}{h12}時{minute}分"));
}

fn year_forms(year: i64, out: &mut Vec<String>) {
    out.push(format!("{year}年"));
    // 年だけの和暦はその年の末日で判定する（2019 年は令和元年）
    if let Some(era) = japanese_era(year, 12, 31) {
        out.push(era);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicI32, Ordering};

    /// 1970-01-01 からの日数（Howard Hinnant の days_from_civil）
    fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
        let y = if m <= 2 { y - 1 } else { y };
        let era = y.div_euclid(400);
        let yoe = y.rem_euclid(400);
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    /// 指定した UTC 日時で固定した時計と、ずれを変えられる Rewriter
    fn at_utc(
        y: i64,
        mo: i64,
        d: i64,
        h: i64,
        mi: i64,
        offset: i32,
    ) -> (DateTimeRewriter, Arc<AtomicI32>) {
        let secs = days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60;
        let off = Arc::new(AtomicI32::new(offset));
        let off2 = off.clone();
        let r = DateTimeRewriter::new(
            Arc::new(move || secs),
            Arc::new(move || off2.load(Ordering::Relaxed)),
        );
        (r, off)
    }

    /// 2026-10-27 15:30 JST（火曜）
    fn jst() -> DateTimeRewriter {
        at_utc(2026, 10, 27, 6, 30, 540).0
    }

    #[test]
    fn civil_round_trip() {
        for days in [-800_000, -1, 0, 1, 11_016, 20_000, 60_000] {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, i64::from(m), i64::from(d)), days);
        }
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(days_from_civil(2024, 2, 29)), (2024, 2, 29));
    }

    #[test]
    fn today_forms() {
        let expected = [
            "2026/10/27",
            "2026-10-27",
            "2026年10月27日",
            "10月27日",
            "令和8年10月27日",
            "10/27",
            "10月27日(火)",
        ];
        assert_eq!(jst().rewrite("きょう"), expected);
        assert_eq!(jst().rewrite("ほんじつ"), expected);
    }

    #[test]
    fn relative_days() {
        let r = jst();
        assert_eq!(r.rewrite("あした")[0], "2026/10/28");
        assert_eq!(r.rewrite("あす")[0], "2026/10/28");
        assert_eq!(r.rewrite("みょうにち")[0], "2026/10/28");
        assert_eq!(r.rewrite("あさって")[0], "2026/10/29");
        assert_eq!(r.rewrite("きのう")[0], "2026/10/26");
        assert_eq!(r.rewrite("さくじつ")[0], "2026/10/26");
        assert_eq!(r.rewrite("おととい")[0], "2026/10/25");
        assert_eq!(r.rewrite("おとつい")[0], "2026/10/25");
        assert_eq!(r.rewrite("あした")[6], "10月28日(水)");
    }

    #[test]
    fn month_and_year_rollover() {
        // 2026-12-31 → あした = 2027-01-01（令和9年）、金曜
        let (r, _) = at_utc(2026, 12, 31, 3, 0, 540);
        let tomorrow = r.rewrite("あした");
        assert_eq!(tomorrow[0], "2027/01/01");
        assert_eq!(tomorrow[4], "令和9年1月1日");
        assert_eq!(tomorrow[6], "1月1日(金)");
        assert_eq!(r.rewrite("らいげつ"), ["1月", "2027年1月"]);
        // うるう日
        let (r, _) = at_utc(2024, 2, 28, 3, 0, 540);
        assert_eq!(r.rewrite("あした")[0], "2024/02/29");
        assert_eq!(r.rewrite("あさって")[0], "2024/03/01");
        // 1 月の先月は前年 12 月
        let (r, _) = at_utc(2026, 1, 15, 3, 0, 540);
        assert_eq!(r.rewrite("せんげつ"), ["12月", "2025年12月"]);
    }

    #[test]
    fn time_forms_cases() {
        assert_eq!(jst().rewrite("いま"), ["15:30", "15時30分", "午後3時30分"]);
        assert_eq!(jst().rewrite("げんざい"), jst().rewrite("じこく"));
        // 0 時台
        let (r, _) = at_utc(2026, 10, 26, 15, 5, 540);
        assert_eq!(r.rewrite("いま"), ["00:05", "0時5分", "午前0時5分"]);
        // 12 時台
        let (r, _) = at_utc(2026, 10, 27, 3, 0, 540);
        assert_eq!(r.rewrite("いま"), ["12:00", "12時0分", "午後0時0分"]);
        // 9 時台
        let (r, _) = at_utc(2026, 10, 27, 0, 9, 540);
        assert_eq!(r.rewrite("いま"), ["09:09", "9時9分", "午前9時9分"]);
    }

    #[test]
    fn year_month_weekday() {
        let r = jst();
        assert_eq!(r.rewrite("ことし"), ["2026年", "令和8年"]);
        assert_eq!(r.rewrite("らいねん"), ["2027年", "令和9年"]);
        assert_eq!(r.rewrite("きょねん"), ["2025年", "令和7年"]);
        assert_eq!(r.rewrite("さくねん"), ["2025年", "令和7年"]);
        assert_eq!(r.rewrite("こんげつ"), ["10月", "2026年10月"]);
        assert_eq!(r.rewrite("らいげつ"), ["11月", "2026年11月"]);
        assert_eq!(r.rewrite("せんげつ"), ["9月", "2026年9月"]);
        assert_eq!(r.rewrite("ようび"), ["火曜日", "火"]);
    }

    #[test]
    fn unrelated_readings_are_empty() {
        let r = jst();
        for s in ["", "きょ", "きょうは", "今日", "いまさら", "20261027"] {
            assert!(r.rewrite(s).is_empty(), "{s}");
        }
    }

    #[test]
    fn offset_changes_at_runtime_and_day_boundary() {
        // UTC 2026-10-27 20:00: JST では翌 28 日 5:00、UTC では 27 日 20:00、米東部 (-05:00) では 27 日 15:00
        let (r, off) = at_utc(2026, 10, 27, 20, 0, 540);
        assert_eq!(r.rewrite("きょう")[0], "2026/10/28");
        assert_eq!(r.rewrite("いま")[0], "05:00");
        off.store(0, Ordering::Relaxed);
        assert_eq!(r.rewrite("きょう")[0], "2026/10/27");
        assert_eq!(r.rewrite("いま")[0], "20:00");
        off.store(-300, Ordering::Relaxed);
        assert_eq!(r.rewrite("いま")[0], "15:00");
        off.store(-330, Ordering::Relaxed);
        assert_eq!(r.rewrite("いま")[0], "14:30");
    }

    #[test]
    fn japanese_era_boundaries() {
        assert_eq!(japanese_era(2019, 5, 1).as_deref(), Some("令和元年"));
        assert_eq!(japanese_era(2019, 4, 30).as_deref(), Some("平成31年"));
        assert_eq!(japanese_era(1989, 1, 8).as_deref(), Some("平成元年"));
        assert_eq!(japanese_era(1989, 1, 7), None);
        assert_eq!(japanese_era(1900, 1, 1), None);
        // 和暦が無い日付では和暦の候補を出さない
        let (r, _) = at_utc(1980, 6, 1, 3, 0, 540);
        let forms = r.rewrite("きょう");
        assert_eq!(forms.len(), 6);
        assert!(
            forms
                .iter()
                .all(|s| !s.contains("昭和") && !s.contains("令和"))
        );
        assert_eq!(r.rewrite("ことし"), ["1980年"]);
        // 2019 年は令和元年、2018 年は平成30年
        let (r, _) = at_utc(2019, 6, 1, 3, 0, 540);
        assert_eq!(r.rewrite("ことし"), ["2019年", "令和元年"]);
        assert_eq!(r.rewrite("きょねん"), ["2018年", "平成30年"]);
        assert_eq!(r.rewrite("きょう")[4], "令和元年6月1日");
    }

    #[test]
    fn generated_candidates_are_detected_for_learning_exclusion() {
        let r = jst();
        for reading in [
            "きょう",
            "あした",
            "いま",
            "ことし",
            "こんげつ",
            "ようび",
            "きのう",
            "らいねん",
        ] {
            for surface in r.rewrite(reading) {
                assert!(
                    is_date_time_candidate(reading, &surface),
                    "{reading} {surface}"
                );
            }
        }
        // 辞書の候補・別の読みは対象外
        assert!(!is_date_time_candidate("きょう", "今日"));
        assert!(!is_date_time_candidate("ことし", "今年"));
        assert!(!is_date_time_candidate("ようび", "曜日"));
        assert!(!is_date_time_candidate("かっこ", "「」"));
        assert!(!is_date_time_candidate("こうえん", "2026年"));
    }

    #[test]
    fn system_clock_gives_plausible_date() {
        let r = DateTimeRewriter::system(Arc::new(|| 0));
        let first = &r.rewrite("きょう")[0];
        assert!(first.starts_with("20"), "{first}");
    }
}
