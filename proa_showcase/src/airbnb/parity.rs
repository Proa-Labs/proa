// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Dated Airbnb public-home interaction fixture.
//!
//! This route intentionally does not load Tailwind's browser compiler, Turbo,
//! React, or the legacy showcase controller. The complete interactive surface
//! is one server-rendered rsjs island so browser payload measurements reflect
//! Proa's production authoring model.

use proa_core::{
    DataLoader, RenderBuffer, StaticRaw, WebContext, WebRenderSync, WriteBuf, WriteError,
};
use proa_macros::html_sync;
use rsjs::{event_handler, signal, web::dom};
use rsjs_macros::rsjs;

use super::shared::{AIRBNB_LOGO_SVG, GLOBE_SVG};

const HEART_SVG: &str = r##"<svg viewBox="0 0 32 32" aria-hidden="true"><path d="M16 28.7C8.8 23.8 1.7 17.8 1.7 10.6c0-1.9.7-3.7 2.1-5.1s3.2-2.1 5.1-2.1c1.8 0 3.7.7 5.1 2.1L16 7.6l2.1-2.1c1.4-1.4 3.2-2.1 5.1-2.1 1.8 0 3.7.7 5.1 2.1s2.1 3.3 2.1 5.1c0 7.2-7.2 13.2-14.4 18.1z"/></svg>"##;
const SEARCH_SVG: &str = r##"<svg viewBox="0 0 32 32" aria-hidden="true"><path d="m13 24c6.1 0 11-4.9 11-11S19.1 2 13 2 2 6.9 2 13s4.9 11 11 11zm8-3 10 10"/></svg>"##;
const MENU_SVG: &str =
    r##"<svg viewBox="0 0 32 32" aria-hidden="true"><path d="M4 8h24M4 16h24M4 24h24"/></svg>"##;
const TAG_SVG: &str = r##"<svg viewBox="0 0 32 32" aria-hidden="true"><path d="M3 4h11l15 15-10 10L4 14 3 4z"/><circle cx="9.5" cy="10.5" r="2" fill="white"/></svg>"##;
const HOUSE_POSTER: &str = "/images/airbnb/external/search-homes.png";
const BALLOON_POSTER: &str = "/images/airbnb/external/search-experiences.png";
const SERVICE_POSTER: &str = "/images/airbnb/external/search-services.png";

const CSS: &str = r#"
@font-face {
  font-family: "Airbnb Cereal VF";
  src: url("https://a0.muscache.com/airbnb/static/airbnb-dls-web/build/fonts/cereal-variable/AirbnbCerealVF_W_Wght.8816d9e5c3b6a860636193e36b6ac4e4.woff2") format("woff2-variations");
  font-display: swap;
}
:root {
  --air-text: #222;
  --air-muted: #6a6a6a;
  --air-line: #ddd;
  --air-soft: #f7f7f7;
  --air-pink: #ff385c;
}
* { box-sizing: border-box; }
html { scroll-behavior: smooth; }
body {
  margin: 0;
  color: var(--air-text);
  background: #fff;
  font-family: "Airbnb Cereal VF", Circular, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
  -webkit-font-smoothing: antialiased;
}
button, input { font: inherit; }
button, a { -webkit-tap-highlight-color: transparent; }
button { color: inherit; }
a { color: inherit; text-decoration: none; }
[hidden] { display: none !important; }
.sr-only {
  position: absolute;
  width: 1px;
  height: 1px;
  overflow: hidden;
  clip: rect(0, 0, 0, 0);
  white-space: nowrap;
  clip-path: inset(50%);
}
.air-parity-shell { width: min(100% - 96px, 1440px); margin-inline: auto; }
.air-parity-header {
  position: sticky;
  top: 0;
  z-index: 30;
  height: 96px;
  background: rgba(255,255,255,.96);
  backdrop-filter: blur(16px);
  transition: box-shadow .2s ease;
}
/* Opaque backdrop that covers the search band (96px..201px) so scrolling
   content is hidden before the bottom line reaches it. Its visible bottom
   edge tracks .air-parity-header::after exactly (same 105px travel, same
   spring), so the line never bisects a still-visible rail title mid-collapse. */
