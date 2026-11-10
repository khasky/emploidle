use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Schedule {
    pub start: String,
    pub finish: String,
    #[serde(default = "every_day")]
    pub days: [bool; 7],
}

fn every_day() -> [bool; 7] {
    [true; 7]
}

fn weekday(day: i64) -> usize {
    // Civil day zero is Thursday, January 1, 1970; the selection starts on Monday.
    ((day.rem_euclid(7) + 3) % 7) as usize
}
impl Default for Schedule {
    fn default() -> Self {
        Self {
            start: "00:00".into(),
            finish: "00:00".into(),
            days: every_day(),
        }
    }
}
fn minute(value: &str) -> Option<u16> {
    let bytes = value.as_bytes();
    if bytes.len() != 5
        || bytes[2] != b':'
        || ![bytes[0], bytes[1], bytes[3], bytes[4]]
            .iter()
            .all(u8::is_ascii_digit)
    {
        return None;
    }
    let hour = (bytes[0] - b'0') as u16 * 10 + (bytes[1] - b'0') as u16;
    let minute = (bytes[3] - b'0') as u16 * 10 + (bytes[4] - b'0') as u16;
    (hour < 24 && minute < 60).then_some(hour * 60 + minute)
}
impl Schedule {
    pub fn validate(&self) -> Result<(), String> {
        let start = minute(&self.start).ok_or("Start must use HH:MM, from 00:00 to 23:59")?;
        let finish = minute(&self.finish).ok_or("Finish must use HH:MM, from 00:00 to 23:59")?;
        if start == finish && start != 0 {
            return Err(
                "Start and finish must differ, or both be 00:00 to disable the schedule".into(),
            );
        }
        Ok(())
    }
    pub fn active_shift(&self, clock: LocalClock) -> Option<i64> {
        let (start, finish) = (minute(&self.start)?, minute(&self.finish)?);
        if start == finish || clock.minute >= 1440 {
            return None;
        }
        let shift = if start < finish {
            (start <= clock.minute && clock.minute < finish).then_some(clock.day)
        } else if clock.minute >= start {
            Some(clock.day)
        } else if clock.minute < finish {
            Some(clock.day - 1)
        } else {
            None
        };
        shift.filter(|day| self.days[weekday(*day)])
    }
    fn enabled(&self) -> bool {
        self.validate().is_ok() && self.start != self.finish && self.days.contains(&true)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct LocalClock {
    pub day: i64,
    pub minute: u16,
}

fn civil_day(year: i32, month: u16, day: u16) -> i64 {
    let year = i64::from(year) - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let y = year - era * 400;
    let m = i64::from(month) + if month > 2 { -3 } else { 9 };
    let doy = (153 * m + 2) / 5 + i64::from(day) - 1;
    era * 146097 + y * 365 + y / 4 - y / 100 + doy - 719468
}

impl LocalClock {
    #[cfg(target_os = "windows")]
    pub fn now() -> Result<Self, String> {
        #[repr(C)]
        #[derive(Default)]
        struct SystemTime {
            year: u16,
            month: u16,
            weekday: u16,
            day: u16,
            hour: u16,
            minute: u16,
            second: u16,
            millis: u16,
        }
        #[link(name = "kernel32")]
        extern "system" {
            fn GetLocalTime(time: *mut SystemTime);
        }
        let mut time = SystemTime::default();
        unsafe {
            GetLocalTime(&mut time);
        }
        Ok(Self {
            day: civil_day(i32::from(time.year), time.month, time.day),
            minute: time.hour * 60 + time.minute,
        })
    }
    #[cfg(unix)]
    pub fn now() -> Result<Self, String> {
        #[repr(C)]
        struct LocalTime {
            second: i32,
            minute: i32,
            hour: i32,
            day: i32,
            month: i32,
            year: i32,
            weekday: i32,
            yearday: i32,
            dst: i32,
            offset: i64,
            zone: *const std::ffi::c_char,
        }
        extern "C" {
            fn localtime_r(time: *const i64, result: *mut LocalTime) -> *mut LocalTime;
        }
        let seconds = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_secs() as i64;
        let mut time: LocalTime = unsafe { std::mem::zeroed() };
        if unsafe { localtime_r(&seconds, &mut time) }.is_null() {
            return Err("Cannot read local system time".into());
        }
        Ok(Self {
            day: civil_day(time.year + 1900, (time.month + 1) as u16, time.day as u16),
            minute: (time.hour * 60 + time.minute) as u16,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Start,
    Stop,
}

pub struct Scheduler {
    schedule: Schedule,
    suppressed: Option<i64>,
    bound_shift: Option<i64>,
    last_started: Option<i64>,
    finish_at: Option<(i64, u16)>,
    running: bool,
}
impl Scheduler {
    pub fn new(schedule: Schedule) -> Self {
        Self {
            schedule,
            suppressed: None,
            bound_shift: None,
            last_started: None,
            finish_at: None,
            running: false,
        }
    }
    pub fn update(&mut self, schedule: Schedule) -> bool {
        if self.schedule == schedule {
            return false;
        }
        self.schedule = schedule;
        self.suppressed = None;
        self.bound_shift = None;
        self.last_started = None;
        self.finish_at = None;
        self.running = false;
        true
    }
    pub fn manual_start(&mut self, clock: LocalClock) {
        self.running = true;
        self.suppressed = None;
        self.bound_shift = self.schedule.active_shift(clock);
        self.finish_at = self.next_finish(clock);
        if let Some(shift) = self.bound_shift {
            self.last_started = Some(self.last_started.map_or(shift, |last| last.max(shift)));
        }
    }
    pub fn manual_stop(&mut self, clock: LocalClock) {
        self.running = false;
        self.bound_shift = None;
        self.finish_at = None;
        self.suppressed = self.schedule.active_shift(clock);
    }
    fn next_finish(&self, clock: LocalClock) -> Option<(i64, u16)> {
        if !self.schedule.enabled() {
            return None;
        }
        let finish = minute(&self.schedule.finish)?;
        let overnight = i64::from(minute(&self.schedule.start)? > finish);
        (clock.day - 1..=clock.day + 7)
            .filter(|day| self.schedule.days[weekday(*day)])
            .map(|day| (day + overnight, finish))
            .find(|end| *end > (clock.day, clock.minute))
    }
    pub fn tick(&mut self, clock: LocalClock) -> Option<Decision> {
        if !self.schedule.enabled() {
            return None;
        }
        if self.running && self.finish_at.is_none() {
            self.finish_at = self.next_finish(clock);
        }
        let current = self.schedule.active_shift(clock);
        if let Some(shift) = current {
            if self.bound_shift == current || self.suppressed == current {
                return None;
            }
            if self.last_started.is_none_or(|last| shift > last) {
                self.last_started = Some(shift);
                self.bound_shift = Some(shift);
                self.running = true;
                self.finish_at = self.next_finish(clock);
                return Some(Decision::Start);
            }
        }
        if self.running
            && self
                .finish_at
                .is_some_and(|finish| (clock.day, clock.minute) >= finish)
        {
            self.running = false;
            self.bound_shift = None;
            self.finish_at = None;
            return Some(Decision::Stop);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saving_the_same_schedule_preserves_manual_stop() {
        let shift = schedule("09:00", "17:00");
        let mut scheduler = Scheduler::new(shift.clone());
        assert_eq!(scheduler.tick(clock(1, "09:00")), Some(Decision::Start));
        scheduler.manual_stop(clock(1, "10:00"));
        scheduler.update(shift);
        assert_eq!(scheduler.tick(clock(1, "11:00")), None);
        assert_eq!(scheduler.tick(clock(2, "09:00")), Some(Decision::Start));
    }
    #[test]
    fn repeated_discovery_ticks_do_not_duplicate_starts_or_revoke_manual_stop() {
        let mut scheduler = Scheduler::new(schedule("09:00", "17:00"));
        assert_eq!(scheduler.tick(clock(1, "09:00")), Some(Decision::Start));
        assert_eq!(scheduler.tick(clock(1, "09:01")), None);
        scheduler.manual_stop(clock(1, "10:00"));
        assert_eq!(scheduler.tick(clock(1, "10:01")), None);
    }
    fn clock(day: i64, time: &str) -> LocalClock {
        LocalClock {
            day,
            minute: minute(time).unwrap(),
        }
    }
    fn schedule(start: &str, finish: &str) -> Schedule {
        Schedule {
            start: start.into(),
            finish: finish.into(),
            ..Schedule::default()
        }
    }
    #[test]
    fn validates_canonical_times_and_disabled_default() {
        assert!(Schedule::default().validate().is_ok());
        for invalid in ["1:00", "24:00", "12:60", "09:0x", " 09:00", "１２:00"] {
            assert!(schedule(invalid, "17:00").validate().is_err());
        }
        assert!(schedule("09:00", "09:00").validate().is_err());
        assert!(schedule("23:59", "00:00").validate().is_ok());
    }
    #[test]
    fn boundaries_and_manual_stop_suppression() {
        let mut scheduler = Scheduler::new(schedule("09:00", "17:00"));
        assert_eq!(scheduler.tick(clock(1, "08:59")), None);
        assert_eq!(scheduler.tick(clock(1, "09:00")), Some(Decision::Start));
        scheduler.manual_stop(clock(1, "10:00"));
        assert_eq!(scheduler.tick(clock(1, "10:01")), None);
        assert_eq!(scheduler.tick(clock(1, "17:00")), None);
        assert_eq!(scheduler.tick(clock(2, "09:00")), Some(Decision::Start));
        assert_eq!(scheduler.tick(clock(2, "17:00")), Some(Decision::Stop));
        assert_eq!(scheduler.tick(clock(2, "17:01")), None);
    }
    #[test]
    fn overnight_shift_is_keyed_to_start_day() {
        let shift = schedule("22:00", "06:00");
        assert_eq!(shift.active_shift(clock(4, "23:00")), Some(4));
        assert_eq!(shift.active_shift(clock(5, "05:59")), Some(4));
        assert_eq!(shift.active_shift(clock(5, "06:00")), None);
        let mut scheduler = Scheduler::new(shift);
        scheduler.manual_start(clock(4, "23:00"));
        assert_eq!(scheduler.tick(clock(5, "06:00")), Some(Decision::Stop));
    }

    #[test]
    fn selected_weekdays_skip_weekends_and_preserve_manual_stop() {
        let friday = civil_day(2026, 11, 6);
        let monday = civil_day(2026, 11, 9);
        let mut shift = schedule("09:00", "17:00");
        shift.days = [true, true, true, true, true, false, false];
        let mut scheduler = Scheduler::new(shift);
        assert_eq!(
            scheduler.tick(clock(friday, "09:00")),
            Some(Decision::Start)
        );
        scheduler.manual_stop(clock(friday, "10:00"));
        assert_eq!(scheduler.tick(clock(friday, "11:00")), None);
        assert_eq!(scheduler.tick(clock(civil_day(2026, 11, 7), "10:00")), None);
        assert_eq!(scheduler.tick(clock(civil_day(2026, 11, 8), "10:00")), None);
        assert_eq!(
            scheduler.tick(clock(monday, "09:00")),
            Some(Decision::Start)
        );
        assert_eq!(scheduler.tick(clock(monday, "17:00")), Some(Decision::Stop));
        assert_eq!(weekday(civil_day(1969, 12, 28)), 6);
        assert_eq!(weekday(0), 3);
    }

    #[test]
    fn selected_overnight_shift_finishes_on_unselected_day_and_across_week_boundary() {
        for (day, selected) in [(civil_day(2026, 11, 6), 4), (civil_day(2026, 11, 8), 6)] {
            let mut shift = schedule("22:00", "06:00");
            shift.days = [false; 7];
            shift.days[selected] = true;
            assert_eq!(shift.active_shift(clock(day + 1, "05:59")), Some(day));
            assert_eq!(shift.active_shift(clock(day + 1, "22:00")), None);
            let mut reopened = Scheduler::new(shift.clone());
            assert_eq!(
                reopened.tick(clock(day + 1, "05:59")),
                Some(Decision::Start)
            );
            assert_eq!(reopened.tick(clock(day + 1, "06:00")), Some(Decision::Stop));
            let mut scheduler = Scheduler::new(shift);
            assert_eq!(scheduler.tick(clock(day, "21:59")), None);
            assert_eq!(scheduler.tick(clock(day, "22:00")), Some(Decision::Start));
            assert_eq!(scheduler.tick(clock(day + 1, "05:59")), None);
            assert_eq!(
                scheduler.tick(clock(day + 1, "06:00")),
                Some(Decision::Stop)
            );
            assert_eq!(scheduler.tick(clock(day + 1, "22:00")), None);
            assert_eq!(
                scheduler.tick(clock(day + 7, "22:00")),
                Some(Decision::Start)
            );
        }
    }

    #[test]
    fn manual_run_waits_for_next_selected_finish_and_no_days_disables_automation() {
        let friday = civil_day(2026, 11, 6);
        let monday = civil_day(2026, 11, 9);
        let mut shift = schedule("09:00", "17:00");
        shift.days = [true, false, false, false, false, false, false];
        let mut scheduler = Scheduler::new(shift.clone());
        scheduler.manual_start(clock(friday, "10:00"));
        assert_eq!(scheduler.tick(clock(friday, "17:00")), None);
        assert_eq!(
            scheduler.tick(clock(monday, "09:00")),
            Some(Decision::Start)
        );
        assert_eq!(scheduler.tick(clock(monday, "17:00")), Some(Decision::Stop));
        shift.days = [false; 7];
        assert!(shift.validate().is_ok());
        let mut scheduler = Scheduler::new(shift);
        assert_eq!(scheduler.tick(clock(monday, "09:00")), None);
        scheduler.manual_start(clock(monday, "10:00"));
        assert_eq!(scheduler.tick(clock(monday, "17:00")), None);
    }
    #[test]
    fn manual_outside_adopts_next_shift_and_disabled_never_stops() {
        let mut scheduler = Scheduler::new(schedule("09:00", "17:00"));
        scheduler.manual_start(clock(1, "07:00"));
        assert_eq!(scheduler.tick(clock(1, "08:00")), None);
        assert_eq!(scheduler.tick(clock(1, "09:00")), Some(Decision::Start));
        assert_eq!(scheduler.tick(clock(1, "17:00")), Some(Decision::Stop));
        scheduler.manual_start(clock(2, "07:00"));
        scheduler.update(Schedule::default());
        assert_eq!(scheduler.tick(clock(2, "17:00")), None);
    }
    #[test]
    fn repeated_local_hour_does_not_restart_finished_shift() {
        let mut scheduler = Scheduler::new(schedule("01:00", "01:30"));
        assert_eq!(scheduler.tick(clock(1, "01:00")), Some(Decision::Start));
        assert_eq!(scheduler.tick(clock(1, "01:30")), Some(Decision::Stop));
        assert_eq!(scheduler.tick(clock(1, "01:00")), None);
        assert_eq!(scheduler.tick(clock(2, "01:00")), Some(Decision::Start));
    }
    #[test]
    fn sleeping_past_shift_and_clock_rollback_preserve_finish() {
        let mut scheduler = Scheduler::new(schedule("09:00", "17:00"));
        scheduler.manual_start(clock(1, "07:00"));
        assert_eq!(scheduler.tick(clock(1, "18:00")), Some(Decision::Stop));
        scheduler.manual_start(clock(2, "10:00"));
        assert_eq!(scheduler.tick(clock(2, "08:00")), None);
        assert_eq!(scheduler.tick(clock(2, "17:00")), Some(Decision::Stop));
    }
    #[test]
    fn civil_dates_include_leaps_month_and_year_boundaries() {
        assert_eq!(civil_day(1970, 1, 1), 0);
        for (a, b) in [
            ((2026, 12, 31), (2027, 1, 1)),
            ((2024, 2, 28), (2024, 2, 29)),
            ((2024, 2, 29), (2024, 3, 1)),
            ((1900, 2, 28), (1900, 3, 1)),
            ((2000, 2, 28), (2000, 2, 29)),
        ] {
            assert_eq!(civil_day(b.0, b.1, b.2) - civil_day(a.0, a.1, a.2), 1);
        }
    }
}
