// Copyright 2012-2014 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use std::time::{SystemTime, UNIX_EPOCH};
use std::result::Result;
use std::io::Error;

use super::windows_sys::{WinFileTime, WinSystemTime, WinTimeZoneInfo};

use super::{FixedOffset, Local};
use crate::{DateTime, Datelike, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, Timelike};


pub(super) fn now() -> DateTime<Local> {
    let datetime = tm_to_datetime(Timespec::now().local());
    datetime.single().expect("invalid time")
}

/// Converts a local `NaiveDateTime` to the `time::Timespec`.
pub(super) fn naive_to_local(d: &NaiveDateTime, local: bool) -> LocalResult<DateTime<Local>> {
    let tm = Tm {
        tm_sec: d.second() as i32,
        tm_min: d.minute() as i32,
        tm_hour: d.hour() as i32,
        tm_mday: d.day() as i32,
        tm_mon: d.month0() as i32, // yes, C is that strange...
        tm_year: d.year() - 1900, // this doesn't underflow, we know that d is `NaiveDateTime`.
        tm_wday: 0,                // to_local ignores this
        tm_yday: 0,                // and this
        tm_isdst: -1,
        // This seems pretty fake?
        tm_utcoff: i32::from(local),
        // do not set this, OS APIs are heavily inconsistent in terms of leap second handling
        tm_nsec: 0,
    };

    let spec = Timespec {
        sec: match local {
            false => {
                match tm.utc_to_time() {
                    Ok(sec) => sec,
                    Err(_) => return LocalResult::None,
                }
            }
            true => {
                match tm.local_to_time() {
                    Ok(sec) => sec,
                    Err(_) => return LocalResult::None,
                }
            }
        },
        nsec: tm.tm_nsec,
    };

    // Adjust for leap seconds
    let mut tm = spec.local();
    assert_eq!(tm.tm_nsec, 0);
    tm.tm_nsec = d.nanosecond() as i32;

    tm_to_datetime(tm)
}



/// Converts a `time::Tm` struct into the timezone-aware `DateTime`.
fn tm_to_datetime(mut tm: Tm) -> LocalResult<DateTime<Local>> {
    if tm.tm_sec >= 60 {
        tm.tm_nsec += (tm.tm_sec - 59) * 1_000_000_000;
        tm.tm_sec = 59;
    }

    let date = NaiveDate::from_ymd_opt(tm.tm_year + 1900, tm.tm_mon as u32 + 1, tm.tm_mday as u32)
        .unwrap();

    let time = NaiveTime::from_hms_nano_opt(
        tm.tm_hour as u32,
        tm.tm_min as u32,
        tm.tm_sec as u32,
        tm.tm_nsec as u32,
    );

    match time {
        Some(time) => {
            let offset = FixedOffset::east_opt(tm.tm_utcoff).unwrap();
            let datetime = DateTime::from_utc(date.and_time(time) - offset, offset);
            // #TODO - there should be ambiguous cases, investigate?
            LocalResult::Single(datetime)
        }
        None => LocalResult::None,
    }
}

/// A record specifying a time value in seconds and nanoseconds, where
/// nanoseconds represent the offset from the given second.
///
/// For example a timespec of 1.2 seconds after the beginning of the epoch would
/// be represented as {sec: 1, nsec: 200000000}.
struct Timespec {
    sec: i64,
    nsec: i32,
}

impl Timespec {
    /// Constructs a timespec representing the current time in UTC.
    fn now() -> Timespec {
        let st =
            SystemTime::now().duration_since(UNIX_EPOCH).expect("system time before Unix epoch");
        Timespec { sec: st.as_secs() as i64, nsec: st.subsec_nanos() as i32 }
    }

    /// Converts this timespec into the system's local time.
    fn local(self) -> Tm {
        Tm::from_timespec(self)
    }
}

/// Holds a calendar date and time broken down into its components (year, month,
/// day, and so on), also called a broken-down time value.
// FIXME: use c_int instead of i32?
#[derive(Default)]
#[repr(C)]
struct Tm {
    /// Seconds after the minute - [0, 60]
    tm_sec: i32,

    /// Minutes after the hour - [0, 59]
    tm_min: i32,

    /// Hours after midnight - [0, 23]
    tm_hour: i32,

