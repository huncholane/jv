use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Offset, Utc};

#[derive(Debug, Clone, PartialEq)]
pub enum TemporalValue {
    DateTime(DateTime<chrono::FixedOffset>),
    DateTimeUtc(DateTime<Utc>),
    NaiveDateTime(NaiveDateTime),
    NaiveDate(NaiveDate),
    NaiveTime(NaiveTime),
    UnixTimestamp(i64, DateTime<Utc>),
}

impl TemporalValue {
    pub fn display(&self) -> String {
        match self {
            Self::DateTime(dt) => dt.format("%Y-%m-%d %H:%M:%S %Z").to_string(),
            Self::DateTimeUtc(dt) => dt.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
            Self::NaiveDateTime(dt) => dt.format("%Y-%m-%d %H:%M:%S").to_string(),
            Self::NaiveDate(d) => d.format("%Y-%m-%d").to_string(),
            Self::NaiveTime(t) => t.format("%H:%M:%S").to_string(),
            Self::UnixTimestamp(ts, dt) => {
                format!("{} (unix: {})", dt.format("%Y-%m-%d %H:%M:%S UTC"), ts)
            }
        }
    }

    pub fn relative_time(&self) -> String {
        let now = Utc::now();
        let dt_utc = match self {
            Self::DateTime(dt) => dt.with_timezone(&Utc),
            Self::DateTimeUtc(dt) => *dt,
            Self::NaiveDateTime(dt) => dt.and_utc(),
            Self::NaiveDate(d) => d
                .and_hms_opt(0, 0, 0)
                .unwrap_or_default()
                .and_utc(),
            Self::NaiveTime(_) => return String::new(),
            Self::UnixTimestamp(_, dt) => *dt,
        };

        let duration = now.signed_duration_since(dt_utc);
        let secs = duration.num_seconds().unsigned_abs();

        if secs < 60 {
            "just now".to_string()
        } else if secs < 3600 {
            let mins = secs / 60;
            format!("{}m ago", mins)
        } else if secs < 86400 {
            let hours = secs / 3600;
            format!("{}h ago", hours)
        } else if secs < 2592000 {
            let days = secs / 86400;
            format!("{}d ago", days)
        } else if secs < 31536000 {
            let months = secs / 2592000;
            format!("{}mo ago", months)
        } else {
            let years = secs / 31536000;
            format!("{}y ago", years)
        }
    }

    pub fn to_naive_date(&self) -> Option<NaiveDate> {
        match self {
            Self::DateTime(dt) => Some(dt.date_naive()),
            Self::DateTimeUtc(dt) => Some(dt.date_naive()),
            Self::NaiveDateTime(dt) => Some(dt.date()),
            Self::NaiveDate(d) => Some(*d),
            Self::NaiveTime(_) => None,
            Self::UnixTimestamp(_, dt) => Some(dt.date_naive()),
        }
    }

    pub fn timezone_info(&self) -> Option<String> {
        match self {
            Self::DateTime(dt) => Some(dt.offset().to_string()),
            Self::DateTimeUtc(_) | Self::UnixTimestamp(_, _) => Some("UTC".to_string()),
            _ => None,
        }
    }

    /// Returns UTC offset in fractional hours, if timezone is known
    pub fn utc_offset_hours(&self) -> Option<f32> {
        match self {
            Self::DateTime(dt) => Some(dt.offset().local_minus_utc() as f32 / 3600.0),
            Self::DateTimeUtc(_) | Self::UnixTimestamp(_, _) => Some(0.0),
            _ => None,
        }
    }
}

pub fn detect_temporal(value: &str) -> Option<TemporalValue> {
    // ISO 8601 with timezone
    if let Ok(dt) = DateTime::parse_from_rfc3339(value) {
        return Some(TemporalValue::DateTime(dt));
    }

    // RFC 2822
    if let Ok(dt) = DateTime::parse_from_rfc2822(value) {
        return Some(TemporalValue::DateTime(dt));
    }

    // ISO 8601 variations
    for fmt in &[
        "%Y-%m-%dT%H:%M:%S%.f%:z",
        "%Y-%m-%dT%H:%M:%S%:z",
        "%Y-%m-%dT%H:%M:%SZ",
        "%Y-%m-%dT%H:%M:%S%.fZ",
        "%Y-%m-%d %H:%M:%S%:z",
        "%Y-%m-%d %H:%M:%S%.f%:z",
    ] {
        if let Ok(dt) = DateTime::parse_from_str(value, fmt) {
            return Some(TemporalValue::DateTime(dt));
        }
    }

    // Naive datetime
    for fmt in &[
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y/%m/%d %H:%M:%S",
    ] {
        if let Ok(dt) = NaiveDateTime::parse_from_str(value, fmt) {
            return Some(TemporalValue::NaiveDateTime(dt));
        }
    }

    // Naive date
    for fmt in &["%Y-%m-%d", "%Y/%m/%d", "%m/%d/%Y", "%d/%m/%Y"] {
        if let Ok(d) = NaiveDate::parse_from_str(value, fmt) {
            return Some(TemporalValue::NaiveDate(d));
        }
    }

    // Naive time
    for fmt in &["%H:%M:%S", "%H:%M:%S%.f", "%H:%M"] {
        if let Ok(t) = NaiveTime::parse_from_str(value, fmt) {
            return Some(TemporalValue::NaiveTime(t));
        }
    }

    None
}

pub fn detect_unix_timestamp(value: i64) -> Option<TemporalValue> {
    // Reasonable range: 2000-01-01 to 2099-12-31
    if (946684800..=4102444800).contains(&value) {
        if let Some(dt) = DateTime::from_timestamp(value, 0) {
            return Some(TemporalValue::UnixTimestamp(value, dt));
        }
    }
    // Millisecond timestamps
    if (946684800000..=4102444800000).contains(&value) {
        let secs = value / 1000;
        let nanos = ((value % 1000) * 1_000_000) as u32;
        if let Some(dt) = DateTime::from_timestamp(secs, nanos) {
            return Some(TemporalValue::UnixTimestamp(value, dt));
        }
    }
    None
}

/// Info about an IANA timezone string like "America/Los_Angeles"
pub struct TimezoneInfo {
    pub name: String,
    pub abbreviation: String,
    pub offset_hours: f32,
    pub display: String,
}

/// Detect IANA timezone names like "America/Los_Angeles", "Europe/London", "UTC"
pub fn detect_timezone(value: &str) -> Option<TimezoneInfo> {
    let tz: chrono_tz::Tz = value.parse().ok()?;
    let now = Utc::now().with_timezone(&tz);
    let offset_secs = now.offset().fix().local_minus_utc();
    let offset_hours = offset_secs as f32 / 3600.0;
    let h = offset_secs / 3600;
    let m = (offset_secs % 3600).abs() / 60;
    let offset_str = if m == 0 {
        format!("UTC{:+}", h)
    } else {
        format!("UTC{:+}:{:02}", h, m)
    };
    Some(TimezoneInfo {
        name: value.to_string(),
        abbreviation: now.format("%Z").to_string(),
        offset_hours,
        display: format!("{} ({})", offset_str, now.format("%Z")),
    })
}
