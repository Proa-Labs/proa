// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Third-party gtag bootstrap stays outside RSJS: it owns Google's global queue.
(function () {
  const id = document.currentScript?.dataset.measurementId;
  const production = location.hostname === "proa.so" || location.hostname === "www.proa.so";
  if (!id || !production || window.self !== window.top) return;

  window.dataLayer = window.dataLayer || [];
  window.gtag = function () { window.dataLayer.push(arguments); };
  window.gtag("js", new Date());
  window.gtag("config", id, {
    allow_google_signals: false,
    allow_ad_personalization_signals: false,
  });

  const tag = document.createElement("script");
  tag.async = true;
  tag.src = "https://www.googletagmanager.com/gtag/js?id=" + encodeURIComponent(id);
  document.head.appendChild(tag);
})();
