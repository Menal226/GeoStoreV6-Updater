use chrono::{DateTime, Local, NaiveDate};

pub fn date(d: NaiveDate) -> String {
    d.format("%d.%m.%Y").to_string()
}

pub fn datetime(dt: DateTime<Local>) -> String {
    let today = Local::now().date_naive();
    let d = dt.date_naive();
    if d == today {
        format!("dnes v {}", dt.format("%H:%M"))
    } else if Some(d) == today.pred_opt() {
        format!("včera v {}", dt.format("%H:%M"))
    } else if Some(d) == today.succ_opt() {
        format!("zítra v {}", dt.format("%H:%M"))
    } else {
        format!("{} v {}", date(d), dt.format("%H:%M"))
    }
}

pub fn mb(bytes: u64) -> String {
    let v = bytes as f64 / 1_048_576.0;
    if v >= 100.0 {
        format!("{v:.0} MB")
    } else {
        format!("{v:.1} MB").replace('.', ",")
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
    match n {
        1 => "balíček",
        2..=4 => "balíčky",
        _ => "balíčků",
    }
}
