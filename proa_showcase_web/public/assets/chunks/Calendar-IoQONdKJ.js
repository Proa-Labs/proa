import { useCallback, useRef, useSyncExternalStore } from "react";
import { jsx, jsxs } from "react/jsx-runtime";
//#region node_modules/.pnpm/nanostores@0.11.4/node_modules/nanostores/atom/index.js
let listenerQueue = [];
let lqIndex = 0;
const QUEUE_ITEMS_PER_LISTENER = 4;
let epoch = 0;
let atom = (initialValue) => {
	let listeners = [];
	let $atom = {
		get() {
			if (!$atom.lc) $atom.listen(() => {})();
			return $atom.value;
		},
		lc: 0,
		listen(listener) {
			$atom.lc = listeners.push(listener);
			return () => {
				for (let i = lqIndex + QUEUE_ITEMS_PER_LISTENER; i < listenerQueue.length;) if (listenerQueue[i] === listener) listenerQueue.splice(i, QUEUE_ITEMS_PER_LISTENER);
				else i += QUEUE_ITEMS_PER_LISTENER;
				let index = listeners.indexOf(listener);
				if (~index) {
					listeners.splice(index, 1);
					if (!--$atom.lc) $atom.off();
				}
			};
		},
		notify(oldValue, changedKey) {
			epoch++;
			let runListenerQueue = !listenerQueue.length;
			for (let listener of listeners) listenerQueue.push(listener, $atom.value, oldValue, changedKey);
			if (runListenerQueue) {
				for (lqIndex = 0; lqIndex < listenerQueue.length; lqIndex += QUEUE_ITEMS_PER_LISTENER) listenerQueue[lqIndex](listenerQueue[lqIndex + 1], listenerQueue[lqIndex + 2], listenerQueue[lqIndex + 3]);
				listenerQueue.length = 0;
			}
		},
		off() {},
		set(newValue) {
			let oldValue = $atom.value;
			if (oldValue !== newValue) {
				$atom.value = newValue;
				$atom.notify(oldValue);
			}
		},
		subscribe(listener) {
			let unbind = $atom.listen(listener);
			listener($atom.value);
			return unbind;
		},
		value: initialValue
	};
	return $atom;
};
//#endregion
//#region node_modules/.pnpm/nanostores@0.11.4/node_modules/nanostores/listen-keys/index.js
function listenKeys($store, keys, listener) {
	let keysSet = new Set(keys).add(void 0);
	return $store.listen((value, oldValue, changed) => {
		if (keysSet.has(changed)) listener(value, oldValue, changed);
	});
}
//#endregion
//#region node_modules/.pnpm/@nanostores+react@0.8.4_nanostores@0.11.4_react@19.2.4/node_modules/@nanostores/react/index.js
let emit = (snapshotRef, onChange) => (value) => {
	if (snapshotRef.current === value) return;
	snapshotRef.current = value;
	onChange();
};
function useStore(store, { keys, deps = [store, keys] } = {}) {
	let snapshotRef = useRef();
	snapshotRef.current = store.get();
	let subscribe = useCallback((onChange) => {
		emit(snapshotRef, onChange)(store.value);
		return keys?.length > 0 ? listenKeys(store, keys, emit(snapshotRef, onChange)) : store.listen(emit(snapshotRef, onChange));
	}, deps);
	let get = () => snapshotRef.current;
	return useSyncExternalStore(subscribe, get, get);
}
//#endregion
//#region islands/booking-store.ts
let initialisedBookingKey = null;
const $booking = atom({
	checkIn: null,
	checkOut: null,
	activeField: "checkIn",
	popoverOpen: false
});
/** Initialise from server-rendered defaults (called once per island mount). */
function initBooking(bookingKey, checkIn, checkOut) {
	if (initialisedBookingKey === bookingKey) return;
	initialisedBookingKey = bookingKey;
	$booking.set({
		checkIn,
		checkOut,
		activeField: "checkIn",
		popoverOpen: false
	});
}
/** Core range-selection state machine — mirrors the logic in the old vanilla JS. */
function selectDate(date, keepPopoverOpen) {
	const state = $booking.get();
	if (!state.checkIn || state.activeField === "checkIn" && state.checkOut !== null) {
		$booking.set({
			checkIn: date,
			checkOut: null,
			activeField: "checkOut",
			popoverOpen: keepPopoverOpen
		});
		return;
	}
	if (state.activeField === "checkOut") {
		if (date <= state.checkIn) {
			$booking.set({
				checkIn: date,
				checkOut: null,
				activeField: "checkOut",
				popoverOpen: keepPopoverOpen
			});
			return;
		}
		$booking.set({
			...state,
			checkOut: date,
			activeField: "checkIn",
			popoverOpen: keepPopoverOpen
		});
		return;
	}
	$booking.set({
		checkIn: date,
		checkOut: null,
		activeField: "checkOut",
		popoverOpen: keepPopoverOpen
	});
}
function clearDates() {
	$booking.set({
		...$booking.get(),
		checkIn: null,
		checkOut: null,
		activeField: "checkIn",
		popoverOpen: true
	});
}
function clearField(field) {
	const state = $booking.get();
	if (field === "checkOut") $booking.set({
		...state,
		checkOut: null,
		activeField: "checkOut",
		popoverOpen: true
	});
	else $booking.set({
		...state,
		checkIn: null,
		checkOut: null,
		activeField: "checkIn",
		popoverOpen: true
	});
}
function setActiveField(field) {
	$booking.set({
		...$booking.get(),
		activeField: field,
		popoverOpen: true
	});
}
function closePopover() {
	const state = $booking.get();
	if (state.popoverOpen) $booking.set({
		...state,
		popoverOpen: false
	});
}
function parseIso(value) {
	if (!value) return null;
	const [y, m, d] = value.split("-").map(Number);
	if (!y || !m || !d) return null;
	return new Date(Date.UTC(y, m - 1, d));
}
function formatShortDate(value) {
	const date = parseIso(value);
	if (!date) return "Add date";
	return new Intl.DateTimeFormat("en-US", {
		month: "numeric",
		day: "numeric",
		year: "numeric",
		timeZone: "UTC"
	}).format(date);
}
function formatLongDate(value) {
	const date = parseIso(value);
	if (!date) return "Add date";
	return new Intl.DateTimeFormat("en-US", {
		month: "short",
		day: "numeric",
		year: "numeric",
		timeZone: "UTC"
	}).format(date);
}
function nightsBetween(checkIn, checkOut) {
	const a = parseIso(checkIn);
	const b = parseIso(checkOut);
	if (!a || !b) return 0;
	return Math.max(0, Math.round((b.getTime() - a.getTime()) / 864e5));
}
//#endregion
//#region islands/Calendar.tsx
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
	"December"
];
const WEEKDAY_HEADERS = [
	"Su",
	"Mo",
	"Tu",
	"We",
	"Th",
	"Fr",
	"Sa"
];
function daysInMonth(year, month) {
	return new Date(year, month, 0).getDate();
}
/** Sunday = 0 */
function firstDayOfMonth(year, month) {
	return new Date(year, month - 1, 1).getDay();
}
function formatDate(year, month, day) {
	return `${year}-${String(month).padStart(2, "0")}-${String(day).padStart(2, "0")}`;
}
function Calendar({ month, year, className, inPopover }) {
	const booking = useStore($booking);
	const monthName = MONTH_NAMES[month - 1];
	const dim = daysInMonth(year, month);
	const firstDow = firstDayOfMonth(year, month);
	const prevMonth = month === 1 ? 12 : month - 1;
	const prevYear = month === 1 ? year - 1 : year;
	const prevDim = daysInMonth(prevYear, prevMonth);
	const nextMonth = month === 12 ? 1 : month + 1;
	const nextYear = month === 12 ? year + 1 : year;
	const totalCells = firstDow + dim;
	const trailing = (totalCells > 35 ? 42 : 35) - totalCells;
	function dayState(dateStr) {
		const { checkIn, checkOut } = booking;
		if (checkIn && checkOut) {
			if (dateStr === checkIn && dateStr === checkOut) return "selected";
			if (dateStr === checkIn) return "range-start";
			if (dateStr === checkOut) return "range-end";
			if (dateStr > checkIn && dateStr < checkOut) return "in-range";
		} else if (checkIn && dateStr === checkIn) return "selected";
		return null;
	}
	function handleClick(e, dateStr) {
		e.stopPropagation();
		selectDate(dateStr, !!inPopover || booking.popoverOpen);
	}
	return /* @__PURE__ */ jsxs("div", {
		"data-calendar": "",
		"data-react-managed-calendar": "",
		"data-month": month,
		"data-year": year,
		"data-mode": "range",
		"data-week-start": "sunday",
		className: `air-calendar ${className ?? ""}`,
		children: [
			/* @__PURE__ */ jsx("div", {
				"data-calendar-header": "",
				children: /* @__PURE__ */ jsxs("span", {
					"data-calendar-heading": "",
					children: [
						monthName,
						" ",
						year
					]
				})
			}),
			/* @__PURE__ */ jsx("div", {
				"data-calendar-weekdays": "",
				children: WEEKDAY_HEADERS.map((d) => /* @__PURE__ */ jsx("span", { children: d }, d))
			}),
			/* @__PURE__ */ jsxs("div", {
				"data-calendar-grid": "",
				role: "grid",
				children: [
					Array.from({ length: firstDow }, (_, i) => {
						const d = prevDim - firstDow + i + 1;
						const dateStr = formatDate(prevYear, prevMonth, d);
						return /* @__PURE__ */ jsx("button", {
							"data-calendar-day": "",
							"data-date": dateStr,
							"data-outside": "",
							type: "button",
							disabled: true,
							children: d
						}, `p${d}`);
					}),
					Array.from({ length: dim }, (_, i) => {
						const d = i + 1;
						const dateStr = formatDate(year, month, d);
						const state = dayState(dateStr);
						return /* @__PURE__ */ jsx("button", {
							"data-calendar-day": String(d),
							"data-date": dateStr,
							"data-state": state === "selected" ? "selected" : void 0,
							"data-range-start": state === "range-start" ? "" : void 0,
							"data-range-end": state === "range-end" ? "" : void 0,
							"data-in-range": state === "in-range" ? "" : void 0,
							type: "button",
							onClick: (e) => handleClick(e, dateStr),
							children: d
						}, d);
					}),
					Array.from({ length: trailing }, (_, i) => {
						const d = i + 1;
						const dateStr = formatDate(nextYear, nextMonth, d);
						return /* @__PURE__ */ jsx("button", {
							"data-calendar-day": "",
							"data-date": dateStr,
							"data-outside": "",
							type: "button",
							disabled: true,
							children: d
						}, `n${d}`);
					})
				]
			})
		]
	});
}
//#endregion
export { closePopover as a, initBooking as c, useStore as d, clearField as i, nightsBetween as l, $booking as n, formatLongDate as o, clearDates as r, formatShortDate as s, Calendar as t, setActiveField as u };
