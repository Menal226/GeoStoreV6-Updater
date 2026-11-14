use crate::i18n::{self, Lang};
use crate::t;
use chrono::{DateTime, Local, NaiveDate};

pub fn date(d: NaiveDate) -> String {
    d.format(t!("fmt.date_format")).to_string()
}

pub fn datetime(dt: DateTime<Local>) -> String {
    let today = Local::now().date_naive();
    let d = dt.date_naive();
    let time = dt.format("%H:%M");
    if d == today {
        t!("fmt.today_at", time = time)
    } else if Some(d) == today.pred_opt() {
        t!("fmt.yesterday_at", time = time)
    } else if Some(d) == today.succ_opt() {
        t!("fmt.tomorrow_at", time = time)
    } else {
        t!("fmt.date_at", date = date(d), time = time)
    }
}

pub fn mb(bytes: u64) -> String {
    let v = bytes as f64 / 1_048_576.0;
    if v >= 100.0 {
        format!("{v:.0} MB")
    } else if i18n::current() == Lang::Cs {
        format!("{v:.1} MB").replace('.', ",")
    } else {
        format!("{v:.1} MB")
    }
}

pub fn duration(secs: u64) -> String {
    if secs >= 3600 {
        format!("{}:{:02}:{:02}", secs / 3600, (secs / 60) % 60, secs % 60)
    } else {
        format!("{}:{:02}", secs / 60, secs % 60)
    }
}

pub fn packages(n: usize) -> &'static str {
    i18n::plural("fmt.packages", n as u64)
}
