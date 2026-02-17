use std::time::Duration;

use chrono::{Local, NaiveTime};

use muralis_core::config::ScheduleEntry;

/// Parse interval string like "30m", "1h", "90s" into Duration.
pub fn parse_interval(s: &str) -> Option<Duration> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    let (num_str, suffix) = if s.ends_with('s') {
        (&s[..s.len() - 1], 's')
    } else if s.ends_with('m') {
        (&s[..s.len() - 1], 'm')
    } else if s.ends_with('h') {
        (&s[..s.len() - 1], 'h')
    } else {
        // default to seconds if no suffix
        (s, 's')
    };

    let num: u64 = num_str.parse().ok()?;
    let secs = match suffix {
        's' => num,
        'm' => num * 60,
        'h' => num * 3600,
        _ => return None,
    };

    Some(Duration::from_secs(secs))
}

/// Find the next schedule entry that should trigger, and how long until it fires.
pub fn next_schedule_trigger(schedules: &[ScheduleEntry]) -> Option<(Duration, Vec<String>)> {
    if schedules.is_empty() {
        return None;
    }

    let now = Local::now().time();
    let mut best_duration: Option<Duration> = None;
    let mut best_tags = Vec::new();

    for entry in schedules {
        if let Ok(target) = NaiveTime::parse_from_str(&entry.time, "%H:%M") {
            let secs_until = seconds_until(now, target);
            let dur = Duration::from_secs(secs_until as u64);

            if best_duration.is_none() || dur < best_duration.unwrap() {
                best_duration = Some(dur);
                best_tags = entry.tags.clone();
            }
        }
    }

    best_duration.map(|d| (d, best_tags))
}

fn seconds_until(now: NaiveTime, target: NaiveTime) -> i64 {
    let diff = target.signed_duration_since(now).num_seconds();
    if diff > 0 {
        diff
    } else {
        diff + 86400 // wrap to next day
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_interval() {
        assert_eq!(parse_interval("30m"), Some(Duration::from_secs(1800)));
        assert_eq!(parse_interval("1h"), Some(Duration::from_secs(3600)));
        assert_eq!(parse_interval("90s"), Some(Duration::from_secs(90)));
        assert_eq!(parse_interval("60"), Some(Duration::from_secs(60)));
        assert_eq!(parse_interval(""), None);
        assert_eq!(parse_interval("abc"), None);
    }

    #[test]
    fn test_seconds_until_future() {
        let now = NaiveTime::from_hms_opt(10, 0, 0).unwrap();
        let target = NaiveTime::from_hms_opt(12, 0, 0).unwrap();
        assert_eq!(seconds_until(now, target), 7200);
    }

    #[test]
    fn test_seconds_until_past_wraps() {
        let now = NaiveTime::from_hms_opt(23, 0, 0).unwrap();
        let target = NaiveTime::from_hms_opt(1, 0, 0).unwrap();
        assert_eq!(seconds_until(now, target), 7200);
    }

    #[test]
    fn test_next_schedule_trigger() {
        let schedules = vec![
            ScheduleEntry {
                time: "08:00".into(),
                tags: vec!["morning".into()],
            },
            ScheduleEntry {
                time: "20:00".into(),
                tags: vec!["evening".into()],
            },
        ];

        let result = next_schedule_trigger(&schedules);
        assert!(result.is_some());
        let (_, tags) = result.unwrap();
        assert!(!tags.is_empty());
    }
}