    /// Day of the month - [1, 31]
    tm_mday: i32,

    /// Months since January - [0, 11]
    tm_mon: i32,

    /// Years since 1900
    tm_year: i32,

    /// Days since Sunday - [0, 6]. 0 = Sunday, 1 = Monday, ..., 6 = Saturday.
    tm_wday: i32,

    /// Days since January 1 - [0, 365]
    tm_yday: i32,

    /// Daylight Saving Time flag.
    ///
    /// This value is positive if Daylight Saving Time is in effect, zero if
    /// Daylight Saving Time is not in effect, and negative if this information
    /// is not available.
    tm_isdst: i32,

    /// Identifies the time zone that was used to compute this broken-down time
    /// value, including any adjustment for Daylight Saving Time. This is the
    /// number of seconds east of UTC. For example, for U.S. Pacific Daylight
    /// Time, the value is `-7*60*60 = -25200`.
    tm_utcoff: i32,

    /// Nanoseconds after the second - [0, 10<sup>9</sup> - 1]
    tm_nsec: i32,
}

impl Tm {
    /// # Panics
    ///
    /// Can panic if Window's SystemCall fails.
    pub(crate) fn from_timespec(timespec: Timespec) -> Self {
        let mut tm = Self::default();
        tm.update_from_seconds(timespec.sec).unwrap();
        tm.tm_nsec = timespec.nsec;
        tm
    }

    // TODO: consider changing to update_ from set_
    pub(crate) fn update_from_seconds(&mut self, sec: i64) -> Result<(), Error> {
            let filetime = WinFileTime::from_seconds(sec);
            let utc = filetime.as_system_time()?;
            let local = utc.as_time_zone_specific()?;
            self.update_from_system_time(&local);

            let local_filetime = local.as_file_time()?;
            let local_sec = local_filetime.as_unix_seconds();

            let tz = WinTimeZoneInfo::new()?;

            // SystemTimeToTzSpecificLocalTime already applied the biases so
            // check if it non standard
            self.tm_utcoff = (local_sec - sec) as i32;
            self.tm_isdst = if self.tm_utcoff == -60 * (tz.bias() + tz.standard_bias()) { 0 } else { 1 };
            Ok(())
        }

    pub(crate) fn update_from_system_time(&mut self, sys: &WinSystemTime) {
        self.tm_sec = sys.inner().wSecond as i32;
        self.tm_min = sys.inner().wMinute as i32;
        self.tm_hour = sys.inner().wHour as i32;
        self.tm_mday = sys.inner().wDay as i32;
        self.tm_wday = sys.inner().wDayOfWeek as i32;
        self.tm_mon = (sys.inner().wMonth - 1) as i32;
        self.tm_year = (sys.inner().wYear as i32) - 1900;
        self.tm_yday = yday(self.tm_year, self.tm_mon + 1, self.tm_mday);
    }

    pub(crate) fn as_system_time(&self) -> WinSystemTime {
        let mut sys = WinSystemTime::new();
        sys.mut_inner().wSecond = self.tm_sec as u16;
        sys.mut_inner().wMinute = self.tm_min as u16;
        sys.mut_inner().wHour = self.tm_hour as u16;
        sys.mut_inner().wDay = self.tm_mday as u16;
        sys.mut_inner().wDayOfWeek = self.tm_wday as u16;
        sys.mut_inner().wMonth = (self.tm_mon + 1) as u16;
        sys.mut_inner().wYear = (self.tm_year + 1900) as u16;
        sys
    }

    fn utc_to_time(&self) -> Result<i64, Error> {
        let sys_time = self.as_system_time();
        let filetime = sys_time.as_file_time()?;
        Ok(filetime.as_unix_seconds())
    }

    fn local_to_time(&self) -> Result<i64, Error> {
        let sys_time = self.as_system_time();
        let utc = WinSystemTime::from_local_time(&sys_time)?;
        let filetime = utc.as_file_time()?;
        Ok(filetime.as_unix_seconds())
    }
}

fn yday(year: i32, month: i32, day: i32) -> i32 {
    let leap = if month > 2 {
        if year % 4 == 0 {
            1
        } else {
            2
        }
    } else {
        0
    };
    let july = i32::from(month > 7);

    (month - 1) * 30 + month / 2 + (day - 1) - leap + july
}

