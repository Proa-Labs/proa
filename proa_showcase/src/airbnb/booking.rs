// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Booking controls for the two months displayed by the listing fixture.

use super::listing::{ListingDetails, ListingOverview, MALIBU};
use proa_core::{DataLoader, WebContext, WebRenderSync, WriteBuf, WriteError};
use rsjs::{event_handler, rsjs};

const MARCH_DAYS: u32 = 31;
const APRIL_DAYS: u32 = 30;

#[derive(rsjs::SharedState)]
#[rsjs(type_key = "proa.showcase.airbnb.booking")]
struct BookingState {
    check_in: u32,
    check_out: u32,
    choosing_checkout: bool,
}

pub(super) struct BookingSection {}

#[rsjs(component)]
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for BookingSection {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let data = &MALIBU;
        let check_in = calendar_day(data.booking.check_in);
        let check_out = calendar_day(data.booking.checkout);
        let check_out = if check_in > 0 && check_out > check_in {
            check_out
        } else {
            0
        };
        proa_macros::html_sync! {
            <div class="air-shell py-10">
                {rsjs::SharedStateProvider {
                    state: BookingState {
                        check_in,
                        check_out,
                        choosing_checkout: check_in > 0 && check_out == 0,
                    },
                    children: BookingColumns {},
                }}
            </div>
        }
        .render(cx)
    }
}

struct BookingColumns {}

#[rsjs(component)]
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for BookingColumns {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let data = &MALIBU;
        proa_macros::html_sync! {
            <div class="grid grid-cols-1 gap-y-12 lg:grid-cols-12 lg:gap-x-16">
                <div class="lg:col-span-7">
                    {ListingOverview(data)}
                    {DatePicker {}}
                    {ListingDetails(data)}
                </div>
                <aside class="lg:col-span-4 lg:col-start-9">
                    {BookingCard {
                        guests: data.booking.guests,
                        free_cancel_date: data.booking.free_cancel_date,
                        rating: data.rating,
                        review_count: data.review_count,
                    }}
                </aside>
            </div>
        }
        .render(cx)
    }
}

// Both month grids use the same day scale for range comparisons and pricing.
fn calendar_day(iso: &str) -> u32 {
    let Some((month, day)) = iso.rsplit_once('-') else {
        return 0;
    };
    let Ok(day) = day.parse::<u32>() else {
        return 0;
    };
    match month {
        "2026-03" if (1..=MARCH_DAYS).contains(&day) => day,
        "2026-04" if (1..=APRIL_DAYS).contains(&day) => MARCH_DAYS + day,
        _ => 0,
    }
}

#[rsjs::client]
fn on_or_before(day: u32, other: u32) -> bool {
    day <= other
}

#[rsjs::client]
fn is_endpoint(day: u32, endpoint: u32) -> bool {
    day == endpoint
}

#[rsjs::client]
fn inside_range(day: u32, start: u32, end: u32) -> bool {
    day > start && day < end
}

#[rsjs::client]
fn stay_length(start: u32, end: u32) -> u32 {
    if end > start {
        end - start
    } else {
        0
    }
}

#[rsjs::client]
fn room_cost(start: u32, end: u32, rate: u32) -> u32 {
    stay_length(start, end) * rate
}

#[rsjs::client]
fn stay_total(start: u32, end: u32, rate: u32, cleaning: u32, service: u32) -> u32 {
    if end > start {
        room_cost(start, end, rate) + cleaning + service
    } else {
        0
    }
}

struct DatePicker {}

