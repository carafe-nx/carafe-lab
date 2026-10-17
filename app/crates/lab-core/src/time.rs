//! Calendar time for run folders and the report, without a date library.

/// A moment in UTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Utc {
    /// Year.
    pub year: i64,
    /// Month, 1…12.
    pub month: u32,
    /// Day, 1…31.
    pub day: u32,
    /// Hour, 0…23.
    pub hour: u32,
    /// Minute, 0…59.
    pub minute: u32,
    /// Second, 0…59.
    pub second: u32,
}

impl Utc {
    /// Converts seconds since 1970-01-01 00:00 UTC.
    #[must_use]
    pub fn from_unix(seconds: u64) -> Self {
        let days = (seconds / 86_400) as i64;
        let rest = seconds % 86_400;
        let shifted = days + 719_468;
        let era = shifted.div_euclid(146_097);
        let day_of_era = shifted.rem_euclid(146_097);
        let year_of_era =
            (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
        let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let month_index = (5 * day_of_year + 2) / 153;
        let day = (day_of_year - (153 * month_index + 2) / 5 + 1) as u32;
        let month = if month_index < 10 {
            month_index + 3
        } else {
            month_index - 9
        } as u32;
        let year = year_of_era + era * 400 + i64::from(month <= 2);
        Self {
            year,
            month,
            day,
            hour: (rest / 3600) as u32,
            minute: (rest % 3600 / 60) as u32,
            second: (rest % 60) as u32,
        }
    }

    /// Returns `YYYY-MM-DD HH:MM`.
    #[must_use]
    pub fn minutes(self) -> String {
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}",
            self.year, self.month, self.day, self.hour, self.minute
        )
    }

    /// Returns `YYYY-MM-DD_HH-MM-SS`, usable as a folder name.
    #[must_use]
    pub fn folder(self) -> String {
        format!(
            "{:04}-{:02}-{:02}_{:02}-{:02}-{:02}",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }

    /// Returns the date as a number, `YYYYMMDD`.
    #[must_use]
    pub fn date_number(self) -> u64 {
        u64::try_from(self.year).unwrap_or(0) * 10_000
            + u64::from(self.month) * 100
            + u64::from(self.day)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_moments_convert() {
        assert_eq!(Utc::from_unix(0).minutes(), "1970-01-01 00:00");
        assert_eq!(Utc::from_unix(951_782_400).minutes(), "2000-02-29 00:00");
        assert_eq!(
            Utc::from_unix(1_791_806_405).folder(),
            "2026-10-12_12-00-05"
        );
    }

    #[test]
    fn the_date_number_reads_as_a_date() {
        assert_eq!(Utc::from_unix(1_791_806_405).date_number(), 20_261_012);
    }
}
