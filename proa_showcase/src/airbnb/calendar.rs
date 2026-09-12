// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Showcase-owned calendar markup for the Airbnb listing fixture.
//!
//! This intentionally implements only the date-grid contract used by the
//! fixture. It keeps the calendar independent from downstream component
//! libraries while retaining the data attributes consumed by the browser
//! runtime and the showcase CSS.

use proa_core::shared::attr_value::AttrValue;
use proa_core::{DataLoader, WebContext, WebRenderSync, WriteBuf, WriteError};
use proa_macros::html_sync;

const MONTH_NAMES: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const WEEKDAYS: [&str; 7] = ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"];
const DAY_LABELS: [&str; 32] = [
    "", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16",
    "17", "18", "19", "20", "21", "22", "23", "24", "25", "26", "27", "28", "29", "30", "31",
];

const ROOT_CLASS: &str =
    "w-fit p-3 [--calendar-day-size:2.25rem] [--calendar-navigation-size:1.75rem] air-calendar";
const HEADER_CLASS: &str = "flex items-center justify-between mb-2";
const HEADING_CLASS: &str = "text-sm font-medium";
const NAVIGATION_CLASS: &str =
    "size-7 inline-flex items-center justify-center border border-input rounded-md transition-colors";
const WEEKDAYS_CLASS: &str = "grid grid-cols-7 text-muted-foreground text-xs text-center mb-1";
const WEEKDAY_CLASS: &str = "py-1";
const GRID_CLASS: &str = "grid grid-cols-7";
const DAY_CLASS: &str = "size-9 rounded-md text-sm inline-flex items-center justify-center cursor-pointer transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring";

const CELL_COUNT: u8 = 42;

/// Static date-grid renderer used by the non-React listing fallback.
#[derive(Debug, Clone, Copy)]
pub(super) struct AirbnbCalendar {
    year: u16,
    month: u8,
    range_start: Option<u8>,
    range_end: Option<u8>,
}

impl AirbnbCalendar {
    pub(super) const fn new(year: u16, month: u8) -> Self {
        Self {
            year,
            month,
            range_start: None,
            range_end: None,
        }
    }

    pub(super) const fn with_range(mut self, start: u8, end: u8) -> Self {
        self.range_start = Some(start);
        self.range_end = Some(end);
        self
    }
}

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for AirbnbCalendar {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let month = self.month.clamp(1, 12);
        let month_name = MONTH_NAMES[(month - 1) as usize];

        html_sync! {
            <div
                data-calendar
                data-scope="calendar"
                data-part="root"
                data-slot="calendar"
                data-month={month}
                data-year={self.year}
                data-mode="range"
                data-week-start="sunday"
                data-size="md"
                data-variant="plain"
                class={ROOT_CLASS}
            >
                <div
                    data-calendar-header
                    data-scope="calendar"
                    data-part="header"
                    data-slot="calendar-header"
                    class={HEADER_CLASS}
                >
                    <button
                        data-calendar-prev
                        data-calendar-nav="prev"
                        data-scope="calendar"
                        data-part="navigation"
                        data-slot="calendar-navigation"
                        aria-label="Previous month"
                        type="button"
                        class={NAVIGATION_CLASS}
                    >
                        "\u{ab}"
                    </button>
                    <span
                        data-calendar-heading
                        data-scope="calendar"
                        data-part="heading"
                        data-slot="calendar-heading"
                        aria-live="polite"
                        class={HEADING_CLASS}
                    >
                        {month_name}
                        " "
                        {self.year}
                    </span>
                    <button
                        data-calendar-next
                        data-calendar-nav="next"
                        data-scope="calendar"
                        data-part="navigation"
                        data-slot="calendar-navigation"
                        aria-label="Next month"
                        type="button"
                        class={NAVIGATION_CLASS}
                    >
                        "\u{bb}"
                    </button>
                </div>
                <div
                    data-calendar-weekdays
                    data-scope="calendar"
                    data-part="weekdays"
                    data-slot="calendar-weekdays"
                    class={WEEKDAYS_CLASS}
                >
                    {for weekday in WEEKDAYS {
                        html_sync! {
                            <span
                                data-scope="calendar"
                                data-part="weekday"
                                data-slot="calendar-weekday"
                                class={WEEKDAY_CLASS}
                            >
                                {weekday}
                            </span>
                        }
                    }}
                </div>
                <div
                    data-calendar-grid
                    data-scope="calendar"
                    data-part="grid"
                    data-slot="calendar-grid"
                    role="group"
                    aria-label="Calendar days"
                    class={GRID_CLASS}
                >
                    {for index in 0..CELL_COUNT {
                        let cell = calendar_cell(self.year, month, index);
                        let day_label = DAY_LABELS[cell.day as usize];
                        let day_attr = if cell.outside { "" } else { day_label };
                        let outside = cell.outside.then_some("true");
                        let range_start = (!cell.outside
                            && self.range_start == Some(cell.day))
                            .then_some("true");
                        let range_end = (!cell.outside
                            && self.range_end == Some(cell.day))
                            .then_some("true");
                        let in_range = (!cell.outside
                            && self.range_start.is_some_and(|start| cell.day > start)
                            && self.range_end.is_some_and(|end| cell.day < end))
                            .then_some("true");
                        let selected = range_start.is_some() && range_end.is_some();
                        let state = selected.then_some("selected");
                        let pressed = if selected
                            || range_start.is_some()
                            || range_end.is_some()
                            || in_range.is_some()
                        {
                            "true"
                        } else {
                            "false"
                        };

                        html_sync! {
                            <button
                                data-calendar-day={day_attr}
                                data-scope="calendar"
                                data-part="day"
                                data-slot="calendar-day"
                                data-date={CalendarDate::new(cell.year, cell.month, cell.day)}
                                data-outside={outside}
                                data-state={state}
                                data-range-start={range_start}
                                data-range-end={range_end}
                                data-in-range={in_range}
                                aria-pressed={pressed}
                                disabled={cell.outside}
                                type="button"
                                class={DAY_CLASS}
                            >
                                {day_label}
                            </button>
                        }
                    }}
                </div>
            </div>
        }
        .render(cx)
    }
}