#[rsjs(component, client)]
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for DatePicker {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let booking = rsjs::shared::<BookingState>();
        let check_in = booking.check_in();
        let check_out = booking.check_out();
        let choosing_checkout = booking.choosing_checkout();
        let clear = event_handler(|_: rsjs::Event| {
            check_in.set(0_u32);
            check_out.set(0_u32);
            choosing_checkout.set(false);
        });
        proa_macros::html_sync! {
            <section data-listing-date-picker class="py-8">
                <div class="mb-6 flex items-end justify-between gap-4">
                    <div>
                        <h2 class="text-[24px] font-semibold tracking-[-0.03em] text-[#222]">
                            "Select dates"
                        </h2>
                        <p class="mt-2 text-[15px] text-[var(--air-muted)]">
                            "Add your travel dates for exact pricing"
                        </p>
                    </div>
                    <button
                        type="button"
                        class="text-sm font-semibold underline underline-offset-4"
                        data-booking-clear-all
                        hidden={check_in.get() == 0}
                        on_click={clear()}
                    >
                        "Clear dates"
                    </button>
                </div>
                <div class="grid gap-5 md:grid-cols-2">
                    <div class="rounded-[22px] border border-[var(--air-line)] p-5">
                        {MonthGrid { april: false }}
                    </div>
                    <div class="rounded-[22px] border border-[var(--air-line)] p-5">
                        {MonthGrid { april: true }}
                    </div>
                </div>
            </section>
        }
        .render(cx)
    }
}

struct MonthGrid {
    april: bool,
}

#[rsjs(component, client)]
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for MonthGrid {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let booking = rsjs::shared::<BookingState>();
        let check_in = booking.check_in();
        let check_out = booking.check_out();
        let choosing_checkout = booking.choosing_checkout();
        let choose = event_handler(|_: rsjs::Event, date: u32| {
            if check_in.get() == 0 || !choosing_checkout.get() || on_or_before(date, check_in.get())
            {
                check_in.set(date);
                check_out.set(0_u32);
                choosing_checkout.set(true);
            } else {
                check_out.set(date);
                choosing_checkout.set(false);
            }
        });
        let month = if self.april { 4 } else { 3 };
        let offset = if self.april { MARCH_DAYS } else { 0 };
        let days = if self.april { APRIL_DAYS } else { MARCH_DAYS };
        let heading = if self.april {
            "April 2026"
        } else {
            "March 2026"
        };
        proa_macros::html_sync! {
            <div
                data-listing-calendar
                data-calendar
                data-month={month}
                data-year="2026"
                class="air-calendar"
            >
                <h3 data-calendar-heading class="text-center text-sm font-semibold">{heading}</h3>
                <div
                    data-calendar-weekdays
                    class="grid grid-cols-7 text-center text-xs"
                    aria-hidden="true"
                >
                    {for weekday in ["Su","Mo","Tu","We","Th","Fr","Sa"] { proa_macros::html_sync! {
                        <span>{weekday}</span>
                    } }}
                </div>
                <div data-calendar-grid class="grid grid-cols-7">
                    {for day in 1..=days {
                        let date = offset + day;
                        proa_macros::html_sync! {
                            <button type="button" class="size-9 rounded-md text-sm" data-calendar-day={day}
                                aria-label={text!("{}/{}/2026", month, day)}
                                aria-pressed={is_endpoint(date as u32, check_in.get()) || is_endpoint(date as u32, check_out.get())}
                                data-range-start={is_endpoint(date as u32, check_in.get())}
                                data-range-end={is_endpoint(date as u32, check_out.get())}
                                data-in-range={inside_range(date as u32, check_in.get(), check_out.get())}
                                on_click={choose(date as u32)}>{day}</button>
                        }
                    }}
                </div>
            </div>
        }.render(cx)
    }
}

struct SelectedDate {
    checkout: bool,
}

#[rsjs(component, client)]
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for SelectedDate {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let booking = rsjs::shared::<BookingState>();
        let check_in = booking.check_in();
        let check_out = booking.check_out();
        let checkout = rsjs::signal::<bool>(self.checkout);
        // Ordinals 1–31 are March; 32–61 are April.
        let month_index = rsjs::memo(|| {
            rsjs::div_int::<u32>(
                if checkout.get() {
                    check_out.get()
                } else {
                    check_in.get()
                },
                32,
            )
        });
        let date = rsjs::memo(|| {
            if checkout.get() {
                check_out.get()
            } else {
                check_in.get()
            }
        });
        proa_macros::html_sync! {
            <span>
                <span hidden={date.get() != 0}>"Add date"</span>
                <span hidden={date.get() == 0}>
                    {if month_index.get() == 0 { 3 } else { 4 }}
                    "/"
                    {if month_index.get() == 0 { date.get() } else { date.get() - 31_u32 }}
                    "/2026"
                </span>
            </span>
        }
        .render(cx)
    }
}

