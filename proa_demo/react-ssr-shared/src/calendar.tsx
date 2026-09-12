// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Calendar component — React equivalent of proa_ui::components::calendar::Calendar
//
// Produces HTML with the same structure, data attributes, and class strings
// as the Proa server-rendered calendar component.

import React from "react";

// ---------------------------------------------------------------------------
// Props
// ---------------------------------------------------------------------------

interface CalendarProps {
  month: number; // 1-12
  year: number;
  mode?: "single" | "range";
  weekStart?: "monday" | "sunday";
  selected?: number;
  today?: number;
  disabledBefore?: number;
  rangeStart?: number;
  rangeEnd?: number;
  className?: string;
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const MONTH_NAMES = [
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

const WEEKDAY_HEADERS_MONDAY = ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"];
const WEEKDAY_HEADERS_SUNDAY = ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"];

// ---------------------------------------------------------------------------
// Class constants (must match recipe.rs exactly)
// ---------------------------------------------------------------------------

const CALENDAR_BASE = "w-fit p-3";
const CALENDAR_HEADER = "flex items-center justify-between mb-2";
const CALENDAR_HEADING = "text-sm font-medium";
const CALENDAR_WEEKDAYS =
  "grid grid-cols-7 text-muted-foreground text-xs text-center mb-1";
const CALENDAR_GRID = "grid grid-cols-7";
const CALENDAR_DAY =
  "size-9 rounded-md text-sm inline-flex items-center justify-center cursor-pointer hover:bg-accent hover:text-accent-foreground transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring";
const CALENDAR_DAY_SELECTED =
  "bg-primary text-primary-foreground hover:bg-primary/90";
const CALENDAR_DAY_TODAY = "bg-accent text-accent-foreground font-semibold";
const CALENDAR_DAY_OUTSIDE = "text-muted-foreground opacity-50";
const CALENDAR_DAY_DISABLED =
  "text-muted-foreground opacity-50 cursor-not-allowed hover:bg-transparent";
const CALENDAR_NAV_BUTTON =
  "size-7 inline-flex items-center justify-center border border-input rounded-md hover:bg-accent hover:text-accent-foreground transition-colors";
const CALENDAR_DAY_RANGE_START =
  "bg-primary text-primary-foreground rounded-r-none";
const CALENDAR_DAY_RANGE_END =
  "bg-primary text-primary-foreground rounded-l-none";
const CALENDAR_DAY_IN_RANGE =
  "bg-accent text-accent-foreground rounded-none";

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/** Returns the number of days in a given month (1-indexed), accounting for leap years. */
function daysInMonth(year: number, month: number): number {
  switch (month) {
    case 1:
    case 3:
    case 5:
    case 7:
    case 8:
    case 10:
    case 12:
      return 31;
    case 4:
    case 6:
    case 9:
    case 11:
      return 30;
    case 2:
      return (year % 4 === 0 && year % 100 !== 0) || year % 400 === 0
        ? 29
        : 28;
    default:
      return 30;
  }
}

/**
 * Tomohiko Sakamoto algorithm.
 * Returns 0=Mon, 1=Tue, ..., 6=Sun.
 */
function dayOfWeek(year: number, month: number, day: number): number {
  const t = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
  let y = year;
  if (month < 3) {
    y -= 1;
  }
  const dow =
    (y + Math.floor(y / 4) - Math.floor(y / 100) + Math.floor(y / 400) +
      t[month - 1] +
      day) %
    7;
  // dow: 0=Sun, 1=Mon, ..., 6=Sat  ->  convert to 0=Mon, ..., 6=Sun
  return (dow + 6) % 7;
}

/** Format a date as YYYY-MM-DD. */
function formatDate(year: number, month: number, day: number): string {
  const yy = String(year).padStart(4, "0");
  const mm = String(month).padStart(2, "0");
  const dd = String(day).padStart(2, "0");
  return `${yy}-${mm}-${dd}`;
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

export function Calendar({
  month: rawMonth,
  year,
  mode = "single",
  weekStart = "monday",
  selected,
  today,
  disabledBefore,
  rangeStart,
  rangeEnd,
  className,
}: CalendarProps): React.JSX.Element {
  const month = Math.max(1, Math.min(12, rawMonth));
  const monthName = MONTH_NAMES[month - 1];
  const dim = daysInMonth(year, month);

  // Day-of-week for the 1st of the month (0=Mon..6=Sun)
  const firstDowMonday = dayOfWeek(year, month, 1);
  const firstDow =
    weekStart === "monday" ? firstDowMonday : (firstDowMonday + 1) % 7;

  const weekdayHeaders =
    weekStart === "monday" ? WEEKDAY_HEADERS_MONDAY : WEEKDAY_HEADERS_SUNDAY;

  // Previous month info for leading days
  const prevYear = month === 1 ? year - 1 : year;
  const prevMonth = month === 1 ? 12 : month - 1;
  const prevDim = daysInMonth(prevYear, prevMonth);

  // Trailing days
  const totalCells = firstDow + dim;
  const rows = totalCells > 35 ? 42 : 35;
  const trailing = rows - totalCells;
  const nextYear = month === 12 ? year + 1 : year;
  const nextMonth = month === 12 ? 1 : month + 1;

  // Root class
  const rootClass = className
    ? `${CALENDAR_BASE} ${className}`
    : CALENDAR_BASE;

  const outsideClasses = `${CALENDAR_DAY} ${CALENDAR_DAY_OUTSIDE}`;

  // Build leading day buttons
  const leadingDays: React.JSX.Element[] = [];
  for (let i = 0; i < firstDow; i++) {
    const d = prevDim - firstDow + i + 1;
    const dateStr = formatDate(prevYear, prevMonth, d);
    leadingDays.push(
      <button
        key={`lead-${d}`}
        data-calendar-day=""
        data-date={dateStr}
        data-outside=""
        type="button"
        disabled
        className={outsideClasses}
      >
        {d}
      </button>
    );
  }

  // Build current month day buttons
  const currentDays: React.JSX.Element[] = [];
  for (let d = 1; d <= dim; d++) {
    const dateStr = formatDate(year, month, d);

    const isRangeStart = rangeStart === d;
    const isRangeEnd = rangeEnd === d;
    const isInRange =
      rangeStart !== undefined &&
      rangeEnd !== undefined &&
      d > rangeStart &&
      d < rangeEnd;
    const isSelected =
      selected === d || (isRangeStart && isRangeEnd);
    const isToday = today === d;
    const isDisabled =
      disabledBefore !== undefined && d < disabledBefore;

    // data-state: only present when selected
    const stateAttr: Record<string, string> = isSelected
      ? { "data-state": "selected" }
      : {};

    // Determine the state-specific class addition
    let stateClass: string | undefined;
    if (isSelected) {
      stateClass = CALENDAR_DAY_SELECTED;
    } else if (isRangeStart) {
      stateClass = CALENDAR_DAY_RANGE_START;
    } else if (isRangeEnd) {
      stateClass = CALENDAR_DAY_RANGE_END;
    } else if (isInRange) {
      stateClass = CALENDAR_DAY_IN_RANGE;
    } else if (isToday) {
      stateClass = CALENDAR_DAY_TODAY;
    }

    let dayClasses = CALENDAR_DAY;
    if (stateClass) {
      dayClasses += ` ${stateClass}`;
    }
    if (isDisabled) {
      dayClasses += ` ${CALENDAR_DAY_DISABLED}`;
    }

    currentDays.push(
      <button
        key={`day-${d}`}
        data-calendar-day={String(d)}
        data-date={dateStr}
        {...stateAttr}
        {...(isRangeStart ? { "data-range-start": "" } : {})}
        {...(isRangeEnd ? { "data-range-end": "" } : {})}
        {...(isInRange ? { "data-in-range": "" } : {})}
        {...(isToday ? { "data-today": "" } : {})}
        disabled={isDisabled || undefined}
        type="button"
        className={dayClasses}
      >
        {d}
      </button>
    );
  }

  // Build trailing day buttons
  const trailingDays: React.JSX.Element[] = [];
  for (let d = 1; d <= trailing; d++) {
    const dateStr = formatDate(nextYear, nextMonth, d);
    trailingDays.push(
      <button
        key={`trail-${d}`}
        data-calendar-day=""
        data-date={dateStr}
        data-outside=""
        type="button"
        disabled
        className={outsideClasses}
      >
        {d}
      </button>
    );
  }

  return (
    <div
      data-calendar=""
      data-month={String(month)}
      data-year={String(year)}
      data-mode={mode}
      data-week-start={weekStart}
      className={rootClass}
    >
      {/* Header */}
      <div data-calendar-header="" className={CALENDAR_HEADER}>
        <button
          data-calendar-prev=""
          type="button"
          className={CALENDAR_NAV_BUTTON}
        >
          {"\u00ab"}
        </button>
        <span data-calendar-heading="" className={CALENDAR_HEADING}>
          {monthName} {String(year)}
        </span>
        <button
          data-calendar-next=""
          type="button"
          className={CALENDAR_NAV_BUTTON}
        >
          {"\u00bb"}
        </button>
      </div>

      {/* Weekday headers */}
      <div data-calendar-weekdays="" className={CALENDAR_WEEKDAYS}>
        {weekdayHeaders.map((hdr) => (
          <span key={hdr} className="py-1">
            {hdr}
          </span>
        ))}
      </div>

      {/* Day grid */}
      <div data-calendar-grid="" role="grid" className={CALENDAR_GRID}>
        {leadingDays}
        {currentDays}
        {trailingDays}
      </div>
    </div>
  );
}

export default Calendar;
