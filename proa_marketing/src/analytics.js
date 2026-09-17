// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Deployment policy belongs to this application, not the reusable GA component.
(function () {
  const id = document.currentScript.dataset.measurementId;
  const production = location.hostname === "proa.so" || location.hostname === "www.proa.so";
  if (!production || window.self !== window.top) window["ga-disable-" + id] = true;
})();