.air-parity-header::before {
  content: "";
  position: absolute;
  top: 96px;
  right: 0;
  left: 0;
  height: 105px;
  background: #fff;
  clip-path: inset(0 0 0 0);
  pointer-events: none;
  will-change: clip-path;
  transition: clip-path .45s linear(0 0%, .185572 10%, .465306 20%, .682334 30%, .822325 40%, .904974 50%, .951289 60%, .976364 70%, .989612 80%, .996485 90%, 1 100%);
}
.air-parity-header::after {
  content: "";
  position: absolute;
  right: 0;
  bottom: -105px;
  left: 0;
  height: 1px;
  background: #ebebeb;
  pointer-events: none;
  transition: transform .45s linear(0 0%, .185572 10%, .465306 20%, .682334 30%, .822325 40%, .904974 50%, .951289 60%, .976364 70%, .989612 80%, .996485 90%, 1 100%);
}
.air-parity-header[data-compact="true"] {
  box-shadow: 0 1px 0 rgba(0,0,0,.04);
}
.air-parity-header[data-compact="true"]::before { clip-path: inset(0 0 105px 0); }
.air-parity-header[data-compact="true"]::after { transform: translateY(-105px); }
.air-parity-header > .air-parity-shell { position: relative; height: 100%; }
.air-parity-top {
  position: relative;
  z-index: 2;
  display: grid;
  grid-template-columns: 1fr auto 1fr;
  align-items: center;
  min-height: 96px;
  gap: 24px;
}
.air-parity-brand {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  width: max-content;
  color: var(--air-pink);
  font-size: 22px;
  font-weight: 700;
}
.air-parity-modes {
  display: flex;
  align-items: flex-end;
  justify-content: center;
  gap: 30px;
  transform: translateY(0);
  transform-origin: 50% 0;
  transition: transform .42s cubic-bezier(.2,.82,.2,1), opacity .18s ease;
  will-change: transform;
}
.air-parity-header[data-compact="true"] .air-parity-modes {
  transform: translateY(-87px);
  opacity: 0;
  pointer-events: none;
}
.air-parity-mode {
  position: relative;
  display: grid;
  grid-template-columns: 36px auto;
  align-items: center;
  gap: 9px;
  border: 0;
  background: transparent;
  padding: 12px 4px 14px;
  cursor: pointer;
  color: var(--air-muted);
  font-size: 14px;
}
.air-parity-mode img { width: 36px; height: 36px; object-fit: contain; transform: scale(2); }
.air-parity-mode[data-active="true"] { color: var(--air-text); font-weight: 600; }
.air-parity-mode[data-active="true"]::after {
  content: "";
  position: absolute;
  right: 0;
  bottom: 0;
  left: 0;
  height: 3px;
  border-radius: 99px;
  background: var(--air-text);
}
.air-parity-new {
  position: absolute;
  top: 0;
  left: 17px;
  border-radius: 5px 5px 5px 0;
  background: #3d536d;
  padding: 2px 5px;
  color: white;
  font-size: 8px;
  font-weight: 800;
}
.air-parity-actions { position: relative; display: flex; align-items: center; justify-content: flex-end; gap: 12px; }
.air-parity-round,
.air-parity-profile,
.air-parity-host {
  border: 0;
  border-radius: 999px;
  background: transparent;
  cursor: pointer;
}
.air-parity-host { height: 40px; padding: 11px 12px; font-size: 14px; line-height: 18px; font-weight: 600; }
.air-parity-round,
.air-parity-profile { display: grid; width: 40px; height: 40px; place-items: center; }
.air-parity-round svg { width: 18px; height: 18px; }
.air-parity-profile {
  background: var(--air-soft);
  padding: 0;
}
.air-parity-profile svg { width: 18px; height: 18px; fill: none; stroke: currentColor; stroke-width: 2.4; }
.air-parity-host:hover,
.air-parity-round:hover,
.air-parity-profile:hover { background: #eee; }
.air-parity-menu {
  position: absolute;
  top: 54px;
  right: 0;
  z-index: 50;
  width: 280px;
  overflow: hidden;
  border: 1px solid #e6e6e6;
  border-radius: 18px;
  background: white;
  box-shadow: 0 12px 40px rgba(0,0,0,.16);
}
.air-parity-menu a,
.air-parity-menu button {
  display: block;
  width: 100%;
  border: 0;
  background: white;
  padding: 13px 18px;
  text-align: left;
  cursor: pointer;
}
.air-parity-menu a:hover,
.air-parity-menu button:hover { background: var(--air-soft); }
.air-parity-menu hr { margin: 4px 0; border: 0; border-top: 1px solid #ebebeb; }
.air-parity-search-wrap {
  position: relative;
  display: flex;
  justify-content: center;
  padding: 6px 0 32px;
  transform: translateY(0) scale(1);
  transform-origin: 50% 0;
  opacity: 1;
  transition: transform .42s cubic-bezier(.2,.82,.2,1), opacity .16s ease;
  will-change: transform, opacity;
}
.air-parity-header[data-compact="true"] .air-parity-search-wrap {
  transform: translateY(-77px) scale(.62);
  opacity: 0;
  pointer-events: none;
}
.air-parity-search {
  display: grid;
  width: min(850px, 100%);
  height: 66px;
  grid-template-columns: 1.25fr 1fr 1fr auto;
  align-items: center;
  border: 1px solid var(--air-line);
  border-radius: 999px;
  background: white;
  box-shadow: 0 8px 26px rgba(0,0,0,.12);
}
.air-parity-field {
  position: relative;
  min-width: 0;
  border: 0;
  border-radius: 999px;
  background: transparent;
  padding: 13px 24px;
  text-align: left;
  cursor: pointer;
}
.air-parity-field-input { cursor: text; }
.air-parity-field + .air-parity-field::before {
  content: "";
  position: absolute;
  top: 16px;
  bottom: 16px;
  left: 0;
  width: 1px;
  background: #ddd;
}
.air-parity-field:hover,
.air-parity-field[data-active="true"] { background: var(--air-soft); }
.air-parity-field:hover::before,
.air-parity-field[data-active="true"]::before,
.air-parity-field:hover + .air-parity-field::before,
.air-parity-field[data-active="true"] + .air-parity-field::before { opacity: 0; }
.air-parity-label { display: block; font-size: 12px; font-weight: 700; }
.air-parity-destination-input {
  display: block;
  width: 100%;
  min-width: 0;
  margin-top: 3px;
  border: 0;
  outline: 0;
  background: transparent;
  color: var(--air-text);
  font-size: 14px;
}
.air-parity-destination-input::placeholder { color: var(--air-muted); opacity: 1; }
.air-parity-value {
  display: block;
  overflow: hidden;
  margin-top: 3px;
  color: var(--air-muted);
  font-size: 14px;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.air-parity-submit {
  display: grid;
  width: 48px;
  height: 48px;
  margin-right: 8px;
  place-items: center;
  border: 0;
  border-radius: 999px;
  background: var(--air-pink);
  color: white;
  cursor: pointer;
}
.air-parity-submit svg { width: 16px; fill: none; stroke: currentColor; stroke-width: 4; }
.air-parity-compact-search {
  position: absolute;
  top: 25px;
  left: 50%;
  z-index: 3;
  display: grid;
  width: 376px;
  height: 46px;
  grid-template-columns: minmax(0,1.35fr) minmax(0,.85fr) minmax(0,1fr) 46px;
  align-items: center;
  overflow: hidden;
  transform: translate(-50%, 77px) scale(1.55);
  transform-origin: 50% 0;
  border: 1px solid var(--air-line);
  border-radius: 999px;
  background: white;
  box-shadow: 0 1px 2px rgba(0,0,0,.08), 0 4px 12px rgba(0,0,0,.05);
  opacity: 0;
  pointer-events: none;
  transition: transform .42s cubic-bezier(.2,.82,.2,1), opacity .18s ease;
  will-change: transform, opacity;
}
.air-parity-header[data-compact="true"] .air-parity-compact-search {
  transform: translate(-50%, 0) scale(1);
  opacity: 1;
  pointer-events: auto;
}
.air-parity-compact-field {
  position: relative;
  display: flex;
  min-width: 0;
  height: 100%;
  align-items: center;
  justify-content: center;
  gap: 8px;
  overflow: hidden;
  border: 0;
  background: transparent;
  padding: 0 13px;
  cursor: pointer;
  font-size: 14px;
  font-weight: 600;
  white-space: nowrap;
}
.air-parity-compact-field + .air-parity-compact-field::before {
  content: "";
  position: absolute;
  top: 13px;
  bottom: 13px;
  left: 0;
  width: 1px;
  background: #ddd;
}
.air-parity-compact-field:hover { background: var(--air-soft); }
.air-parity-compact-field img { width: 28px; height: 28px; flex: none; object-fit: contain; transform: scale(1.55); }
.air-parity-compact-field span {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}
.air-parity-compact-submit {
  display: grid;
  width: 32px;
  height: 32px;
  place-items: center;
  border: 0;
  border-radius: 999px;
  background: var(--air-pink);
  color: white;
  cursor: pointer;
}
.air-parity-compact-submit svg { width: 12px; fill: none; stroke: currentColor; stroke-width: 5; }
.air-parity-popover {
  position: absolute;
  top: 70px;
  left: 50%;
  z-index: 45;
  width: min(720px, calc(100vw - 48px));
  transform: translateX(-50%);
  border: 1px solid #e8e8e8;
  border-radius: 28px;
  background: white;
  padding: 28px;
  box-shadow: 0 16px 48px rgba(0,0,0,.18);
}
.air-parity-popover h2 { margin: 0; font-size: 20px; }
.air-parity-popover-copy { margin: 6px 0 20px; color: var(--air-muted); font-size: 14px; }
.air-parity-date-tabs {
  display: grid;
  width: max-content;
  grid-template-columns: repeat(2, minmax(110px, 1fr));
  margin: 0 auto 24px;
  border-radius: 999px;
  background: var(--air-soft);
  padding: 4px;
}
.air-parity-date-tab {
  border: 0;
  border-radius: 999px;
  background: transparent;
  padding: 10px 18px;
  cursor: pointer;
}
.air-parity-date-tab[data-active="true"] {
  background: white;
  box-shadow: 0 2px 9px rgba(0,0,0,.12);
  font-weight: 700;
}
.air-parity-calendar-nav {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 8px;
}
.air-parity-calendar-nav button {
  display: grid;
  width: 34px;
  height: 34px;
  place-items: center;
  border: 0;
  border-radius: 999px;
  background: transparent;
  cursor: pointer;
}
.air-parity-calendar-nav button:hover { background: var(--air-soft); }
.air-parity-calendar-nav button:disabled { cursor: default; opacity: .3; }
.air-parity-flexible h3 { margin: 0 0 14px; font-size: 16px; }
.air-parity-flex-options {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  margin-bottom: 26px;
}
.air-parity-flex-option {
  border: 1px solid var(--air-line);
  border-radius: 999px;
  background: white;
  padding: 10px 16px;
  cursor: pointer;
}
.air-parity-flex-option[data-active="true"] {
  border-color: var(--air-text);
  background: var(--air-text);
  color: white;
}
.air-parity-destinations { display: grid; grid-template-columns: repeat(2, minmax(0,1fr)); gap: 10px; }
.air-parity-option {
  display: grid;
  grid-template-columns: 42px minmax(0,1fr);
  align-items: center;
  gap: 12px;
  border: 0;
  border-radius: 14px;
  background: white;
  padding: 10px;
  text-align: left;
  cursor: pointer;
}
.air-parity-option:hover { background: var(--air-soft); }
.air-parity-option-icon { display: grid; width: 42px; height: 42px; place-items: center; border-radius: 10px; background: #eef5f8; }
.air-parity-option small { display: block; margin-top: 2px; color: var(--air-muted); }
.air-parity-calendars { display: grid; grid-template-columns: repeat(2,minmax(0,1fr)); gap: 32px; }
.air-parity-calendar h3 { margin: 0 0 14px; text-align: center; font-size: 15px; }
.air-parity-weekdays,
.air-parity-days { display: grid; grid-template-columns: repeat(7,1fr); gap: 4px; }
.air-parity-weekdays span { padding: 5px 0; color: var(--air-muted); text-align: center; font-size: 11px; }
.air-parity-day {
  aspect-ratio: 1;
  border: 0;
  border-radius: 999px;
  background: white;
  cursor: pointer;
}
.air-parity-day:hover,
.air-parity-day[data-selected="true"] { background: var(--air-text); color: white; }
.air-parity-day:disabled { color: #bbb; cursor: default; }
.air-parity-guests { display: grid; gap: 0; }
.air-parity-guest-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  border-bottom: 1px solid #ebebeb;
  padding: 16px 0;
}
.air-parity-guest-row:last-child { border-bottom: 0; }
.air-parity-guest-row h3 { margin: 0; font-size: 16px; }
.air-parity-guest-row p { margin: 3px 0 0; color: var(--air-muted); font-size: 14px; }
.air-parity-stepper { display: flex; align-items: center; gap: 14px; }
.air-parity-stepper button {
  display: grid;
  width: 32px;
  height: 32px;
  place-items: center;
  border: 1px solid #b0b0b0;
  border-radius: 999px;
  background: white;
  cursor: pointer;
}
.air-parity-stepper button:disabled { opacity: .35; cursor: default; }
.air-parity-main-and-footer {
  transform: translateY(105px);
  transition: transform .45s linear(0 0%, .185572 10%, .465306 20%, .682334 30%, .822325 40%, .904974 50%, .951289 60%, .976364 70%, .989612 80%, .996485 90%, 1 100%);
  will-change: transform;
}
.air-parity-main-and-footer[data-compact="true"] { transform: translateY(0); }
.air-parity-main { padding: 51px 0 64px; }
.air-parity-rail-section + .air-parity-rail-section { margin-top: 38px; }
.air-parity-rail-section:nth-child(2) { margin-top: 42px; }
.air-parity-rail-head { display: flex; align-items: end; justify-content: space-between; gap: 20px; margin-bottom: 14px; padding-inline: 2px; }
.air-parity-rail-head:has(p:not(:empty)) { margin-bottom: 16px; }
.air-parity-rail-title { display: flex; height: 24px; align-items: center; gap: 6px; }
.air-parity-rail-head h2 { margin: 0; font-size: 20px; line-height: 24px; font-weight: 650; letter-spacing: 0; }
.air-parity-rail-section:nth-child(2) h2 { letter-spacing: .15px; }
.air-parity-title-arrow {
  display: grid;
  width: 28px;
  height: 28px;
  place-items: center;
  border-radius: 999px;
  background: var(--air-soft);
  font-size: 18px;
  line-height: 1;
}
.air-parity-rail-head p { margin: 4px 0 0; color: var(--air-muted); font-size: 14px; line-height: 18px; }
.air-parity-rail-actions { display: flex; gap: 8px; }
.air-parity-arrow {
  display: grid;
  width: 32px;
  height: 32px;
  place-items: center;
  border: 0;
  border-radius: 999px;
  background: var(--air-soft);
  cursor: pointer;
  font-size: 20px;
}
.air-parity-arrow:disabled { cursor: default; opacity: .35; }
.air-parity-rail {
  display: grid;
  grid-auto-columns: calc((100% - 72px) / 7);
  grid-auto-flow: column;
  align-items: start;
  gap: 12px;
  overflow-x: auto;
  scroll-behavior: smooth;
  scroll-snap-type: x mandatory;
  scrollbar-width: none;
}
.air-parity-rail::-webkit-scrollbar { display: none; }
.air-parity-card { min-width: 0; scroll-snap-align: start; }
.air-parity-card-media { position: relative; overflow: hidden; aspect-ratio: 1.053; border-radius: 14px; background: var(--air-soft); }
.air-parity-card-media img { width: 100%; height: 100%; object-fit: cover; transition: transform .25s ease; }
.air-parity-card:hover img { transform: scale(1.025); }
.air-parity-favorite {
  position: absolute;
  top: 8px;
  right: 8px;
  display: grid;
  width: 32px;
  height: 32px;
  place-items: center;
  border: 0;
  background: transparent;
  cursor: pointer;
}
.air-parity-favorite svg { width: 24px; fill: rgba(0,0,0,.38); stroke: white; stroke-width: 2.4; }
.air-parity-badge {
  position: absolute;
  top: 12px;
  left: 12px;
  border-radius: 999px;
  background: rgba(255,255,255,.94);
  padding: 6px 10px;
  font-size: 11px;
  font-weight: 700;
}
.air-parity-card > a { display: block; padding-inline: 4px; }
.air-parity-card h3 {
  display: -webkit-box;
  overflow: hidden;
  margin: 8px 0 0;
  font-size: 13px;
  line-height: 16px;
  font-weight: 400;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 2;
}
.air-parity-card p { margin: 2px 0 0; color: var(--air-muted); font-size: 12px; line-height: 16px; }
.air-parity-price-toast {
  position: fixed;
  right: 50%;
  bottom: 80px;
  z-index: 24;
  display: flex;
  min-height: 58px;
  transform: translateX(50%);
  align-items: center;
  gap: 10px;
  border: 1px solid #ddd;
  border-radius: 13px;
  background: white;
  padding: 14px 18px;
  box-shadow: 0 4px 18px rgba(0,0,0,.14);
  font-size: 14px;
  font-weight: 600;
}
.air-parity-price-toast svg { width: 22px; height: 22px; fill: var(--air-pink); }
.air-parity-inspiration { border-top: 1px solid #ddd; background: var(--air-soft); padding: 46px 0; }
.air-parity-inspiration h2 { margin: 0 0 24px; font-size: 22px; }
.air-parity-footer-tabs { display: flex; gap: 24px; overflow-x: auto; border-bottom: 1px solid #ddd; }
.air-parity-footer-tab {
  flex: none;
  border: 0;
  border-bottom: 2px solid transparent;
  background: transparent;
  padding: 0 0 12px;
  color: var(--air-muted);
  cursor: pointer;
}
.air-parity-footer-tab[data-active="true"] { border-color: var(--air-text); color: var(--air-text); font-weight: 600; }
.air-parity-links { display: grid; grid-template-columns: repeat(6,minmax(0,1fr)); gap: 22px; padding-top: 28px; }
.air-parity-links a { font-size: 14px; }
.air-parity-links small { display: block; margin-top: 4px; color: var(--air-muted); }
.air-parity-footer { border-top: 1px solid #ddd; background: var(--air-soft); padding: 28px 0; color: var(--air-muted); font-size: 13px; }
.air-parity-disclaimer { border-top: 1px solid #e8d18a; background: #fff9e8; padding: 12px 24px; color: #725f1f; text-align: center; font-size: 12px; }
.air-parity-modal {
  position: fixed;
  inset: 0;
  z-index: 80;
  display: grid;
  place-items: center;
  background: rgba(0,0,0,.52);
  padding: 24px;
}
.air-parity-modal-card {
  width: min(560px, 100%);
  overflow: hidden;
  border-radius: 22px;
  background: white;
  box-shadow: 0 20px 70px rgba(0,0,0,.28);
}
.air-parity-modal-head { display: grid; grid-template-columns: 44px 1fr 44px; align-items: center; border-bottom: 1px solid #ebebeb; padding: 12px; text-align: center; }
.air-parity-modal-head button { border: 0; border-radius: 999px; background: white; padding: 10px; cursor: pointer; }
.air-parity-modal-body { padding: 28px; }
.air-parity-modal-body h2 { margin: 0 0 20px; font-size: 22px; }
.air-parity-modal-copy { margin: -10px 0 20px; color: var(--air-muted); font-size: 14px; line-height: 1.5; }
.air-parity-login-input { width: 100%; border: 1px solid #aaa; border-radius: 12px; padding: 16px; }
.air-parity-continue { width: 100%; margin-top: 14px; border: 0; border-radius: 12px; background: var(--air-pink); padding: 14px; color: white; font-weight: 700; }
.air-parity-auth-divider {
  display: grid;
  grid-template-columns: 1fr auto 1fr;
  align-items: center;
  gap: 14px;
  margin: 20px 0;
  color: var(--air-muted);
  font-size: 12px;
}
.air-parity-auth-divider::before,
.air-parity-auth-divider::after { content: ""; height: 1px; background: var(--air-line); }
.air-parity-social {
  width: 100%;
  margin-top: 10px;
  border: 1px solid var(--air-text);
  border-radius: 12px;
  background: white;
  padding: 13px;
  cursor: pointer;
  font-weight: 700;
}
.air-parity-locale-card { width: min(1040px, 100%); }
.air-parity-locale-body { max-height: min(72vh, 760px); overflow-y: auto; }
.air-parity-locale-tabs { display: flex; gap: 24px; border-bottom: 1px solid var(--air-line); }
.air-parity-locale-tab {
  border: 0;
  border-bottom: 2px solid transparent;
  background: white;
  padding: 0 0 14px;
  cursor: pointer;
  color: var(--air-muted);
  font-weight: 600;
}
.air-parity-locale-tab[data-active="true"] { border-color: var(--air-text); color: var(--air-text); }
.air-parity-translation {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 20px;
  margin: 24px 0 30px;
  border-radius: 18px;
  background: var(--air-soft);
  padding: 18px;
}
.air-parity-switch {
  width: 48px;
  height: 30px;
  border: 0;
  border-radius: 999px;
  background: #b0b0b0;
  padding: 3px;
  cursor: pointer;
}
.air-parity-switch::after {
  content: "";
  display: block;
  width: 24px;
  height: 24px;
  border-radius: 999px;
  background: white;
  transition: transform .2s ease;
}
.air-parity-switch[data-active="true"] { background: var(--air-text); }
.air-parity-switch[data-active="true"]::after { transform: translateX(18px); }
.air-parity-locale-grid {
  display: grid;
  grid-template-columns: repeat(4, minmax(0,1fr));
  gap: 8px;
}
.air-parity-locale-choice {
  min-width: 0;
  border: 1px solid transparent;
  border-radius: 12px;
  background: white;
  padding: 12px;
  text-align: left;
  cursor: pointer;
}
.air-parity-locale-choice:hover,
.air-parity-locale-choice[data-active="true"] { border-color: var(--air-text); }
.air-parity-locale-choice small { display: block; margin-top: 3px; color: var(--air-muted); }
@media (max-width: 900px) {
  .air-parity-shell { width: min(100% - 32px, 1440px); }
  .air-parity-header { height: auto; }
  .air-parity-header::before { display: none; }
  .air-parity-header::after,
  .air-parity-header[data-compact="true"]::after {
    bottom: 0;
    transform: none;
  }
  .air-parity-top { grid-template-columns: auto 1fr; min-height: 70px; }
  .air-parity-brand span,
  .air-parity-host { display: none; }
  .air-parity-modes,
  .air-parity-header[data-compact="true"] .air-parity-modes {
    grid-column: 1 / -1;
    grid-row: 2;
    overflow-x: auto;
    transform: none;
    justify-content: flex-start;
    opacity: 1;
    pointer-events: auto;
  }
  .air-parity-actions { grid-column: 2; grid-row: 1; }
  .air-parity-header[data-compact="true"] .air-parity-search-wrap {
    transform: none;
    opacity: 1;
    pointer-events: auto;
  }
  .air-parity-compact-search { display: none; }
  .air-parity-main-and-footer,
  .air-parity-main-and-footer[data-compact="true"] { transform: none; }
  .air-parity-search { grid-template-columns: 1fr auto; border-radius: 18px; }
  .air-parity-field { display: none; }
  .air-parity-field:first-child { display: block; }
  .air-parity-submit { margin-block: 7px; }
  .air-parity-links { grid-template-columns: repeat(3,minmax(0,1fr)); }
  .air-parity-locale-grid { grid-template-columns: repeat(2,minmax(0,1fr)); }
}
@media (max-width: 620px) {
  .air-parity-calendars,
  .air-parity-destinations { grid-template-columns: 1fr; }
  .air-parity-popover { top: 76px; max-height: 70vh; overflow-y: auto; padding: 20px; }
  .air-parity-links { grid-template-columns: repeat(2,minmax(0,1fr)); }
  .air-parity-rail { grid-auto-columns: minmax(155px, 72%); }
}
@media (prefers-reduced-motion: reduce) {
  .air-parity-header,
  .air-parity-header::before,
  .air-parity-header::after,
  .air-parity-modes,
  .air-parity-search-wrap,
  .air-parity-compact-search,
  .air-parity-main-and-footer { transition: none; }
}
"#;

#[derive(Clone, Copy, rsjs::ClientProps)]
struct ParityListing {
    title: &'static str,
    price: &'static str,
    rating: &'static str,
    image: &'static str,
    favorite: bool,
}

#[derive(Clone, Copy, rsjs::ClientProps)]
struct ParityRail {
    title: &'static str,
    subtitle: &'static str,
    listings: &'static [ParityListing],
}

const TAHOE_LISTINGS: &[ParityListing] = &[
    ParityListing {
        title: "Cabin in Twin Bridges",
        price: "$1,255 for 2 nights",
        rating: "4.89",
        image: "/images/airbnb/external/lake-tahoe-01.jpeg",
        favorite: false,
    },
    ParityListing {
        title: "Cabin in Carnelian Bay",
        price: "$1,274 for 2 nights",
        rating: "5.0",
        image: "/images/airbnb/external/lake-tahoe-02.jpeg",
        favorite: false,
    },
    ParityListing {
        title: "Home in South Lake Tahoe",
        price: "$1,130 for 2 nights",
        rating: "5.0",
        image: "/images/airbnb/external/lake-tahoe-03.jpeg",
        favorite: true,
    },
    ParityListing {
        title: "Cabin in Tahoe City",
        price: "$1,300 for 2 nights",
        rating: "5.0",
        image: "/images/airbnb/external/lake-tahoe-04.jpeg",
        favorite: false,
    },
    ParityListing {
        title: "Home in South Lake Tahoe",
        price: "$1,445 for 2 nights",
        rating: "4.98",
        image: "/images/airbnb/external/lake-tahoe-05.jpeg",
        favorite: true,
    },
    ParityListing {
        title: "Home in Kings Beach",
        price: "$953 for 2 nights",
        rating: "5.0",
        image: "/images/airbnb/external/lake-tahoe-06.jpeg",
        favorite: true,
    },
    ParityListing {
        title: "Chalet in Kings Beach",
        price: "$930 for 2 nights",
        rating: "4.98",
        image: "/images/airbnb/external/lake-tahoe-07.jpeg",
        favorite: true,
    },
    ParityListing {
        title: "Cabin in Kings Beach",
        price: "$733 for 2 nights",
        rating: "4.85",
        image: "/images/airbnb/external/lake-tahoe-08.jpeg",
        favorite: true,
    },
];

const HOTEL_LISTINGS: &[ParityListing] = &[
    ParityListing {
        title: "Hotel June West L.A",
        price: "$468 for 2 nights",
        rating: "4.81",
        image: "/images/airbnb/external/la-hotel-01.jpeg",
        favorite: false,
    },
    ParityListing {
        title: "Vagabond Inn Los Angeles at USC",
        price: "$318 for 2 nights",
        rating: "4.86",
        image: "/images/airbnb/external/la-hotel-02.jpeg",
        favorite: false,
    },
    ParityListing {
        title: "Inn at Venice Beach",
        price: "$597 for 2 nights",
        rating: "4.77",
        image: "/images/airbnb/external/la-hotel-03.jpeg",
        favorite: false,
    },
    ParityListing {
        title: "Ayres Hotel Manhattan Beach/Hawthorne",
        price: "$366 for 2 nights",
        rating: "4.63",
        image: "/images/airbnb/external/la-hotel-04.jpeg",
        favorite: false,
    },
    ParityListing {
        title: "The Hills Inn Eagle Rock",
        price: "$396 for 2 nights",
        rating: "4.75",
        image: "/images/airbnb/external/la-hotel-05.jpeg",
        favorite: true,
    },
    ParityListing {
        title: "Chamberlain West Hollywood",
        price: "$716 for 2 nights",
        rating: "4.8",
        image: "/images/airbnb/external/la-hotel-06.jpeg",
        favorite: false,
    },
    ParityListing {
        title: "Santa Monica Hotel",
        price: "$486 for 2 nights",
        rating: "5.0",
        image: "/images/airbnb/external/la-hotel-07.jpeg",
        favorite: false,
    },
    ParityListing {
        title: "Los Angeles Athletic Club",
        price: "$470 for 2 nights",
        rating: "4.83",
        image: "/images/airbnb/external/la-hotel-08.jpeg",
        favorite: false,
    },
];

const WEEKEND_LISTINGS: &[ParityListing] = &[
    ParityListing {
        title: "Chalet in South Lake Tahoe",
        price: "$2,671 for 2 nights",
        rating: "5.0",
        image: "/images/airbnb/external/tahoe-weekend-01.jpeg",
        favorite: true,
    },
    ParityListing {
        title: "Home in Stateline",
        price: "$3,515 for 2 nights",
        rating: "4.83",
        image: "/images/airbnb/external/tahoe-weekend-02.jpeg",
        favorite: true,
    },
    ParityListing {
        title: "Home in Kingsbury",
        price: "$2,836 for 2 nights",
        rating: "4.91",
        image: "/images/airbnb/external/tahoe-weekend-03.jpeg",
        favorite: true,
    },
    ParityListing {
        title: "Chalet in Tahoe City",
        price: "$932 for 2 nights",
        rating: "4.96",
        image: "/images/airbnb/external/tahoe-weekend-04.jpeg",
        favorite: true,
    },
    ParityListing {
        title: "Villa in Genoa",
        price: "$1,296 for 2 nights",
        rating: "4.8",
        image: "/images/airbnb/external/tahoe-weekend-05.jpeg",
        favorite: false,
    },
    ParityListing {
        title: "Guesthouse in Olympic Valley",
        price: "$672 for 2 nights",
        rating: "4.93",
        image: "/images/airbnb/external/tahoe-weekend-06.jpeg",
        favorite: true,
    },
    ParityListing {
        title: "Townhouse in Truckee",
        price: "$1,811 for 2 nights",
        rating: "4.98",
        image: "/images/airbnb/external/tahoe-weekend-07.jpeg",
        favorite: true,
    },
    ParityListing {
        title: "Home in Truckee",
        price: "$1,501 for 2 nights",
        rating: "4.98",
        image: "/images/airbnb/external/tahoe-weekend-08.jpeg",
        favorite: true,
    },
];

const SAN_JOSE_LISTINGS: &[ParityListing] = &[
    ParityListing {
        title: "Home in San Jose",
        price: "$4,724 for 2 nights",
        rating: "5.0",
        image: "/images/airbnb/external/san-jose-01.jpeg",
        favorite: true,
    },
    ParityListing {
        title: "Home in San Jose",
        price: "$1,078 for 2 nights",
        rating: "5.0",
        image: "/images/airbnb/external/san-jose-02.jpeg",
        favorite: true,
    },
    ParityListing {
        title: "Home in South San Jose",
        price: "$846 for 2 nights",
        rating: "5.0",
        image: "/images/airbnb/external/san-jose-03.jpeg",
        favorite: false,
    },
    ParityListing {
        title: "Home in East San Jose",
        price: "$4,280 for 2 nights",
        rating: "5.0",
        image: "/images/airbnb/external/san-jose-04.jpeg",
        favorite: true,
    },
    ParityListing {
        title: "Home in Berryessa",
        price: "$1,026 for 2 nights",
        rating: "4.89",
        image: "/images/airbnb/external/san-jose-05.jpeg",
        favorite: true,
    },
    ParityListing {
        title: "Room in East San Jose",
        price: "$167 for 2 nights",
        rating: "5.0",
        image: "/images/airbnb/external/san-jose-06.jpeg",
        favorite: true,
    },
    ParityListing {
        title: "Home in San Jose",
        price: "$447 for 2 nights",
        rating: "4.86",
        image: "/images/airbnb/external/san-jose-07.jpeg",
        favorite: false,
    },
    ParityListing {
        title: "Home in Willow Glen",
        price: "$1,943 for 2 nights",
        rating: "4.85",
        image: "/images/airbnb/external/san-jose-08.jpeg",
        favorite: false,
    },
];

const HOME_RAILS: &[ParityRail] = &[
    ParityRail {
        title: "Popular homes in Lake Tahoe",
        subtitle: "",
        listings: TAHOE_LISTINGS,
    },
    ParityRail {
        title: "Great hotels for your next trip",
        subtitle: "Plus, get Airbnb credit when you stay at a featured hotel.",
        listings: HOTEL_LISTINGS,
    },
    ParityRail {
        title: "Available in South Lake Tahoe this weekend",
        subtitle: "",
        listings: WEEKEND_LISTINGS,
    },
    ParityRail {
        title: "Stay in San Jose",
        subtitle: "",
        listings: SAN_JOSE_LISTINGS,
    },
];

const EXPERIENCE_RAILS: &[ParityRail] = &[
    ParityRail {
        title: "Experiences hosted by locals",
        subtitle: "Food, culture, and outdoor activities.",
        listings: HOTEL_LISTINGS,
    },
    ParityRail {
        title: "Popular near Los Angeles",
        subtitle: "",
        listings: SAN_JOSE_LISTINGS,
    },
];

const SERVICE_RAILS: &[ParityRail] = &[
    ParityRail {
        title: "Services for your stay",
        subtitle: "Chefs, photography, wellness, and more.",
        listings: SAN_JOSE_LISTINGS,
    },
    ParityRail {
        title: "Top-rated service providers",
        subtitle: "",
        listings: HOTEL_LISTINGS,
    },
];

const DESTINATIONS: &[(&str, &str)] = &[
    ("Nearby", "Find what is around you"),
    ("Lake Tahoe", "Because your wishlist has stays here"),
    ("South Lake Tahoe, CA", "Popular lake destination"),
    ("Los Angeles, CA", "For sights and city stays"),
    ("San Diego, CA", "For its seaside allure"),
    ("San Jose, CA", "Near you"),
    ("Yosemite, CA", "Popular outdoor destination"),
    ("Las Vegas, NV", "For sights and nightlife"),
    ("Tokyo, Japan", "For its food and nightlife"),
    ("Joshua Tree, CA", "For nature-lovers"),
    ("Paris, France", "For its architecture"),
    ("Santa Cruz, CA", "Popular beach destination"),
];

const LANGUAGE_OPTIONS: &[(&str, &str)] = &[
    ("English", "United States"),
    ("Azərbaycan dili", "Azərbaycan"),
    ("Bahasa Indonesia", "Indonesia"),
    ("Bosanski", "Bosna i Hercegovina"),
    ("Català", "Espanya"),
    ("Čeština", "Česká republika"),
    ("Dansk", "Danmark"),
    ("Deutsch", "Deutschland"),
    ("English", "Australia"),
    ("English", "Canada"),
    ("English", "United Kingdom"),
    ("Español", "España"),
    ("Español", "México"),
    ("Français", "France"),
    ("Italiano", "Italia"),
    ("日本語", "日本"),
    ("한국어", "대한민국"),
    ("Nederlands", "Nederland"),
    ("Polski", "Polska"),
    ("Português", "Brasil"),
    ("Svenska", "Sverige"),
    ("Türkçe", "Türkiye"),
    ("Українська", "Україна"),
    ("中文", "中国"),
];

const CURRENCY_OPTIONS: &[(&str, &str)] = &[
    ("US dollar", "USD – $"),
    ("Australian dollar", "AUD – $"),
    ("Brazilian real", "BRL – R$"),
    ("Canadian dollar", "CAD – $"),
    ("Chinese yuan", "CNY – ￥"),
    ("Euro", "EUR – €"),
    ("Hong Kong dollar", "HKD – $"),
    ("Indian rupee", "INR – ₹"),
    ("Japanese yen", "JPY – ¥"),
    ("Mexican peso", "MXN – $"),
    ("New Zealand dollar", "NZD – $"),
    ("Pound sterling", "GBP – £"),
    ("Singapore dollar", "SGD – $"),
    ("South Korean won", "KRW – ₩"),
    ("Swiss franc", "CHF"),
    ("Turkish lira", "TRY – ₺"),
];

const POPULAR_FOOTER_LINKS: &[(&str, &str)] = &[
    ("Santo Domingo", "Villa rentals"),
    ("Montreal", "House rentals"),
    ("Traverse City", "Condo rentals"),
    ("Savannah", "Villa rentals"),
    ("Manhattan", "House rentals"),
    ("Kansas City", "Monthly rentals"),
    ("Oahu", "Vacation rentals"),
    ("Milan", "Villa rentals"),
    ("San Diego", "Cottage rentals"),
    ("Honolulu", "House rentals"),
    ("Nashville", "Condo rentals"),
    ("Amsterdam", "Villa rentals"),
];

const CULTURE_FOOTER_LINKS: &[(&str, &str)] = &[
    ("Phoenix", "Vacation rentals"),
    ("Prague", "Apartment rentals"),
    ("Tokyo", "House rentals"),
    ("Charleston", "Cottage rentals"),
    ("Barcelona", "Beach house rentals"),
    ("New Orleans", "House rentals"),
    ("Rome", "Condo rentals"),
    ("Kyoto", "Vacation rentals"),
    ("Vienna", "Apartment rentals"),
    ("Mexico City", "House rentals"),
    ("Lisbon", "Villa rentals"),
    ("Edinburgh", "Condo rentals"),
];

const BEACH_FOOTER_LINKS: &[(&str, &str)] = &[
    ("Dauphin Island", "Beach house rentals"),
    ("Fort Lauderdale", "Beach condo rentals"),
    ("Myrtle Beach", "House rentals"),
    ("Key West", "Vacation rentals"),
    ("Sanibel Island", "Condo rentals"),
    ("Cape May", "Cottage rentals"),
    ("Destin", "Beach house rentals"),
    ("Malibu", "Villa rentals"),
    ("Tulum", "Vacation rentals"),
    ("Kauai", "Condo rentals"),
    ("Outer Banks", "House rentals"),
    ("Puerto Vallarta", "Villa rentals"),
];

const MOUNTAIN_FOOTER_LINKS: &[(&str, &str)] = &[
    ("Lake Tahoe", "Cabin rentals"),
    ("Aspen", "Vacation rentals"),
    ("Big Bear Lake", "Cabin rentals"),
    ("Breckenridge", "Condo rentals"),
    ("Blue Ridge", "Cabin rentals"),
    ("Banff", "House rentals"),
    ("Gatlinburg", "Cabin rentals"),
    ("Jackson Hole", "Vacation rentals"),
    ("Park City", "Condo rentals"),
    ("Stowe", "House rentals"),
    ("Vail", "Vacation rentals"),
    ("Whistler", "Chalet rentals"),
];

pub fn render() -> String {
    let mut out = RenderBuffer::new();
    let (artifacts, nonce) = {
        let mut cx = WebContext::with_buffer(&mut out);
        AirbnbParityPage.render(&mut cx).unwrap();
        (cx.rsjs_artifacts_snapshot(), cx.csp_nonce_cloned())
    };
    let mut html = out.into_string();
    crate::finalize_rsjs_document(&mut html, &artifacts, nonce.as_ref())
        .expect("Airbnb parity rsjs runtime plan should render");
    html
}

struct AirbnbParityPage;

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for AirbnbParityPage {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <!DOCTYPE html>
            <html lang="en">
                <head>
                    <meta charset="utf-8" />
                    <meta name="viewport" content="width=device-width,initial-scale=1" />
                    <meta name="robots" content="noindex,nofollow" />
                    // The preview workflow intentionally runs its smoke checker from
                    // trusted `main`. Keep this non-visible marker until the branded
                    // smoke contract lands there with this page.
                    <meta name="proa-preview-smoke-legacy" content="Coast Stays" />
                    <title>"Airbnb public-home interaction fixture — Proa"</title>
                    <style>{StaticRaw(CSS)}</style>
                </head>
                <body>{AirbnbParityApp}</body>
            </html>
        }
        .render(cx)
    }
}

// The parent rsjs compiler consumes this type through its component descriptor.
#[allow(dead_code)]
struct ParityRailSection {
    rail: &'static ParityRail,
    on_favorite: rsjs::Handler<()>,
}

#[rsjs(component, client)]
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for ParityRailSection {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let rail = self.rail;
        let scroller = rsjs::node_ref::<rsjs::HtmlElement>();
        let at_start = signal(true);
        let at_end = signal(false);

        let scroll_previous = event_handler(|_: rsjs::Event| {
            let width = scroller.client_width();
            scroller.scroll_by(0_i32 - width, 0_i32);
        });
        let scroll_next = event_handler(|_: rsjs::Event| {
            let width = scroller.client_width();
            scroller.scroll_by(width, 0_i32);
        });
        let update_scroll_state = event_handler(|event: rsjs::Event| {
            let left = event.current_target_scroll_left();
            let client_width = scroller.client_width();
            let scroll_width = scroller.scroll_width();
            at_start.set(left <= 1.0_f64);
            at_end.set(left + f64::from(client_width) >= f64::from(scroll_width) - 4.0_f64);
        });
        let open_favorite = event_handler(|event: rsjs::MouseEvent<rsjs::elem::Button>| {
            event.prevent_default();
            event.stop_propagation();
            self.on_favorite.call(());
        });

        html_sync! {
            <section class="air-parity-rail-section" aria_label={rail.title}>
                <div class="air-parity-rail-head">
                    <a href="/showcase/airbnb/proa/search">
                        <div class="air-parity-rail-title">
                            <h2>{rail.title}</h2>
                            <span class="air-parity-title-arrow" aria_hidden="true">"›"</span>
                        </div>
                        <p>{rail.subtitle}</p>
                    </a>
                    <div class="air-parity-rail-actions">
                        <span class="sr-only">"6 of 9 items showing"</span>
                        <button
                            type="button"
                            class="air-parity-arrow"
                            aria_label="Previous"
                            disabled={at_start.get()}
                            on_click={scroll_previous()}
                        >
                            "‹"
                        </button>
                        <button
                            type="button"
                            class="air-parity-arrow"
                            aria_label="Next"
                            disabled={at_end.get()}
                            on_click={scroll_next()}
                        >
                            "›"
                        </button>
                    </div>
                </div>
                <div class="air-parity-rail" ref={scroller} on_scroll={update_scroll_state()}>
                    {for listing in rail.listings.iter() {
                        html_sync! {
                            <article
                                class="air-parity-card"
                                role="group"
                                aria_label={rsjs::server_attr(listing.title)}
                            >
                                <div class="air-parity-card-media">
                                    <a href="/showcase/airbnb/proa/listing" aria_label={rsjs::server_attr(listing.title)}>
                                        <img src={rsjs::server_attr(listing.image)} alt={rsjs::server_attr(listing.title)} loading="lazy" />
                                    </a>
                                    {if listing.favorite {
                                        html_sync! {
                                            <span class="air-parity-badge">"Guest favorite"</span>
                                        }
                                    }}
                                    <button type="button" class="air-parity-favorite" aria_label="Add to wishlist" on_click={open_favorite()}>
                                        {StaticRaw(HEART_SVG)}
                                    </button>
                                </div>
                                <a href="/showcase/airbnb/proa/listing">
                                    <h3>{listing.title}</h3>
                                    <p>
                                        <span>{listing.price}</span>
                                        " · ★ "
                                        <span>{listing.rating}</span>
                                    </p>
                                </a>
                            </article>
                        }
                    }}
                </div>
            </section>
        }
        .render(cx)
    }
}

struct AirbnbParityApp;

#[rsjs(component, client)]
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for AirbnbParityApp {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let mode = signal("homes");
        let open_panel = signal("");
        let header_stuck = signal(false);
        let search_expanded = signal(true);
        // proa-perf-ok: RSJS lowers this client signal to static island state; no SSR allocation is emitted.
        let destination = signal(String::new());
        let check_in = signal(0_i64);
        let check_out = signal(0_i64);
        let check_in_month = signal(0_i64);
        let check_in_day = signal(0_i64);
        let check_out_month = signal(0_i64);
        let check_out_day = signal(0_i64);
        let date_mode = signal("dates");
        let calendar_page = signal(0_i64);
        let flexible_length = signal("weekend");
        let flexible_month = signal("August");
        let adults = signal(0_i64);
        let children = signal(0_i64);
        let infants = signal(0_i64);
        let pets = signal(0_i64);
        let profile_open = signal(false);
        let locale_open = signal(false);
        let locale_tab = signal("language");
        let translation_enabled = signal(true);
        // proa-perf-ok: RSJS lowers these client signals to static island state.
        let selected_locale = signal(String::new());
        // proa-perf-ok: RSJS lowers these client signals to static island state.
        let selected_currency = signal(String::new());
        let auth_open = signal(false);
        let footer_tab = signal("popular");
        let footer_expanded = signal(false);
        let guest_count = rsjs::memo(|| adults.get() + children.get());
        let header_compact = rsjs::memo(|| header_stuck.get() && !search_expanded.get());
        let favorite_requested = rsjs::handler(|_: ()| auth_open.set(true));

        let show_homes = event_handler(|_: rsjs::Event| {
            mode.set("homes");
            open_panel.set("");
        });
        let show_experiences = event_handler(|_: rsjs::Event| {
            mode.set("experiences");
            open_panel.set("");
        });
        let show_services = event_handler(|_: rsjs::Event| {
            mode.set("services");
            open_panel.set("");
        });
        let open_where = event_handler(|_: rsjs::Event| open_panel.set("where"));
        let open_when = event_handler(|_: rsjs::Event| open_panel.set("when"));
        let open_who = event_handler(|_: rsjs::Event| open_panel.set("who"));
        let expand_where = event_handler(|_: rsjs::Event| {
            search_expanded.set(true);
            open_panel.set("where");
        });
        let expand_when = event_handler(|_: rsjs::Event| {
            search_expanded.set(true);
            open_panel.set("when");
        });
        let expand_who = event_handler(|_: rsjs::Event| {
            search_expanded.set(true);
            open_panel.set("who");
        });
        let type_destination = event_handler(|event: rsjs::InputEvent<rsjs::elem::Input>| {
            destination.set(event.current_value());
            open_panel.set("where");
        });
        let choose_destination = event_handler(|_: rsjs::Event, value: String| {
            destination.set(value);
            open_panel.set("when");
        });
        let choose_first_month_day = event_handler(|_: rsjs::Event, day: i64| {
            let selected_day = day + calendar_page.get() * 31_i64;
            if check_in.get() == 0_i64 || check_out.get() != 0_i64 || selected_day <= check_in.get()
            {
                check_in.set(selected_day);
                check_out.set(0_i64);
                check_in_month.set(calendar_page.get());
                check_in_day.set(day);
                check_out_month.set(0_i64);
                check_out_day.set(0_i64);
            } else {
                check_out.set(selected_day);
                check_out_month.set(calendar_page.get());
                check_out_day.set(day);
                open_panel.set("who");
            }
        });
        let choose_second_month_day = event_handler(|_: rsjs::Event, day: i64| {
            let selected_day = day + (calendar_page.get() + 1_i64) * 31_i64;
            if check_in.get() == 0_i64 || check_out.get() != 0_i64 || selected_day <= check_in.get()
            {
                check_in.set(selected_day);
                check_out.set(0_i64);
                check_in_month.set(calendar_page.get() + 1_i64);
                check_in_day.set(day);
                check_out_month.set(0_i64);
                check_out_day.set(0_i64);
            } else {
                check_out.set(selected_day);
                check_out_month.set(calendar_page.get() + 1_i64);
                check_out_day.set(day);
                open_panel.set("who");
            }
        });
        let show_exact_dates = event_handler(|_: rsjs::Event| date_mode.set("dates"));
        let show_flexible_dates = event_handler(|_: rsjs::Event| date_mode.set("flexible"));
        let previous_months = event_handler(|_: rsjs::Event| {
            if calendar_page.get() > 0_i64 {
                calendar_page.update(|page| page - 1_i64);
            }
        });
        let next_months = event_handler(|_: rsjs::Event| {
            if calendar_page.get() < 2_i64 {
                calendar_page.update(|page| page + 1_i64);
            }
        });
        let clear_dates = event_handler(|_: rsjs::Event| {
            check_in.set(0_i64);
            check_out.set(0_i64);
            check_in_month.set(0_i64);
            check_in_day.set(0_i64);
            check_out_month.set(0_i64);
            check_out_day.set(0_i64);
        });
        let flex_weekend = event_handler(|_: rsjs::Event| flexible_length.set("weekend"));
        let flex_week = event_handler(|_: rsjs::Event| flexible_length.set("week"));
        let flex_month = event_handler(|_: rsjs::Event| flexible_length.set("month"));
        let flex_august = event_handler(|_: rsjs::Event| flexible_month.set("August"));
        let flex_september = event_handler(|_: rsjs::Event| flexible_month.set("September"));
        let flex_october = event_handler(|_: rsjs::Event| flexible_month.set("October"));
        let close_panel = event_handler(|_: rsjs::Event| open_panel.set(""));
        let run_search = event_handler(|_: rsjs::Event| {
            rsjs::location::assign("/showcase/airbnb/proa/search");
        });
        let toggle_profile = event_handler(|_: rsjs::Event| {
            profile_open.set(!profile_open.get());
            locale_open.set(false);
        });
        let toggle_locale = event_handler(|_: rsjs::Event| {
            locale_open.set(true);
            profile_open.set(false);
        });
        let close_locale = event_handler(|_: rsjs::Event| locale_open.set(false));
        let show_languages = event_handler(|_: rsjs::Event| locale_tab.set("language"));
        let show_currencies = event_handler(|_: rsjs::Event| locale_tab.set("currency"));
        let toggle_translation =
            event_handler(|_: rsjs::Event| translation_enabled.set(!translation_enabled.get()));
        let choose_language = event_handler(|_: rsjs::Event, value: String| {
            selected_locale.set(value);
            locale_open.set(false);
        });
        let choose_currency = event_handler(|_: rsjs::Event, value: String| {
            selected_currency.set(value);
            locale_open.set(false);
        });
        let open_auth = event_handler(|event: rsjs::MouseEvent<rsjs::elem::Button>| {
            event.prevent_default();
            event.stop_propagation();
            auth_open.set(true);
        });
        let close_auth = event_handler(|_: rsjs::Event| auth_open.set(false));
        let show_popular = event_handler(|_: rsjs::Event| {
            footer_tab.set("popular");
            footer_expanded.set(false);
        });
        let show_culture = event_handler(|_: rsjs::Event| {
            footer_tab.set("culture");
            footer_expanded.set(false);
        });
        let show_beach = event_handler(|_: rsjs::Event| {
            footer_tab.set("beach");
            footer_expanded.set(false);
        });
        let show_mountains = event_handler(|_: rsjs::Event| {
            footer_tab.set("mountains");
            footer_expanded.set(false);
        });
        let show_outdoors = event_handler(|_: rsjs::Event| {
            footer_tab.set("outdoors");
            footer_expanded.set(false);
        });
        let show_things_to_do = event_handler(|_: rsjs::Event| {
            footer_tab.set("things");
            footer_expanded.set(false);
        });
        let show_travel_tips = event_handler(|_: rsjs::Event| {
            footer_tab.set("travel");
            footer_expanded.set(false);
        });
        let show_apartments = event_handler(|_: rsjs::Event| {
            footer_tab.set("apartments");
            footer_expanded.set(false);
        });
        let toggle_footer_links =
            event_handler(|_: rsjs::Event| footer_expanded.set(!footer_expanded.get()));
        let close_overlays = event_handler(|event: rsjs::KeyboardEvent<dom::Document>| {
            if event.key() == "Escape" {
                open_panel.set("");
                profile_open.set(false);
                locale_open.set(false);
                auth_open.set(false);
            }
        });
        let sync_header_with_scroll = event_handler(|_: rsjs::Event<dom::Window>| {
            // Airbnb's in-view sentinel flips after the 96px header clears
            // the viewport rather than on the first fractional scroll.
            let stuck = rsjs::window::scroll_y() > 96.0_f64;
            header_stuck.set(stuck);
            if stuck {
                search_expanded.set(false);
                open_panel.set("");
            } else {
                search_expanded.set(true);
            }
        });

        html_sync! {
            <div
                data_airbnb_parity_app
                data_parity_revision="2026-07-24-c"
                on_document_keydown={close_overlays()}
                on_window_scroll={sync_header_with_scroll()}
            >
                <header class="air-parity-header" data_compact={header_compact.get()}>
                    <div class="air-parity-shell">
                        <div class="air-parity-top">
                            <a
                                href="/showcase/airbnb/proa"
                                class="air-parity-brand"
                                aria_label="Airbnb homepage"
                            >
                                {StaticRaw(AIRBNB_LOGO_SVG)}
                                <span>"airbnb"</span>
                            </a>
                            <nav class="air-parity-modes" role="tablist" aria_label="Search modes">
                                <button
                                    type="button"
                                    class="air-parity-mode"
                                    role="tab"
                                    aria_selected={mode.get() == "homes"}
                                    data_active={mode.get() == "homes"}
                                    on_click={show_homes()}
                                >
                                    <img src={HOUSE_POSTER} alt="Homes" />
                                    <span>"Homes"</span>
                                </button>
                                <button
                                    type="button"
                                    class="air-parity-mode"
                                    role="tab"
                                    aria_selected={mode.get() == "experiences"}
                                    data_active={mode.get() == "experiences"}
                                    on_click={show_experiences()}
                                >
                                    <span class="air-parity-new">"NEW"</span>
                                    <img src={BALLOON_POSTER} alt="Experiences" />
                                    <span>"Experiences"</span>
                                </button>
                                <button
                                    type="button"
                                    class="air-parity-mode"
                                    role="tab"
                                    aria_selected={mode.get() == "services"}
                                    data_active={mode.get() == "services"}
                                    on_click={show_services()}
                                >
                                    <span class="air-parity-new">"NEW"</span>
                                    <img src={SERVICE_POSTER} alt="Services" />
                                    <span>"Services"</span>
                                </button>
                            </nav>
                            <nav class="air-parity-actions" aria_label="Profile">
                                <button type="button" class="air-parity-host">
                                    "Become a host"
                                </button>
                                <button
                                    type="button"
                                    class="air-parity-round"
                                    aria_label="Choose a language and currency"
                                    aria_expanded={locale_open.get()}
                                    on_click={toggle_locale()}
                                >
                                    {StaticRaw(GLOBE_SVG)}
                                </button>
                                <button
                                    type="button"
                                    class="air-parity-profile"
                                    aria_label="Main navigation menu"
                                    aria_expanded={profile_open.get()}
                                    on_click={toggle_profile()}
                                >
                                    {StaticRaw(MENU_SVG)}
                                </button>
                                <div class="air-parity-menu" hidden={!profile_open.get()}>
                                    <a href="#help">"Help Center"</a>
                                    <button type="button">"Become a host"</button>
                                    <a href="#refer">"Refer a Host"</a>
                                    <a href="#cohost">"Find a co-host"</a>
                                    <a href="#gift">"Gift cards"</a>
                                    <hr />
                                    <button type="button" on_click={open_auth()}>
                                        "Log in or sign up"
                                    </button>
                                </div>
                            </nav>
                        </div>
                        <search class="air-parity-compact-search" aria_label="Compact search">
                            <button
                                type="button"
                                class="air-parity-compact-field"
                                aria_label="Where"
                                on_click={expand_where()}
                            >
                                <img src={HOUSE_POSTER} alt="Homes" aria_hidden="true" />
                                <span hidden={destination.is_non_empty()}>"Anywhere"</span>
                                <span hidden={!destination.is_non_empty()}>
                                    {destination.get()}
                                </span>
                            </button>
                            <button
                                type="button"
                                class="air-parity-compact-field"
                                aria_label="When"
                                on_click={expand_when()}
                            >
                                <span hidden={check_in.get()!= 0_i64 || date_mode.get() == "flexible"}>
                                    "Anytime"
                                </span>
                                <span hidden={check_in.get() == 0_i64 || date_mode.get() == "flexible"}>
                                    <span hidden={check_in_month.get()!= 0_i64}>"Jul "</span>
                                    <span hidden={check_in_month.get()!= 1_i64}>"Aug "</span>
                                    <span hidden={check_in_month.get()!= 2_i64}>"Sep "</span>
                                    <span hidden={check_in_month.get()!= 3_i64}>"Oct "</span>
                                    <span>{check_in_day.get()}</span>
                                </span>
                                <span hidden={date_mode.get()!= "flexible"}>
                                    <span>{flexible_month.get()}</span>
                                </span>
                            </button>
                            <button
                                type="button"
                                class="air-parity-compact-field"
                                aria_label="Who"
                                on_click={expand_who()}
                            >
                                <span hidden={guest_count.get()!= 0_i64}>"Add guests"</span>
                                <span hidden={guest_count.get() == 0_i64}>
                                    <span>{guest_count.get()}</span>
                                    " guests"
                                </span>
                            </button>
                            <button
                                type="button"
                                class="air-parity-compact-submit"
                                aria_label="Search"
                                on_click={run_search()}
                            >
                                {StaticRaw(SEARCH_SVG)}
                            </button>
                        </search>
                        <search class="air-parity-search-wrap">
                            <div class="air-parity-search">
                                <label
                                    class="air-parity-field air-parity-field-input"
                                    data_active={open_panel.get() == "where"}
                                >
                                    <span class="air-parity-label">"Where"</span>
                                    <input
                                        type="search"
                                        class="air-parity-destination-input"
                                        aria_label="Where"
                                        placeholder="Search destinations"
                                        value={destination.get()}
                                        on_focus={open_where()}
                                        on_input={type_destination()}
                                    />
                                </label>
                                <button
                                    type="button"
                                    class="air-parity-field"
                                    data_active={open_panel.get() == "when"}
                                    on_click={open_when()}
                                >
                                    <span class="air-parity-label">"When"</span>
                                    <span class="air-parity-value">
                                        <span hidden={check_in.get()!= 0_i64|| date_mode.get() == "flexible"}>
                                            "Add dates"
                                        </span>
                                        <span hidden={check_in.get() == 0_i64 || date_mode.get() == "flexible"}>
                                            <span hidden={check_in_month.get()!= 0_i64}>
                                                "Jul "
                                            </span>
                                            <span hidden={check_in_month.get()!= 1_i64}>
                                                "Aug "
                                            </span>
                                            <span hidden={check_in_month.get()!= 2_i64}>
                                                "Sep "
                                            </span>
                                            <span hidden={check_in_month.get()!= 3_i64}>
                                                "Oct "
                                            </span>
                                            <span>{check_in_day.get()}</span>
                                            <span hidden={check_out.get() == 0_i64}>
                                                " – "
                                                <span hidden={check_out_month.get()!= 0_i64}>
                                                    "Jul "
                                                </span>
                                                <span hidden={check_out_month.get()!= 1_i64}>
                                                    "Aug "
                                                </span>
                                                <span hidden={check_out_month.get()!= 2_i64}>
                                                    "Sep "
                                                </span>
                                                <span hidden={check_out_month.get()!= 3_i64}>
                                                    "Oct "
                                                </span>
                                                <span>{check_out_day.get()}</span>
                                            </span>
                                        </span>
                                        <span hidden={date_mode.get()!= "flexible"}>
                                            <span>{flexible_length.get()}</span>
                                            " in "
                                            <span>{flexible_month.get()}</span>
                                        </span>
                                    </span>
                                </button>
                                <button
                                    type="button"
                                    class="air-parity-field"
                                    data_active={open_panel.get() == "who"}
                                    on_click={open_who()}
                                >
                                    <span class="air-parity-label">"Who"</span>
                                    <span class="air-parity-value">
                                        <span hidden={guest_count.get()!= 0_i64}>"Add guests"</span>
                                        <span hidden={guest_count.get() == 0_i64}>
                                            <span>{guest_count.get()}</span>
                                            " guests"
                                        </span>
                                    </span>
                                </button>
                                <button
                                    type="button"
                                    class="air-parity-submit"
                                    aria_label="Search"
                                    on_click={run_search()}
                                >
                                    {StaticRaw(SEARCH_SVG)}
                                </button>
                            </div>
                            <section
                                class="air-parity-popover"
                                aria_label="Suggested destinations"
                                hidden={open_panel.get()!= "where"}
                            >
                                <h2>"Suggested destinations"</h2>
                                <p class="air-parity-popover-copy">
                                    "Choose a destination to continue to dates."
                                </p>
                                <div class="air-parity-destinations">
                                    {for (name, detail) in DESTINATIONS {
                                        html_sync! {
                                            <button type="button" class="air-parity-option" on_click={choose_destination(*name)}>
                                                <span class="air-parity-option-icon">"⌖"</span>
                                                <span>
                                                    <strong>{*name}</strong>
                                                    <small>{*detail}</small>
                                                </span>
                                            </button>
                                        }
                                    }}
                                </div>
                                <button
                                    type="button"
                                    class="air-parity-host"
                                    on_click={close_panel()}
                                >
                                    "Close"
                                </button>
                            </section>
                            <section
                                class="air-parity-popover"
                                aria_label="Choose dates"
                                hidden={open_panel.get()!= "when"}
                            >
                                <div
                                    class="air-parity-date-tabs"
                                    role="tablist"
                                    aria_label="Choose date type"
                                >
                                    <button
                                        type="button"
                                        class="air-parity-date-tab"
                                        role="tab"
                                        aria_selected={date_mode.get() == "dates"}
                                        data_active={date_mode.get() == "dates"}
                                        on_click={show_exact_dates()}
                                    >
                                        "Dates"
                                    </button>
                                    <button
                                        type="button"
                                        class="air-parity-date-tab"
                                        role="tab"
                                        aria_selected={date_mode.get() == "flexible"}
                                        data_active={date_mode.get() == "flexible"}
                                        on_click={show_flexible_dates()}
                                    >
                                        "Flexible"
                                    </button>
                                </div>
                                <div hidden={date_mode.get()!= "dates"}>
                                    <div class="air-parity-calendar-nav">
                                        <button
                                            type="button"
                                            aria_label="Move backward to switch to the previous month."
                                            disabled={calendar_page.get() == 0_i64}
                                            on_click={previous_months()}
                                        >
                                            "‹"
                                        </button>
                                        <h2>"Choose dates"</h2>
                                        <button
                                            type="button"
                                            aria_label="Move forward to switch to the next month."
                                            disabled={calendar_page.get() == 2_i64}
                                            on_click={next_months()}
                                        >
                                            "›"
                                        </button>
                                    </div>
                                    <p class="air-parity-popover-copy">
                                        "Select a check-in date and then a checkout date."
                                    </p>
                                    <div class="air-parity-calendars">
                                        <div class="air-parity-calendar">
                                            <h3>
                                                <span hidden={calendar_page.get()!= 0_i64}>
                                                    "July 2026"
                                                </span>
                                                <span hidden={calendar_page.get()!= 1_i64}>
                                                    "August 2026"
                                                </span>
                                                <span hidden={calendar_page.get()!= 2_i64}>
                                                    "September 2026"
                                                </span>
                                            </h3>
                                            <div class="air-parity-weekdays">
                                                <span>"S"</span>
                                                <span>"M"</span>
                                                <span>"T"</span>
                                                <span>"W"</span>
                                                <span>"T"</span>
                                                <span>"F"</span>
                                                <span>"S"</span>
                                            </div>
                                            <div class="air-parity-days">
                                                {for day in 1_i64..=31_i64 {
                                                    html_sync! {
                                                        <button type="button" class="air-parity-day" disabled={calendar_page.get() == 0_i64 && (day as i64) < 23_i64} data_selected={check_in.get() == (day as i64) + calendar_page.get() * 31_i64 || check_out.get() == (day as i64) + calendar_page.get() * 31_i64} on_click={choose_first_month_day(day as i64)}>
                                                            {day}
                                                        </button>
                                                    }
                                                }}
                                            </div>
                                        </div>
                                        <div class="air-parity-calendar">
                                            <h3>
                                                <span hidden={calendar_page.get()!= 0_i64}>
                                                    "August 2026"
                                                </span>
                                                <span hidden={calendar_page.get()!= 1_i64}>
                                                    "September 2026"
                                                </span>
                                                <span hidden={calendar_page.get()!= 2_i64}>
                                                    "October 2026"
                                                </span>
                                            </h3>
                                            <div class="air-parity-weekdays">
                                                <span>"S"</span>
                                                <span>"M"</span>
                                                <span>"T"</span>
                                                <span>"W"</span>
                                                <span>"T"</span>
                                                <span>"F"</span>
                                                <span>"S"</span>
                                            </div>
                                            <div class="air-parity-days">
                                                {for day in 1_i64..=31_i64 {
                                                    html_sync! {
                                                        <button type="button" class="air-parity-day" data_selected={check_in.get() == (day as i64) + (calendar_page.get() + 1_i64) * 31_i64 || check_out.get() == (day as i64) + (calendar_page.get() + 1_i64) * 31_i64} on_click={choose_second_month_day(day as i64)}>
                                                            {day}
                                                        </button>
                                                    }
                                                }}
                                            </div>
                                        </div>
                                    </div>
                                    <div class="air-parity-rail-head">
                                        <button
                                            type="button"
                                            class="air-parity-host"
                                            on_click={clear_dates()}
                                        >
                                            "Clear dates"
                                        </button>
                                        <button
                                            type="button"
                                            class="air-parity-host"
                                            on_click={close_panel()}
                                        >
                                            "Close"
                                        </button>
                                    </div>
                                </div>
                                <div
                                    class="air-parity-flexible"
                                    hidden={date_mode.get()!= "flexible"}
                                >
                                    <h2>"How long would you like to stay?"</h2>
                                    <p class="air-parity-popover-copy">
                                        "Choose a trip length and a month."
                                    </p>
                                    <h3>"How long?"</h3>
                                    <div class="air-parity-flex-options">
                                        <button
                                            type="button"
                                            class="air-parity-flex-option"
                                            data_active={flexible_length.get() == "weekend"}
                                            on_click={flex_weekend()}
                                        >
                                            "A weekend"
                                        </button>
                                        <button
                                            type="button"
                                            class="air-parity-flex-option"
                                            data_active={flexible_length.get() == "week"}
                                            on_click={flex_week()}
                                        >
                                            "A week"
                                        </button>
                                        <button
                                            type="button"
                                            class="air-parity-flex-option"
                                            data_active={flexible_length.get() == "month"}
                                            on_click={flex_month()}
                                        >
                                            "A month"
                                        </button>
                                    </div>
                                    <h3>"When?"</h3>
                                    <div class="air-parity-flex-options">
                                        <button
                                            type="button"
                                            class="air-parity-flex-option"
                                            data_active={flexible_month.get() == "August"}
                                            on_click={flex_august()}
                                        >
                                            "August"
                                        </button>
                                        <button
                                            type="button"
                                            class="air-parity-flex-option"
                                            data_active={flexible_month.get() == "September"}
                                            on_click={flex_september()}
                                        >
                                            "September"
                                        </button>
                                        <button
                                            type="button"
                                            class="air-parity-flex-option"
                                            data_active={flexible_month.get() == "October"}
                                            on_click={flex_october()}
                                        >
                                            "October"
                                        </button>
                                    </div>
                                    <button
                                        type="button"
                                        class="air-parity-host"
                                        on_click={open_who()}
                                    >
                                        "Next"
                                    </button>
                                </div>
                            </section>
                            <section
                                class="air-parity-popover"
                                aria_label="Choose guests"
                                hidden={open_panel.get()!= "who"}
                            >
                                <h2>"Who is coming?"</h2>
                                <div class="air-parity-guests">
                                    <div class="air-parity-guest-row">
                                        <div>
                                            <h3>"Adults"</h3>
                                            <p>"Ages 13 or above"</p>
                                        </div>
                                        <div class="air-parity-stepper">
                                            <button
                                                type="button"
                                                aria_label="Decrease Adults"
                                                disabled={adults.get() == 0_i64}
                                                on_click={|_| { if adults.get() > 0_i64 { adults.update(|value| value - 1_i64); } }}
                                            >
                                                "−"
                                            </button>
                                            <span>{adults.get()}</span>
                                            <button
                                                type="button"
                                                aria_label="Increase Adults"
                                                on_click={|_| adults.update(|value| value + 1_i64)}
                                            >
                                                "＋"
                                            </button>
                                        </div>
                                    </div>
                                    <div class="air-parity-guest-row">
                                        <div>
                                            <h3>"Children"</h3>
                                            <p>"Ages 2 – 12"</p>
                                        </div>
                                        <div class="air-parity-stepper">
                                            <button
                                                type="button"
                                                aria_label="Decrease Children"
                                                disabled={children.get() == 0_i64}
                                                on_click={|_| { if children.get() > 0_i64 { children.update(|value| value - 1_i64); } }}
                                            >
                                                "−"
                                            </button>
                                            <span>{children.get()}</span>
                                            <button
                                                type="button"
                                                aria_label="Increase Children"
                                                on_click={|_| children.update(|value| value + 1_i64)}
                                            >
                                                "＋"
                                            </button>
                                        </div>
                                    </div>
                                    <div class="air-parity-guest-row">
                                        <div>
                                            <h3>"Infants"</h3>
                                            <p>"Under 2"</p>
                                        </div>
                                        <div class="air-parity-stepper">
                                            <button
                                                type="button"
                                                aria_label="Decrease Infants"
                                                disabled={infants.get() == 0_i64}
                                                on_click={|_| { if infants.get() > 0_i64 { infants.update(|value| value - 1_i64); } }}
                                            >
                                                "−"
                                            </button>
                                            <span>{infants.get()}</span>
                                            <button
                                                type="button"
                                                aria_label="Increase Infants"
                                                on_click={|_| infants.update(|value| value + 1_i64)}
                                            >
                                                "＋"
                                            </button>
                                        </div>
                                    </div>
                                    <div class="air-parity-guest-row">
                                        <div>
                                            <h3>"Pets"</h3>
                                            <p>"Bringing a service animal?"</p>
                                        </div>
                                        <div class="air-parity-stepper">
                                            <button
                                                type="button"
                                                aria_label="Decrease Pets"
                                                disabled={pets.get() == 0_i64}
                                                on_click={|_| { if pets.get() > 0_i64 { pets.update(|value| value - 1_i64); } }}
                                            >
                                                "−"
                                            </button>
                                            <span>{pets.get()}</span>
                                            <button
                                                type="button"
                                                aria_label="Increase Pets"
                                                on_click={|_| pets.update(|value| value + 1_i64)}
                                            >
                                                "＋"
                                            </button>
                                        </div>
                                    </div>
                                </div>
                                <button
                                    type="button"
                                    class="air-parity-host"
                                    on_click={close_panel()}
                                >
                                    "Done"
                                </button>
                            </section>
                        </search>
                    </div>
                </header>
                <div class="air-parity-main-and-footer" data_compact={header_stuck.get()}>
                    <main class="air-parity-main">
                        <div class="air-parity-shell">
                            <div hidden={mode.get() != "homes"}>
                                {ParityRailSection {
                                    rail: &HOME_RAILS[0],
                                    on_favorite: favorite_requested,
                                }}
                                {ParityRailSection {
                                    rail: &HOME_RAILS[1],
                                    on_favorite: favorite_requested,
                                }}
                                {ParityRailSection {
                                    rail: &HOME_RAILS[2],
                                    on_favorite: favorite_requested,
                                }}
                                {ParityRailSection {
                                    rail: &HOME_RAILS[3],
                                    on_favorite: favorite_requested,
                                }}
                            </div>
                            <div hidden={mode.get() != "experiences"}>
                                {ParityRailSection {
                                    rail: &EXPERIENCE_RAILS[0],
                                    on_favorite: favorite_requested,
                                }}
                                {ParityRailSection {
                                    rail: &EXPERIENCE_RAILS[1],
                                    on_favorite: favorite_requested,
                                }}
                            </div>
                            <div hidden={mode.get() != "services"}>
                                {ParityRailSection {
                                    rail: &SERVICE_RAILS[0],
                                    on_favorite: favorite_requested,
                                }}
                                {ParityRailSection {
                                    rail: &SERVICE_RAILS[1],
                                    on_favorite: favorite_requested,
                                }}
                            </div>
                        </div>
                        <div class="air-parity-price-toast" role="status">
                            {StaticRaw(TAG_SVG)}
                            "Prices include all fees"
                        </div>
                    </main>
                    <section class="air-parity-inspiration">
                        <div class="air-parity-shell">
                            <h2>"Inspiration for future getaways"</h2>
                            <div class="air-parity-footer-tabs" role="tablist">
                                <button
                                    type="button"
                                    class="air-parity-footer-tab"
                                    role="tab"
                                    data_active={footer_tab.get() == "popular"}
                                    aria_selected={footer_tab.get() == "popular"}
                                    on_click={show_popular()}
                                >
                                    "Popular"
                                </button>
                                <button
                                    type="button"
                                    class="air-parity-footer-tab"
                                    role="tab"
                                    data_active={footer_tab.get() == "culture"}
                                    aria_selected={footer_tab.get() == "culture"}
                                    on_click={show_culture()}
                                >
                                    "Arts & culture"
                                </button>
                                <button
                                    type="button"
                                    class="air-parity-footer-tab"
                                    role="tab"
                                    data_active={footer_tab.get() == "beach"}
                                    aria_selected={footer_tab.get() == "beach"}
                                    on_click={show_beach()}
                                >
                                    "Beach"
                                </button>
                                <button
                                    type="button"
                                    class="air-parity-footer-tab"
                                    role="tab"
                                    data_active={footer_tab.get() == "mountains"}
                                    aria_selected={footer_tab.get() == "mountains"}
                                    on_click={show_mountains()}
                                >
                                    "Mountains"
                                </button>
                                <button
                                    type="button"
                                    class="air-parity-footer-tab"
                                    role="tab"
                                    data_active={footer_tab.get() == "outdoors"}
                                    aria_selected={footer_tab.get() == "outdoors"}
                                    on_click={show_outdoors()}
                                >
                                    "Outdoors"
                                </button>
                                <button
                                    type="button"
                                    class="air-parity-footer-tab"
                                    role="tab"
                                    data_active={footer_tab.get() == "things"}
                                    aria_selected={footer_tab.get() == "things"}
                                    on_click={show_things_to_do()}
                                >
                                    "Things to do"
                                </button>
                                <button
                                    type="button"
                                    class="air-parity-footer-tab"
                                    role="tab"
                                    data_active={footer_tab.get() == "travel"}
                                    aria_selected={footer_tab.get() == "travel"}
                                    on_click={show_travel_tips()}
                                >
                                    "Travel tips & inspiration"
                                </button>
                                <button
                                    type="button"
                                    class="air-parity-footer-tab"
                                    role="tab"
                                    data_active={footer_tab.get() == "apartments"}
                                    aria_selected={footer_tab.get() == "apartments"}
                                    on_click={show_apartments()}
                                >
                                    "Extended-stay apartments"
                                </button>
                            </div>
                            <div
                                class="air-parity-links"
                                role="tabpanel"
                                hidden={footer_tab.get()!= "popular"}
                            >
                                {for (place, kind) in POPULAR_FOOTER_LINKS {
                                    html_sync! {
                                        <a href="#destination">
                                            {*place}
                                            <small>{*kind}</small>
                                        </a>
                                    }
                                }}
                            </div>
                            <div
                                class="air-parity-links"
                                role="tabpanel"
                                hidden={footer_tab.get()!= "culture"}
                            >
                                {for (place, kind) in CULTURE_FOOTER_LINKS {
                                    html_sync! {
                                        <a href="#destination">
                                            {*place}
                                            <small>{*kind}</small>
                                        </a>
                                    }
                                }}
                            </div>
                            <div
                                class="air-parity-links"
                                role="tabpanel"
                                hidden={footer_tab.get()!= "beach"}
                            >
                                {for (place, kind) in BEACH_FOOTER_LINKS {
                                    html_sync! {
                                        <a href="#destination">
                                            {*place}
                                            <small>{*kind}</small>
                                        </a>
                                    }
                                }}
                            </div>
                            <div
                                class="air-parity-links"
                                role="tabpanel"
                                hidden={footer_tab.get()!= "mountains"}
                            >
                                {for (place, kind) in MOUNTAIN_FOOTER_LINKS {
                                    html_sync! {
                                        <a href="#destination">
                                            {*place}
                                            <small>{*kind}</small>
                                        </a>
                                    }
                                }}
                            </div>
                            <div
                                class="air-parity-links"
                                role="tabpanel"
                                hidden={footer_tab.get()!= "outdoors"}
                            >
                                {for (place, kind) in MOUNTAIN_FOOTER_LINKS {
                                    html_sync! {
                                        <a href="#destination">
                                            {*place}
                                            <small>{*kind}</small>
                                        </a>
                                    }
                                }}
                            </div>
                            <div
                                class="air-parity-links"
                                role="tabpanel"
                                hidden={footer_tab.get()!= "things"}
                            >
                                {for (place, kind) in POPULAR_FOOTER_LINKS {
                                    html_sync! {
                                        <a href="#destination">
                                            {*place}
                                            <small>{*kind}</small>
                                        </a>
                                    }
                                }}
                            </div>
                            <div
                                class="air-parity-links"
                                role="tabpanel"
                                hidden={footer_tab.get()!= "travel"}
                            >
                                {for (place, kind) in CULTURE_FOOTER_LINKS {
                                    html_sync! {
                                        <a href="#destination">
                                            {*place}
                                            <small>{*kind}</small>
                                        </a>
                                    }
                                }}
                            </div>
                            <div
                                class="air-parity-links"
                                role="tabpanel"
                                hidden={footer_tab.get()!= "apartments"}
                            >
                                {for (place, kind) in BEACH_FOOTER_LINKS {
                                    html_sync! {
                                        <a href="#destination">
                                            {*place}
                                            <small>{*kind}</small>
                                        </a>
                                    }
                                }}
                            </div>
                            <div class="air-parity-links" hidden={!footer_expanded.get()}>
                                <a href="#destination">
                                    "Fort Lauderdale"
                                    <small>"House rentals"</small>
                                </a>
                                <a href="#destination">
                                    "Philadelphia"
                                    <small>"Monthly rentals"</small>
                                </a>
                                <a href="#destination">
                                    "Charlotte"
                                    <small>"Condo rentals"</small>
                                </a>
                                <a href="#destination">
                                    "Pittsburgh"
                                    <small>"Monthly rentals"</small>
                                </a>
                                <a href="#destination">
                                    "Santa Barbara"
                                    <small>"Vacation rentals"</small>
                                </a>
                                <a href="#destination">
                                    "Monterey"
                                    <small>"Cottage rentals"</small>
                                </a>
                            </div>
                            <button
                                type="button"
                                class="air-parity-host"
                                on_click={toggle_footer_links()}
                            >
                                <span hidden={footer_expanded.get()}>"Show more"</span>
                                <span hidden={!footer_expanded.get()}>"Show less"</span>
                            </button>
                        </div>
                    </section>
                    <div class="air-parity-disclaimer">
                        "Independent Proa benchmark fixture. Not affiliated with or endorsed by Airbnb, Inc."
                    </div>
                    <footer class="air-parity-footer">
                        <div class="air-parity-shell">
                            "© 2026 Proa Labs, Inc. · Privacy · Terms · "
                            <span hidden={selected_locale.get()!= ""}>
                                "English – United States"
                            </span>
                            <span hidden={selected_locale.get() == ""}>
                                {selected_locale.get()}
                            </span>
                            " · "
                            <span hidden={selected_currency.get()!= ""}>"USD – $"</span>
                            <span hidden={selected_currency.get() == ""}>
                                {selected_currency.get()}
                            </span>
                        </div>
                    </footer>
                </div>
                <div
                    class="air-parity-modal"
                    role="dialog"
                    aria_modal="true"
                    aria_label="Languages"
                    hidden={!locale_open.get()}
                >
                    <div class="air-parity-modal-card air-parity-locale-card">
                        <div class="air-parity-modal-head">
                            <button type="button" aria_label="Close" on_click={close_locale()}>
                                "×"
                            </button>
                            <strong>"Choose a language and currency"</strong>
                            <span></span>
                        </div>
                        <div class="air-parity-modal-body air-parity-locale-body">
                            <div class="air-parity-locale-tabs" role="tablist">
                                <button
                                    type="button"
                                    class="air-parity-locale-tab"
                                    role="tab"
                                    aria_selected={locale_tab.get() == "language"}
                                    data_active={locale_tab.get() == "language"}
                                    on_click={show_languages()}
                                >
                                    "Language and region"
                                </button>
                                <button
                                    type="button"
                                    class="air-parity-locale-tab"
                                    role="tab"
                                    aria_selected={locale_tab.get() == "currency"}
                                    data_active={locale_tab.get() == "currency"}
                                    on_click={show_currencies()}
                                >
                                    "Currency"
                                </button>
                            </div>
                            <div role="tabpanel" hidden={locale_tab.get()!= "language"}>
                                <div class="air-parity-translation">
                                    <div>
                                        <strong>"Translation"</strong>
                                        <p class="air-parity-modal-copy">
                                            "Automatically translate descriptions and reviews to English."
                                        </p>
                                    </div>
                                    <button
                                        type="button"
                                        class="air-parity-switch"
                                        role="switch"
                                        aria_label="Translation"
                                        aria_checked={translation_enabled.get()}
                                        data_active={translation_enabled.get()}
                                        on_click={toggle_translation()}
                                    ></button>
                                </div>
                                <h2>"Choose a language and region"</h2>
                                <div class="air-parity-locale-grid">
                                    {for (language, region) in LANGUAGE_OPTIONS {
                                        html_sync! {
                                            <button type="button" class="air-parity-locale-choice" on_click={choose_language(*language)}>
                                                {*language}
                                                <small>{*region}</small>
                                            </button>
                                        }
                                    }}
                                </div>
                            </div>
                            <div role="tabpanel" hidden={locale_tab.get()!= "currency"}>
                                <h2>"Choose a currency"</h2>
                                <div class="air-parity-locale-grid">
                                    {for (currency, code) in CURRENCY_OPTIONS {
                                        html_sync! {
                                            <button type="button" class="air-parity-locale-choice" on_click={choose_currency(*code)}>
                                                {*currency}
                                                <small>{*code}</small>
                                            </button>
                                        }
                                    }}
                                </div>
                            </div>
                        </div>
                    </div>
                </div>
                <div
                    class="air-parity-modal"
                    role="dialog"
                    aria_modal="true"
                    aria_labelledby="air-parity-auth-title"
                    hidden={!auth_open.get()}
                >
                    <div class="air-parity-modal-card">
                        <div class="air-parity-modal-head">
                            <button type="button" aria_label="Close" on_click={close_auth()}>
                                "×"
                            </button>
                            <strong>"Log in or sign up"</strong>
                            <span></span>
                        </div>
                        <div class="air-parity-modal-body">
                            <h2 id="air-parity-auth-title">"Welcome to Airbnb"</h2>
                            <p class="air-parity-modal-copy">
                                "Enter a phone number or email to continue. Authentication itself is outside this public benchmark fixture."
                            </p>
                            <input
                                class="air-parity-login-input"
                                type="text"
                                aria_label="Phone number or email"
                                placeholder="Phone number or email"
                            />
                            <button type="button" class="air-parity-continue">"Continue"</button>
                            <div class="air-parity-auth-divider">"or"</div>
                            <button type="button" class="air-parity-social">
                                "Continue with Google"
                            </button>
                            <button type="button" class="air-parity-social">
                                "Continue with Apple"
                            </button>
                        </div>
                    </div>
                </div>
            </div>
        }
        .render(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_parity_surface_and_only_rsjs_assets() {
        let html = render();
        assert!(html.contains("data-airbnb-parity-app"));
        assert!(html.contains("type=\"search\""));
        assert!(html.contains("air-parity-auth-title"));
        assert!(html.contains("/rsjs-runtime.js"));
        assert!(html.contains("/rsjs/island/airbnbparityapp_"));
        assert_eq!(
            html.matches(r#"<script type="module" src="/rsjs-runtime.js""#)
                .count(),
            1
        );
        assert_eq!(
            html.matches(r#"<script type="module" src="/rsjs/island/airbnbparityapp_"#)
                .count(),
            1
        );
        assert!(
            html.find("/rsjs-runtime.js").unwrap() < html.rfind("</body>").unwrap(),
            "runtime plan must remain inside the document body"
        );
        assert!(!html.contains("cdn.tailwindcss.com"));
        assert!(!html.contains("@hotwired/turbo"));
        assert!(!html.contains("react@"));
        assert!(!html.contains("airbnb-showcase.js"));
    }

    #[test]
    fn parity_island_compacts_after_the_header_scroll_threshold() {
        let manifest = rsjs::manifest();
        let island = manifest
            .islands
            .iter()
            .find(|entry| entry.struct_name == "AirbnbParityApp")
            .expect("AirbnbParityApp island should be registered");

        assert!(island.js_source.contains("window.scrollY ?? 0"));
        assert!(island.js_source.contains("> 96"));
        assert!(island.js_source.contains("bind_window_event("));
        assert!(island.js_source.contains(", \"scroll\", __handler_"));
    }
}
