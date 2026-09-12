import { a as closePopover, c as initBooking, d as useStore, i as clearField, l as nightsBetween, n as $booking, o as formatLongDate, r as clearDates, s as formatShortDate, t as Calendar, u as setActiveField } from "./chunks/Calendar-IoQONdKJ.js";
import { useEffect, useRef } from "react";
import { jsx, jsxs } from "react/jsx-runtime";
//#region islands/BookingCardIsland.tsx
/**
* BookingCard island — the sticky booking sidebar with pricing,
* check-in/checkout triggers, and a popover containing two calendars.
* Connected to the shared booking nanostore.
*/
const STAR_SVG = `<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="#FF385C"><path d="M12 2l3.09 6.26L22 9.27l-5 4.87 1.18 6.88L12 17.77l-6.18 3.25L7 14.14 2 9.27l6.91-1.01L12 2z"/></svg>`;
const CLOSE_SVG = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32" width="12" height="12" aria-hidden="true"><path d="m6 6 20 20M26 6 6 26" fill="none" stroke="currentColor" stroke-width="4"/></svg>`;
function BookingCard({ bookingKey, nightlyRate, cleaningFee, serviceFee, rating, reviewCount, guests, freeCancelDate, initialCheckIn, initialCheckOut, month1, year1, month2, year2 }) {
	const booking = useStore($booking);
	const rootRef = useRef(null);
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
	useEffect(() => {
		function handleClick(e) {
			if (!booking.popoverOpen) return;
			const root = rootRef.current;
			if (root && !root.contains(e.target)) closePopover();
		}
		document.addEventListener("click", handleClick);
		return () => document.removeEventListener("click", handleClick);
	}, [booking.popoverOpen]);
	const nights = nightsBetween(booking.checkIn, booking.checkOut);
	const nightlyTotal = nightlyRate * nights;
	const total = nights > 0 ? nightlyTotal + cleaningFee + serviceFee : 0;
	const nightsLabel = nights === 1 ? "1 night" : `${nights} nights`;
	const rangeLabel = booking.checkIn && booking.checkOut ? `${formatLongDate(booking.checkIn)} – ${formatLongDate(booking.checkOut)}` : "Select your travel dates";
	const headerTotal = nights > 0 ? `$${total.toLocaleString()}` : `$${nightlyRate.toLocaleString()}`;
	const headerContext = nights > 0 ? `for ${nights} night${nights === 1 ? "" : "s"}` : "add dates for total";
	return /* @__PURE__ */ jsxs("div", {
		className: "sticky top-28",
		ref: rootRef,
		children: [/* @__PURE__ */ jsxs("div", {
			className: "relative",
			children: [
				/* @__PURE__ */ jsx("div", {
					className: "mb-4 flex justify-end",
					children: /* @__PURE__ */ jsxs("div", {
						className: "airbnb-booking-pill",
						children: [/* @__PURE__ */ jsx("span", { dangerouslySetInnerHTML: { __html: STAR_SVG } }), /* @__PURE__ */ jsx("span", { children: "Prices include all fees" })]
					})
				}),
				/* @__PURE__ */ jsxs("div", {
					className: "air-booking-card airbnb-booking-card p-6",
					children: [
						/* @__PURE__ */ jsxs("div", {
							className: "mb-4",
							children: [/* @__PURE__ */ jsxs("div", {
								className: "flex flex-wrap items-baseline gap-x-2 gap-y-1 text-[#222]",
								children: [/* @__PURE__ */ jsx("span", {
									className: "text-[32px] font-semibold tracking-[-0.04em] text-[#222]",
									children: headerTotal
								}), /* @__PURE__ */ jsx("span", {
									className: "text-[15px] text-[var(--air-muted)]",
									children: headerContext
								})]
							}), /* @__PURE__ */ jsxs("div", {
								className: "mt-2 flex items-center gap-1 text-sm text-[#222]",
								children: [
									/* @__PURE__ */ jsx("span", { dangerouslySetInnerHTML: { __html: STAR_SVG } }),
									/* @__PURE__ */ jsx("span", {
										className: "font-medium",
										children: rating
									}),
									/* @__PURE__ */ jsx("span", { children: "·" }),
									/* @__PURE__ */ jsxs("a", {
										href: "#reviews",
										className: "underline",
										children: [reviewCount, " reviews"]
									})
								]
							})]
						}),
						/* @__PURE__ */ jsxs("div", {
							className: "mb-3 overflow-hidden rounded-2xl border border-[#B0B0B0]",
							children: [/* @__PURE__ */ jsxs("div", {
								className: "grid grid-cols-2",
								children: [/* @__PURE__ */ jsxs("button", {
									type: "button",
									className: `airbnb-booking-trigger border-b border-r border-[#B0B0B0] px-3 py-2.5 text-left transition-colors hover:bg-[var(--air-soft)]`,
									"data-active": booking.activeField === "checkIn" && booking.popoverOpen ? "true" : "false",
									onClick: () => setActiveField("checkIn"),
									children: [/* @__PURE__ */ jsx("span", {
										className: "block text-[10px] font-bold tracking-wide uppercase text-[#222]",
										children: "CHECK-IN"
									}), /* @__PURE__ */ jsx("span", {
										className: "airbnb-booking-display mt-0.5 block text-sm text-[#222]",
										"data-empty": booking.checkIn ? "false" : "true",
										children: formatShortDate(booking.checkIn)
									})]
								}), /* @__PURE__ */ jsxs("button", {
									type: "button",
									className: `airbnb-booking-trigger border-b border-[#B0B0B0] px-3 py-2.5 text-left transition-colors hover:bg-[var(--air-soft)]`,
									"data-active": booking.activeField === "checkOut" && booking.popoverOpen ? "true" : "false",
									onClick: () => setActiveField("checkOut"),
									children: [/* @__PURE__ */ jsx("span", {
										className: "block text-[10px] font-bold tracking-wide uppercase text-[#222]",
										children: "CHECKOUT"
									}), /* @__PURE__ */ jsx("span", {
										className: "airbnb-booking-display mt-0.5 block text-sm text-[#222]",
										"data-empty": booking.checkOut ? "false" : "true",
										children: formatShortDate(booking.checkOut)
									})]
								})]
							}), /* @__PURE__ */ jsxs("div", {
								className: "px-3 py-2.5",
								children: [/* @__PURE__ */ jsx("label", {
									className: "block text-[10px] font-bold tracking-wide uppercase text-[#222]",
									children: "GUESTS"
								}), /* @__PURE__ */ jsx("p", {
									className: "mt-0.5 text-sm text-[#222]",
									children: guests
								})]
							})]
						}),
						/* @__PURE__ */ jsxs("div", {
							className: "mb-4 rounded-xl bg-[var(--air-soft)] px-4 py-2.5 text-center text-xs font-medium text-[#222]",
							children: ["Free cancellation before ", freeCancelDate]
						}),
						/* @__PURE__ */ jsx("button", {
							type: "button",
							className: "mb-4 w-full rounded-xl bg-gradient-to-r from-[#E61E4D] via-[#E31C5F] to-[#D70466] py-3.5 font-semibold text-white transition-transform hover:scale-[1.01]",
							children: "Reserve"
						}),
						/* @__PURE__ */ jsx("p", {
							className: "mb-5 text-center text-sm text-[var(--air-muted)]",
							children: "You won’t be charged yet"
						}),
						/* @__PURE__ */ jsxs("div", {
							className: "space-y-4 text-sm",
							children: [
								/* @__PURE__ */ jsxs("div", {
									className: "flex justify-between",
									children: [/* @__PURE__ */ jsx("span", {
										className: "text-[#222] underline",
										children: nights > 0 ? `$${nightlyRate.toLocaleString()} x ${nights} night${nights === 1 ? "" : "s"}` : "Select dates"
									}), /* @__PURE__ */ jsx("span", {
										className: "text-[#222]",
										children: nights > 0 ? `$${nightlyTotal.toLocaleString()}` : "$0"
									})]
								}),
								/* @__PURE__ */ jsxs("div", {
									className: "flex justify-between",
									children: [/* @__PURE__ */ jsx("span", {
										className: "text-[#222] underline",
										children: "Cleaning fee"
									}), /* @__PURE__ */ jsxs("span", {
										className: "text-[#222]",
										children: ["$", cleaningFee]
									})]
								}),
								/* @__PURE__ */ jsxs("div", {
									className: "flex justify-between",
									children: [/* @__PURE__ */ jsx("span", {
										className: "text-[#222] underline",
										children: "Airbnb service fee"
									}), /* @__PURE__ */ jsxs("span", {
										className: "text-[#222]",
										children: ["$", serviceFee]
									})]
								}),
								/* @__PURE__ */ jsx("hr", { className: "air-divider" }),
								/* @__PURE__ */ jsxs("div", {
									className: "flex justify-between font-semibold",
									children: [/* @__PURE__ */ jsx("span", {
										className: "text-[#222]",
										children: "Total before taxes"
									}), /* @__PURE__ */ jsxs("span", {
										className: "text-[#222]",
										children: ["$", total.toLocaleString()]
									})]
								})
							]
						})
					]
				}),
				/* @__PURE__ */ jsx("div", {
					className: `airbnb-booking-popover ${booking.popoverOpen ? "" : "hidden"}`,
					"aria-hidden": !booking.popoverOpen,
					children: /* @__PURE__ */ jsxs("div", {
						className: "airbnb-popover-shell",
						children: [
							/* @__PURE__ */ jsxs("div", {
								className: "airbnb-popover-header",
								children: [/* @__PURE__ */ jsxs("div", {
									className: "airbnb-popover-summary",
									children: [/* @__PURE__ */ jsx("h3", { children: nights > 0 ? nightsLabel : "Select dates" }), /* @__PURE__ */ jsx("p", { children: rangeLabel })]
								}), /* @__PURE__ */ jsxs("div", {
									className: "airbnb-popover-fields",
									children: [/* @__PURE__ */ jsxs("div", {
										className: "airbnb-popover-field",
										children: [/* @__PURE__ */ jsxs("button", {
											type: "button",
											className: "block w-full text-left",
											onClick: () => setActiveField("checkIn"),
											children: [/* @__PURE__ */ jsx("span", {
												className: "airbnb-popover-field-label",
												children: "Check-in"
											}), /* @__PURE__ */ jsx("span", {
												className: "airbnb-popover-field-value airbnb-popover-value",
												"data-empty": booking.checkIn ? "false" : "true",
												children: formatShortDate(booking.checkIn)
											})]
										}), booking.checkIn && /* @__PURE__ */ jsx("button", {
											type: "button",
											className: "airbnb-popover-clear",
											"aria-label": "Clear dates",
											onClick: () => clearField("checkIn"),
											children: /* @__PURE__ */ jsx("span", { dangerouslySetInnerHTML: { __html: CLOSE_SVG } })
										})]
									}), /* @__PURE__ */ jsxs("div", {
										className: "airbnb-popover-field",
										children: [/* @__PURE__ */ jsxs("button", {
											type: "button",
											className: "block w-full text-left",
											onClick: () => setActiveField("checkOut"),
											children: [/* @__PURE__ */ jsx("span", {
												className: "airbnb-popover-field-label",
												children: "Checkout"
											}), /* @__PURE__ */ jsx("span", {
												className: "airbnb-popover-field-value airbnb-popover-value",
												"data-empty": booking.checkOut ? "false" : "true",
												children: formatShortDate(booking.checkOut)
											})]
										}), booking.checkOut && /* @__PURE__ */ jsx("button", {
											type: "button",
											className: "airbnb-popover-clear",
											"aria-label": "Clear checkout date",
											onClick: () => clearField("checkOut"),
											children: /* @__PURE__ */ jsx("span", { dangerouslySetInnerHTML: { __html: CLOSE_SVG } })
										})]
									})]
								})]
							}),
							/* @__PURE__ */ jsxs("div", {
								className: "airbnb-popover-calendars",
								children: [/* @__PURE__ */ jsx("div", {
									className: "airbnb-popover-calendar",
									children: /* @__PURE__ */ jsx(Calendar, {
										month: month1,
										year: year1,
										inPopover: true
									})
								}), /* @__PURE__ */ jsx("div", {
									className: "airbnb-popover-calendar",
									children: /* @__PURE__ */ jsx(Calendar, {
										month: month2,
										year: year2,
										inPopover: true
									})
								})]
							}),
							/* @__PURE__ */ jsxs("div", {
								className: "airbnb-popover-footer",
								children: [/* @__PURE__ */ jsx("button", {
									type: "button",
									className: "airbnb-link-button",
									onClick: clearDates,
									children: "Clear dates"
								}), /* @__PURE__ */ jsx("button", {
									type: "button",
									className: "airbnb-close-button",
									onClick: closePopover,
									children: "Close"
								})]
							})
						]
					})
				})
			]
		}), /* @__PURE__ */ jsxs("button", {
			type: "button",
			className: "mt-6 inline-flex items-center gap-2 text-sm font-semibold text-[#222] underline underline-offset-4",
			children: [/* @__PURE__ */ jsxs("svg", {
				xmlns: "http://www.w3.org/2000/svg",
				width: "16",
				height: "16",
				viewBox: "0 0 24 24",
				fill: "none",
				stroke: "currentColor",
				strokeWidth: "1.9",
				strokeLinecap: "round",
				strokeLinejoin: "round",
				children: [
					/* @__PURE__ */ jsx("path", { d: "M12 9v4" }),
					/* @__PURE__ */ jsx("path", { d: "M12 17h.01" }),
					/* @__PURE__ */ jsx("path", { d: "M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z" })
				]
			}), "Report this listing"]
		})]
	});
}
//#endregion
export { BookingCard as default };