#[derive(Debug, Clone, Copy)]
struct CalendarCell {
    year: u16,
    month: u8,
    day: u8,
    outside: bool,
}

fn calendar_cell(year: u16, month: u8, index: u8) -> CalendarCell {
    let offset = first_weekday_sunday(year, month);
    let current_month_days = days_in_month(year, month);

    if index < offset {
        let (previous_year, previous_month) = previous_month(year, month);
        let previous_month_days = days_in_month(previous_year, previous_month);
        return CalendarCell {
            year: previous_year,
            month: previous_month,
            day: previous_month_days - offset + index + 1,
            outside: true,
        };
    }

    let day = index - offset + 1;
    if day <= current_month_days {
        return CalendarCell {
            year,
            month,
            day,
            outside: false,
        };
    }

    let (next_year, next_month) = next_month(year, month);
    CalendarCell {
        year: next_year,
        month: next_month,
        day: day - current_month_days,
        outside: true,
    }
}

fn previous_month(year: u16, month: u8) -> (u16, u8) {
    if month == 1 {
        (year.saturating_sub(1), 12)
    } else {
        (year, month - 1)
    }
}

fn next_month(year: u16, month: u8) -> (u16, u8) {
    if month == 12 {
        (year.saturating_add(1), 1)
    } else {
        (year, month + 1)
    }
}

fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 30,
    }
}

fn is_leap_year(year: u16) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}

/// Day of week for the first day of a month, where Sunday is zero.
fn first_weekday_sunday(year: u16, month: u8) -> u8 {
    const MONTH_OFFSETS: [u32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];

    let mut adjusted_year = u32::from(year);
    if month < 3 {
        adjusted_year = adjusted_year.saturating_sub(1);
    }

    ((adjusted_year + adjusted_year / 4 - adjusted_year / 100
        + adjusted_year / 400
        + MONTH_OFFSETS[(month - 1) as usize]
        + 1)
        % 7) as u8
}

/// Stack-formatted `YYYY-MM-DD` attribute value.
struct CalendarDate([u8; 10]);

impl CalendarDate {
    fn new(year: u16, month: u8, day: u8) -> Self {
        let year = year % 10_000;
        Self([
            b'0' + (year / 1_000) as u8,
            b'0' + ((year / 100) % 10) as u8,
            b'0' + ((year / 10) % 10) as u8,
            b'0' + (year % 10) as u8,
            b'-',
            b'0' + month / 10,
            b'0' + month % 10,
            b'-',
            b'0' + day / 10,
            b'0' + day % 10,
        ])
    }
}

impl AttrValue for CalendarDate {
    #[inline(always)]
    fn should_render(&self) -> bool {
        true
    }

    #[inline(always)]
    fn render_attr_value<B: WriteBuf, L: DataLoader>(
        &self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), WriteError> {
        cx.out.extend(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proa_core::ctx::Ctx;

    fn render_calendar(calendar: AirbnbCalendar) -> String {
        let mut output = Vec::new();
        let mut cx = Ctx::with_buffer(&mut output);
        calendar.render(&mut cx).unwrap();
        String::from_utf8(output).unwrap()
    }

    fn button_for_date<'a>(html: &'a str, date: &str) -> &'a str {
        let date_marker = format!("data-date=\"{date}\"");
        let date_index = html.find(&date_marker).expect("date cell");
        let start = html[..date_index].rfind("<button").expect("button start");
        let end = html[date_index..]
            .find("</button>")
            .map(|offset| date_index + offset + "</button>".len())
            .expect("button end");
        &html[start..end]
    }

    #[test]
    fn renders_a_stable_six_week_grid() {
        let html = render_calendar(AirbnbCalendar::new(2026, 4));

        assert_eq!(html.matches("data-calendar-day=").count(), 42);
        assert!(html.contains("data-date=\"2026-03-29\""));
        assert!(html.contains("data-date=\"2026-05-09\""));
    }

    #[test]
    fn marks_the_initial_booking_range() {
        let html = render_calendar(AirbnbCalendar::new(2026, 3).with_range(18, 25));

        assert!(button_for_date(&html, "2026-03-18").contains("data-range-start=\"true\""));
        assert!(button_for_date(&html, "2026-03-25").contains("data-range-end=\"true\""));
        assert!(button_for_date(&html, "2026-03-21").contains("data-in-range=\"true\""));
        assert!(!button_for_date(&html, "2026-03-17").contains("data-in-range"));
    }
}
