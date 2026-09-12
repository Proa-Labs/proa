import { c as initBooking, d as useStore, n as $booking, r as clearDates, t as Calendar } from "./chunks/Calendar-IoQONdKJ.js";
import { useEffect, useRef } from "react";
import { jsx, jsxs } from "react/jsx-runtime";
//#region islands/DatePickerIsland.tsx
/**
* DatePicker island — renders the "Select dates" section with two
* side-by-side calendars connected to the shared booking store.
*/
function DatePicker({ bookingKey, initialCheckIn, initialCheckOut, month1, year1, month2, year2 }) {
	const booking = useStore($booking);
	const initialised = useRef(false);
	useEffect(() => {
		if (!initialised.current) {
			initialised.current = true;
			initBooking(bookingKey, initialCheckIn ?? null, initialCheckOut ?? null);
		}
	}, [
		bookingKey,
		initialCheckIn,
		initialCheckOut
	]);
	const hasDates = booking.checkIn || booking.checkOut;
	return /* @__PURE__ */ jsxs("section", {
		className: "py-8",
		children: [/* @__PURE__ */ jsxs("div", {
			className: "mb-6 flex items-end justify-between gap-4",
			children: [/* @__PURE__ */ jsxs("div", { children: [/* @__PURE__ */ jsx("h2", {
				className: "text-[24px] font-semibold tracking-[-0.03em] text-[#222]",
				children: "Select dates"
			}), /* @__PURE__ */ jsx("p", {
				className: "mt-2 text-[15px] text-[var(--air-muted)]",
				children: "Add your travel dates for exact pricing"
			})] }), hasDates && /* @__PURE__ */ jsx("button", {
				type: "button",
				className: "text-sm font-semibold underline underline-offset-4",
				onClick: clearDates,
				children: "Clear dates"
			})]
		}), /* @__PURE__ */ jsxs("div", {
			className: "grid gap-5 md:grid-cols-2",
			children: [/* @__PURE__ */ jsx("div", {
				className: "rounded-[22px] border border-[var(--air-line)] p-5",
				children: /* @__PURE__ */ jsx(Calendar, {
					month: month1,
					year: year1
				})
			}), /* @__PURE__ */ jsx("div", {
				className: "rounded-[22px] border border-[var(--air-line)] p-5",
				children: /* @__PURE__ */ jsx(Calendar, {
					month: month2,
					year: year2
				})
			})]
		})]
	});
}
//#endregion
export { DatePicker as default };
