//#region airbnb-showcase.ts
// Navigate to a showcase route. When inside the playground iframe, posts a
// message to the parent so it can switch pages; otherwise navigates directly.
function navigateShowcase(href) {
	try { var pathname = new URL(href, document.baseURI).pathname; } catch { var pathname = href.split('?')[0]; }
	if (window.parent !== window) {
		window.parent.postMessage({ type: 'proa-nav', href: pathname }, '*');
	} else {
		window.location.href = href;
	}
}
let globalEventsBound = false;
function initAirImages(root = document) {
	root.querySelectorAll("[data-air-image]").forEach((img) => {
		if (img.dataset.airImageHydrated === "true") return;
		img.dataset.airImageHydrated = "true";
		const markLoaded = () => {
			img.dataset.airImageState = "loaded";
		};
		const markErrored = () => {
			const fallbackSrc = img.getAttribute("data-fallback-src");
			if (fallbackSrc && img.dataset.fallbackApplied !== "true" && img.currentSrc !== fallbackSrc && img.getAttribute("src") !== fallbackSrc) {
				img.dataset.fallbackApplied = "true";
				img.setAttribute("src", fallbackSrc);
				return;
			}
			img.dataset.airImageState = "error";
		};
		img.addEventListener("load", markLoaded);
		img.addEventListener("error", markErrored);
		if (img.complete) if (img.naturalWidth > 0) markLoaded();
		else markErrored();
	});
}
function initReviewToggles(page) {
	page.querySelectorAll("[data-review-card]").forEach((card) => {
		if (card.dataset.reviewMounted === "true") return;
		card.dataset.reviewMounted = "true";
		const button = card.querySelector("[data-review-toggle]");
		const body = card.querySelector("[data-review-body]");
		if (!button || !body) return;
		if (!(body.scrollHeight > body.clientHeight + 4)) {
			button.classList.add("hidden");
			return;
		}
		button.addEventListener("click", () => {
			const expanded = card.dataset.expanded === "true";
			card.dataset.expanded = expanded ? "false" : "true";
			button.textContent = expanded ? "Show more" : "Show less";
		});
	});
}
function initListingPage(page) {
	if (page.dataset.airbnbListingMounted === "true") return;
	page.dataset.airbnbListingMounted = "true";
	initReviewToggles(page);
}
function initHomeSearch() {
	const shell = document.querySelector(".home-search-shell");
	if (!shell || shell.dataset.homeSearchMounted === "true") return;
	shell.dataset.homeSearchMounted = "true";
	const tabs = Array.from(document.querySelectorAll("[data-home-mode]"));
	const panels = Array.from(document.querySelectorAll("[data-home-panel]"));
	const segments = Array.from(document.querySelectorAll("[data-search-field]"));
	const popover = document.getElementById("home-search-popover");
	const popoverOptions = popover?.querySelector("[data-popover-options]");
	const popoverTitle = popover?.querySelector("[data-popover-title]");
	const popoverSubtitle = popover?.querySelector("[data-popover-subtitle]");
	const closeBtn = document.querySelector("[data-home-search-close]");
	const submitBtn = document.querySelector("[data-home-search-submit]");
	if (!tabs.length || !popover || !popoverOptions || !popoverTitle || !popoverSubtitle) return;
	const defaults = {
		homes: {
			where: "Search destinations",
			when: "Add dates",
			who: "Add guests"
		},
		experiences: {
			where: "Search experiences",
			when: "Pick a day",
			who: "Add guests"
		},
		services: {
			where: "Where are you staying?",
			when: "Add a day",
			who: "What do you need?"
		}
	};
	const config = {
		homes: {
			where: {
				title: "Where do you want to go?",
				subtitle: "Popular destinations to populate the search bar quickly.",
				options: [
					"Los Angeles",
					"Malibu",
					"Santa Monica",
					"Topanga",
					"Venice Beach"
				]
			},
			when: {
				title: "When is your trip?",
				subtitle: "Use one of the preset date ranges from the live Airbnb flow.",
				options: [
					"Mar 11-18",
					"Mar 18-25",
					"Apr 2-9",
					"Flexible weekend"
				]
			},
			who: {
				title: "Who is coming?",
				subtitle: "Guest count updates the summary before sending you to search.",
				options: [
					"2 guests",
					"4 guests",
					"Family trip",
					"Solo stay"
				]
			}
		},
		experiences: {
			where: {
				title: "Where should it happen?",
				subtitle: "Experiences lean more local and activity-driven.",
				options: [
					"Downtown LA",
					"Silver Lake",
					"Venice",
					"Malibu coast"
				]
			},
			when: {
				title: "What day works best?",
				subtitle: "Experiences are usually tighter on scheduling.",
				options: [
					"This weekend",
					"Next Thursday",
					"Sunset session",
					"Any evening"
				]
			},
			who: {
				title: "How many people?",
				subtitle: "Small groups feel closest to the Airbnb reference.",
				options: [
					"2 guests",
					"4 guests",
					"6 guests",
					"Private group"
				]
			}
		},
		services: {
			where: {
				title: "Where should the service happen?",
				subtitle: "Services assume you already have a stay booked nearby.",
				options: [
					"Malibu stay",
					"Santa Monica stay",
					"West Hollywood",
					"Venice loft"
				]
			},
			when: {
				title: "What day do you need it?",
				subtitle: "Services are generally tied to a specific booking window.",
				options: [
					"Arrival day",
					"Mid-stay",
					"Checkout day",
					"This Saturday"
				]
			},
			who: {
				title: "Which service?",
				subtitle: "Use a preset to change the summary text.",
				options: [
					"Private chef",
					"Massage",
					"Photography",
					"Hair & makeup"
				]
			}
		}
	};
	let mode = "homes";
	let activeField = "where";
	const values = {
		homes: { ...defaults.homes },
		experiences: { ...defaults.experiences },
		services: { ...defaults.services }
	};
	const syncSearchValues = () => {
		segments.forEach((segment) => {
			const field = segment.dataset.searchField;
			const valueNode = segment.querySelector(".home-search-value");
			if (!field || !valueNode) return;
			valueNode.textContent = values[mode][field];
			segment.classList.toggle("active", field === activeField);
		});
	};
	const renderPopover = () => {
		const fieldConfig = config[mode][activeField];
		popoverTitle.textContent = fieldConfig.title;
		popoverSubtitle.textContent = fieldConfig.subtitle;
		popoverOptions.innerHTML = "";
		fieldConfig.options.forEach((option) => {
			const button = document.createElement("button");
			button.type = "button";
			button.className = "home-search-option";
			button.textContent = option;
			button.addEventListener("click", () => {
				values[mode][activeField] = option;
				if (activeField === "where") activeField = "when";
				else if (activeField === "when") activeField = "who";
				else popover.classList.remove("open");
				syncSearchValues();
				if (popover.classList.contains("open")) renderPopover();
			});
			popoverOptions.appendChild(button);
		});
	};
	const setMode = (nextMode) => {
		mode = nextMode;
		tabs.forEach((tab) => {
			const active = tab.dataset.homeMode === nextMode;
			tab.classList.toggle("active", active);
			if (tab.getAttribute("role") === "tab") tab.setAttribute("aria-selected", active ? "true" : "false");
		});
		panels.forEach((panel) => panel.classList.toggle("active", panel.dataset.homePanel === nextMode));
		activeField = "where";
		syncSearchValues();
		renderPopover();
	};
	tabs.forEach((tab) => {
		tab.addEventListener("click", () => {
			setMode(tab.dataset.homeMode ?? "homes");
		});
	});
	segments.forEach((segment) => {
		segment.addEventListener("click", () => {
			activeField = segment.dataset.searchField ?? "where";
			syncSearchValues();
			renderPopover();
			popover.classList.add("open");
		});
	});
	closeBtn?.addEventListener("click", () => popover.classList.remove("open"));
	document.addEventListener("click", (event) => {
		const target = event.target;
		if (!(target instanceof Node)) return;
		if (!popover.contains(target) && !segments.some((segment) => segment.contains(target))) popover.classList.remove("open");
	});
	submitBtn?.addEventListener("click", () => {
		const params = new URLSearchParams({
			mode,
			where: values[mode].where,
			when: values[mode].when,
			who: values[mode].who
		});
		navigateShowcase(`/showcase/airbnb/proa/search?${params.toString()}`);
	});
	setMode("homes");
}
function initSearchMap() {
	const cards = Array.from(document.querySelectorAll("[data-map-index]"));
	const pins = Array.from(document.querySelectorAll("[data-map-pin]"));
	if (!cards.length || !pins.length) return;
	const activate = (index) => {
		cards.forEach((card) => card.classList.toggle("is-active", card.dataset.mapIndex === index));
		pins.forEach((pin) => pin.classList.toggle("active", pin.dataset.mapPin === index));
	};
	cards.forEach((card) => {
		if (card.dataset.mapBound === "true") return;
		card.dataset.mapBound = "true";
		const index = card.dataset.mapIndex;
		card.addEventListener("mouseenter", () => activate(index));
		card.addEventListener("focusin", () => activate(index));
	});
	pins.forEach((pin) => {
		if (pin.dataset.mapBound === "true") return;
		pin.dataset.mapBound = "true";
		const index = pin.dataset.mapPin;
		pin.addEventListener("mouseenter", () => activate(index));
		pin.addEventListener("focus", () => activate(index));
		pin.addEventListener("click", () => {
			activate(index);
			cards.find((card) => card.dataset.mapIndex === index)?.scrollIntoView({
				behavior: "smooth",
				block: "center"
			});
		});
	});
}
function initGallery() {
	const modal = document.getElementById("gallery-modal");
	const image = document.getElementById("gallery-image");
	const counter = document.getElementById("gallery-counter");
	const counterSr = document.getElementById("gallery-counter-sr");
	const prevBtn = modal?.querySelector("[data-gallery-prev]");
	const nextBtn = modal?.querySelector("[data-gallery-next]");
	const triggers = Array.from(document.querySelectorAll("[data-gallery-open]"));
	if (!modal || !image || !counter || !prevBtn || !nextBtn || !triggers.length || modal.dataset.galleryMounted === "true") return;
	modal.dataset.galleryMounted = "true";
	const photos = Array.from(document.querySelectorAll("[data-gallery-src]")).map((node) => ({
		src: node.getAttribute("data-gallery-src") ?? "",
		alt: node.getAttribute("data-gallery-alt") ?? "Listing photo"
	}));
	let activeIndex = 0;
	const render = () => {
		const photo = photos[activeIndex];
		if (!photo) return;
		image.setAttribute("src", photo.src);
		image.setAttribute("alt", photo.alt);
		counter.textContent = `${activeIndex + 1} / ${photos.length}`;
		if (counterSr) counterSr.textContent = `Showing photo ${activeIndex + 1} of ${photos.length}`;
		prevBtn.disabled = activeIndex === 0;
		nextBtn.disabled = activeIndex === photos.length - 1;
	};
	const open = (index) => {
		activeIndex = Math.max(0, Math.min(index, photos.length - 1));
		render();
		modal.classList.remove("hidden");
		document.body.style.overflow = "hidden";
	};
	const close = () => {
		modal.classList.add("hidden");
		document.body.style.overflow = "";
	};
	triggers.forEach((trigger) => {
		trigger.addEventListener("click", (event) => {
			event.preventDefault();
			event.stopPropagation();
			open(Number(trigger.getAttribute("data-gallery-index") ?? "0"));
		});
	});
	modal.querySelectorAll("[data-gallery-close]").forEach((button) => button.addEventListener("click", close));
	prevBtn.addEventListener("click", () => {
		if (activeIndex > 0) {
			activeIndex--;
			render();
		}
	});
	nextBtn.addEventListener("click", () => {
		if (activeIndex < photos.length - 1) {
			activeIndex++;
			render();
		}
	});
	document.addEventListener("keydown", (event) => {
		if (modal.classList.contains("hidden")) return;
		if (event.key === "Escape") close();
		if (event.key === "ArrowLeft" && activeIndex > 0) {
			activeIndex--;
			render();
		}
		if (event.key === "ArrowRight" && activeIndex < photos.length - 1) {
			activeIndex++;
			render();
		}
	});
}
function bindGlobalEvents() {
	if (globalEventsBound) return;
	globalEventsBound = true;
	document.addEventListener("click", async (event) => {
		const target = event.target;
		if (!(target instanceof Element)) return;
		const airTab = target.closest(".air-tab");
		if (airTab) {
			airTab.parentElement?.querySelectorAll(".air-tab").forEach((item) => item.classList.remove("active"));
			airTab.classList.add("active");
			return;
		}
		const heartButton = target.closest(".heart-btn");
		if (heartButton) {
			event.preventDefault();
			event.stopPropagation();
			heartButton.classList.toggle("saved");
			return;
		}
		const filterPill = target.closest(".filter-pill");
		if (filterPill) {
			filterPill.classList.toggle("is-selected");
			return;
		}
		const shareButton = target.closest("[data-action=\"share\"]");
		if (shareButton) {
			const original = shareButton.dataset.originalHtml ?? shareButton.innerHTML;
			shareButton.dataset.originalHtml = original;
			if (navigator.share) {
				try {
					await navigator.share({
						title: document.title,
						url: location.href
					});
				} catch {}
				return;
			}
			try {
				await navigator.clipboard.writeText(location.href);
				shareButton.innerHTML = "Link copied!";
				window.setTimeout(() => {
					shareButton.innerHTML = original;
				}, 1800);
			} catch {}
			return;
		}
		const railButton = target.closest("[data-rail-prev], [data-rail-next]");
		if (railButton) {
			const targetId = railButton.getAttribute("data-rail-prev") ?? railButton.getAttribute("data-rail-next");
			const rail = targetId ? document.getElementById(targetId) : null;
			if (!rail) return;
			const direction = railButton.hasAttribute("data-rail-next") ? 1 : -1;
			rail.scrollBy({
				left: direction * Math.max(rail.clientWidth * .8, 320),
				behavior: "smooth"
			});
		}
	});
}
function bootstrapAirbnbShowcase() {
	bindGlobalEvents();
	initAirImages(document);
	initHomeSearch();
	initSearchMap();
	initGallery();
	document.querySelectorAll("[data-airbnb-listing-page]").forEach((page) => {
		initListingPage(page);
	});
}
if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", bootstrapAirbnbShowcase, { once: true });
else bootstrapAirbnbShowcase();
document.addEventListener("turbo:load", bootstrapAirbnbShowcase);
document.addEventListener("turbo:render", bootstrapAirbnbShowcase);
//#endregion