struct BookingCard<'a> {
    guests: &'a str,
    free_cancel_date: &'a str,
    rating: &'a str,
    review_count: u32,
}

#[rsjs(component, client)]
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for BookingCard<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let booking = rsjs::shared::<BookingState>();
        let check_in = booking.check_in();
        let check_out = booking.check_out();
        let choosing_checkout = booking.choosing_checkout();
        let rate = rsjs::signal::<u32>(MALIBU.nightly_rate);
        let cleaning = rsjs::signal::<u32>(MALIBU.cleaning_fee);
        let service = rsjs::signal::<u32>(MALIBU.service_fee);
        let edit_check_in = event_handler(|_: rsjs::Event| choosing_checkout.set(false));
        let edit_check_out = event_handler(|_: rsjs::Event| choosing_checkout.set(true));
        let clear_all = event_handler(|_: rsjs::Event| {
            check_in.set(0_u32);
            check_out.set(0_u32);
            choosing_checkout.set(false);
        });
        let clear_checkout = event_handler(|_: rsjs::Event| {
            check_out.set(0_u32);
            choosing_checkout.set(true);
        });
        proa_macros::html_sync! {
            <div data-airbnb-booking-root class="sticky top-28">
                <div class="mb-4 flex justify-end">
                    <div class="airbnb-booking-pill">"Prices include all fees"</div>
                </div>
                <div class="air-booking-card airbnb-booking-card p-6">
                    <div class="flex flex-wrap items-baseline gap-2">
                        <strong data-booking-header-total class="text-[32px]">
                            "$"
                            {if check_out.get() > 0 { stay_total(check_in.get(), check_out.get(), rate.get(), cleaning.get(), service.get()) } else { rate.get() }}
                        </strong>
                        <span hidden={check_out.get() == 0}>
                            "for "
                            {stay_length(check_in.get(), check_out.get())}
                            {if stay_length(check_in.get(), check_out.get()) == 1 { " night" } else { " nights" }}
                        </span>
                        <span hidden={check_out.get() > 0}>"add dates for total"</span>
                    </div>
                    <p class="mb-4 text-sm">
                        "★ "
                        {self.rating}
                        " · "
                        <a class="underline" href="#reviews">{self.review_count}" reviews"</a>
                    </p>
                    <div class="grid grid-cols-2 rounded-xl border border-[var(--air-line)]">
                        <button
                            type="button"
                            data-booking-trigger="checkIn"
                            class="p-3 text-left"
                            popovertarget="booking-dates"
                            popovertargetaction="show"
                            on_click={edit_check_in()}
                        >
                            <strong class="block text-xs">"CHECK-IN"</strong>
                            <span data-booking-display="checkIn">
                                {SelectedDate { checkout: false }}
                            </span>
                        </button>
                        <button
                            type="button"
                            data-booking-trigger="checkOut"
                            class="p-3 text-left"
                            popovertarget="booking-dates"
                            popovertargetaction="show"
                            on_click={edit_check_out()}
                        >
                            <strong class="block text-xs">"CHECKOUT"</strong>
                            <span data-booking-display="checkOut">
                                {SelectedDate { checkout: true }}
                            </span>
                        </button>
                        <div class="col-span-2 border-t p-3">
                            <strong class="block text-xs">"GUESTS"</strong>
                            {self.guests}
                        </div>
                    </div>
                    <p class="my-4 rounded-xl bg-[var(--air-soft)] px-4 py-2 text-center">
                        "Free cancellation before "
                        {self.free_cancel_date}
                    </p>
                    <button
                        type="button"
                        class="w-full rounded-xl bg-[#E31C5F] py-3 font-semibold text-white"
                    >
                        "Reserve"
                    </button>
                    <p class="my-4 text-center text-sm">"You won't be charged yet"</p>
                    <div class="flex justify-between">
                        <span hidden={check_out.get() == 0}>
                            "$"
                            {rate.get()}
                            " x "
                            {stay_length(check_in.get(), check_out.get())}
                            {if stay_length(check_in.get(), check_out.get()) == 1 { " night" } else { " nights" }}
                        </span>
                        <span hidden={check_out.get() > 0}>"Select dates"</span>
                        <span>"$"{room_cost(check_in.get(), check_out.get(), rate.get())}</span>
                    </div>
                    <div class="flex justify-between">
                        <span>"Cleaning fee"</span>
                        <span>"$"{cleaning.get()}</span>
                    </div>
                    <div class="flex justify-between">
                        <span>"Airbnb service fee"</span>
                        <span>"$"{service.get()}</span>
                    </div>
                    <hr class="air-divider" />
                    <div class="flex justify-between font-semibold">
                        <span>"Total before taxes"</span>
                        <span data-booking-total-value>
                            "$"
                            {stay_total(check_in.get(), check_out.get(), rate.get(), cleaning.get(), service.get())}
                        </span>
                    </div>
                </div>
                <div
                    id="booking-dates"
                    popover="auto"
                    data-booking-popover
                    class="airbnb-booking-popover"
                >
                    <div class="airbnb-popover-shell">
                        <div class="airbnb-popover-header">
                            <div class="airbnb-popover-summary">
                                <h3 hidden={check_out.get() > 0}>"Select dates"</h3>
                                <h3 hidden={check_out.get() == 0}>
                                    {stay_length(check_in.get(), check_out.get())}
                                    {if stay_length(check_in.get(), check_out.get()) == 1 { " night" } else { " nights" }}
                                </h3>
                                <p hidden={check_out.get() > 0}>"Select your travel dates"</p>
                                <p hidden={check_out.get() == 0}>
                                    {SelectedDate { checkout: false }}
                                    " – "
                                    {SelectedDate { checkout: true }}
                                </p>
                            </div>
                            <div class="airbnb-popover-fields">
                                <div class="airbnb-popover-field">
                                    <button type="button" on_click={edit_check_in()}>
                                        <span class="airbnb-popover-field-label">"Check-in"</span>
                                        {SelectedDate { checkout: false }}
                                    </button>
                                    <button
                                        type="button"
                                        data-booking-clear-field="checkIn"
                                        class="airbnb-popover-clear"
                                        hidden={check_in.get() == 0}
                                        aria-label="Clear dates"
                                        on_click={clear_all()}
                                    >
                                        "×"
                                    </button>
                                </div>
                                <div class="airbnb-popover-field">
                                    <button type="button" on_click={edit_check_out()}>
                                        <span class="airbnb-popover-field-label">"Checkout"</span>
                                        {SelectedDate { checkout: true }}
                                    </button>
                                    <button
                                        type="button"
                                        data-booking-clear-field="checkOut"
                                        class="airbnb-popover-clear"
                                        hidden={check_out.get() == 0}
                                        aria-label="Clear checkout date"
                                        on_click={clear_checkout()}
                                    >
                                        "×"
                                    </button>
                                </div>
                            </div>
                        </div>
                        <div class="airbnb-popover-calendars">
                            <div class="airbnb-popover-calendar">{MonthGrid { april: false }}</div>
                            <div class="airbnb-popover-calendar">{MonthGrid { april: true }}</div>
                        </div>
                        <div class="airbnb-popover-footer">
                            <button
                                type="button"
                                class="airbnb-link-button"
                                data-booking-clear-all
                                on_click={clear_all()}
                            >
                                "Clear dates"
                            </button>
                            <button
                                type="button"
                                class="airbnb-close-button"
                                data-booking-close
                                popovertarget="booking-dates"
                                popovertargetaction="hide"
                            >
                                "Close"
                            </button>
                        </div>
                    </div>
                </div>
                <button type="button" class="mt-6 text-sm font-semibold underline">
                    "Report this listing"
                </button>
            </div>
        }
        .render(cx)
    }
}
