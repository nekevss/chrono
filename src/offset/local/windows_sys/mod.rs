use std::io::Error;
use std::ptr;
use std::result::Result;
use std::fmt;

use windows_sys::Win32::{
    Foundation::{FILETIME, SYSTEMTIME},
    System::Time::{
        FileTimeToSystemTime, GetTimeZoneInformation, SystemTimeToFileTime, 
        SystemTimeToTzSpecificLocalTime, TzSpecificLocalTimeToSystemTime, 
        TIME_ZONE_INFORMATION, TIME_ZONE_ID_INVALID,
    },
};

const HECTONANOSECS_IN_SEC: i64 = 10_000_000;
const HECTONANOSEC_TO_UNIX_EPOCH: i64 = 11_644_473_600 * HECTONANOSECS_IN_SEC;

macro_rules! call {
    ($name:ident($($arg:expr),*)) => {
        if $name($($arg),*) == 0 {
            return Err(Error::last_os_error());
        }
    }
}

pub(crate) struct WinSystemTime {
    inner: SYSTEMTIME,
}

impl fmt::Debug for WinSystemTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WinSystemTime")
            .field("year", &self.inner.wYear)
            .field("month", &self.inner.wMonth)
            .field("dayOfWeek", &self.inner.wDayOfWeek)
            .field("day", &self.inner.wDay)
            .field("hour", &self.inner.wHour)
            .field("minute", &self.inner.wMinute)
            .field("second", &self.inner.wSecond)
            .field("ms", &self.inner.wMilliseconds)
            .finish()
    }
}

impl WinSystemTime {
    pub(crate) fn new() -> Self {
        let st = SYSTEMTIME {
            wYear: 0,
            wMonth: 0,
            wDayOfWeek: 0,
            wDay: 0,
            wHour: 0,
            wMinute: 0,
            wSecond: 0,
            wMilliseconds: 0,
        };

        Self {
            inner: st,
        }
    }

    pub(crate) fn inner(&self) -> SYSTEMTIME {
        self.inner
    }

    pub(crate) fn mut_inner(&mut self) -> &mut SYSTEMTIME {
        &mut self.inner
    }

    pub(crate) fn from_local_time(local: &WinSystemTime) -> Result<WinSystemTime, Error> {
        let mut sys_time = Self::new();
        unsafe { call!(TzSpecificLocalTimeToSystemTime(ptr::null(), &local.inner(), sys_time.mut_inner())) };
        Ok(sys_time)
    }

    pub(crate) fn as_time_zone_specific(&self) -> Result<WinSystemTime, Error> {
        let mut local = WinSystemTime::new();
        unsafe { call!(SystemTimeToTzSpecificLocalTime(ptr::null(), &self.inner(), local.mut_inner())) };
        Ok(local)
    }

    pub(crate) fn as_file_time(&self) -> Result<WinFileTime, Error> {
        let mut filetime = WinFileTime::new();
        unsafe { call!( SystemTimeToFileTime(&self.inner(), filetime.mut_inner())) };
        Ok(filetime)
    }
}

pub(crate) struct WinFileTime {
    inner: FILETIME,
}

impl WinFileTime {
    pub(crate) fn new() -> Self {
        let ft = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
        Self {
            inner: ft,
        }
    }

    pub(crate) fn inner(&self) -> FILETIME {
        self.inner
    }

    pub(crate) fn from_seconds(sec: i64) -> Self {
        let t = ((sec * HECTONANOSECS_IN_SEC) + HECTONANOSEC_TO_UNIX_EPOCH) as u64;
        let ft = FILETIME { dwLowDateTime: t as u32, dwHighDateTime: (t >> 32) as u32 };
        Self {
            inner: ft
        }
    }

    pub(crate) fn mut_inner(&mut self) -> &mut FILETIME {
        &mut self.inner
    }

    pub(crate) const fn as_unix_seconds(&self) -> i64 {
        let t = self.as_u64() as i64;
        ((t - HECTONANOSEC_TO_UNIX_EPOCH) / HECTONANOSECS_IN_SEC) as i64
    }

    pub(crate) const fn as_u64(&self) -> u64 {
        ((self.inner.dwHighDateTime as u64) << 32) | (self.inner.dwLowDateTime as u64)
    }

    pub(crate) fn as_system_time(&self) -> Result<WinSystemTime, Error> {
        let mut st = WinSystemTime::new();
        unsafe { call!(FileTimeToSystemTime(&self.inner(), st.mut_inner())) };
        Ok(st)
    }
}

pub(crate) struct WinTimeZoneInfo {
    inner: TIME_ZONE_INFORMATION,
}

impl WinTimeZoneInfo {
    pub(crate) fn new() -> Result<Self, Error> {
        let mut tz = TIME_ZONE_INFORMATION {
            Bias: 0,
            StandardName: [0_u16; 32],
            StandardDate: WinSystemTime::new().inner(),
            StandardBias: 0,
            DaylightName: [0_u16; 32],
            DaylightDate: WinSystemTime::new().inner(),
            DaylightBias: 0,
        };

        unsafe {
            let result = GetTimeZoneInformation(&mut tz);
            if result == TIME_ZONE_ID_INVALID {
                return Err(Error::last_os_error());
            }
        }

        Ok(Self {
            inner: tz,
        })
    }

    pub(crate) const fn bias(&self) -> i32 {
        self.inner.Bias
    }

    pub(crate) const fn standard_bias(&self) -> i32 {
        self.inner.StandardBias
    }
}