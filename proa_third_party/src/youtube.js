// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

(function () {
  const root = document.currentScript.parentElement;
  const button = root.querySelector("[data-proa-youtube-play]");
  if (!button || button.dataset.proaBound) return;
  button.dataset.proaBound = "true";
  button.addEventListener("click", function () {
    const iframe = document.createElement("iframe");
    iframe.src = button.dataset.src;
    iframe.title = button.dataset.title;
    iframe.allow = "autoplay; encrypted-media; picture-in-picture; fullscreen";
    iframe.allowFullscreen = true;
    iframe.referrerPolicy = "strict-origin-when-cross-origin";
    button.replaceWith(iframe);
    iframe.focus({ preventScroll: true });
  }, { once: true });
})();
