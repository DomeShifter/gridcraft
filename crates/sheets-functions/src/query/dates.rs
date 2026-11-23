//! Dates as spreadsheet serial numbers (days since 30 Dec 1899, time as the fraction).

use gridcraft_core::date::{civil_from_days, days_from_civil};

const EPOCH_OFFSET: i64 = 25_569; // serial of 1 Jan 1970

/// The serial of a `date '2026-01-31'`, `datetime '2026-01-31 13:45:00'` or `timeofday '13:45:00'`.
pub fn literal(kind: &str, s: &str) -> Option<f64> {
    let s = s.trim();
    match kind {
        "date" => ymd(s),
        "datetime" => {
            let (d, t) = s.split_once([' ', 'T'])?;
            Some(ymd(d)? + time(t)?)
        }
        _ => time(s),
    }
}

fn ymd(s: &str) -> Option<f64> {
    let mut p = s.split('-');
    let (y, m, d) = (p.next()?.parse::<i32>().ok()?, p.next()?.parse::<u32>().ok()?, p.next()?.parse::<u32>().ok()?);
    if p.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some((days_from_civil(y, m, d) + EPOCH_OFFSET) as f64)
}

fn time(s: &str) -> Option<f64> {
    let mut p = s.split(':');
    let h: f64 = p.next()?.parse().ok()?;
    let m: f64 = p.next()?.parse().ok()?;
    let sec: f64 = p.next().map_or(Some(0.0), |x| x.parse().ok())?;
    if !(0.0..24.0).contains(&h) || !(0.0..60.0).contains(&m) || !(0.0..60.0).contains(&sec) {
        return None;
    }
    Some((h * 3600.0 + m * 60.0 + sec) / 86_400.0)
}

/// (year, month 1-12, day) of a serial.
pub fn parts(serial: f64) -> (i32, u32, u32) {
    civil_from_days(serial.floor() as i64 - EPOCH_OFFSET)
}

/// (hour, minute, second, millisecond) of a serial's time.
pub fn clock(serial: f64) -> (u32, u32, u32, u32) {
    let ms = ((serial - serial.floor()) * 86_400_000.0).round() as u64;
    ((ms / 3_600_000) as u32, (ms / 60_000 % 60) as u32, (ms / 1000 % 60) as u32, (ms % 1000) as u32)
}
