// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Browser/SDK boundary: this also works in SSR applications with no RSJS runtime.
(function () {
  const api = window.proaThirdParty = window.proaThirdParty || {};
  if (api.loadScript) return;
  const scripts = new Map();
  api.schedule = function (strategy, run) {
    function ready() {
      if (strategy !== "idle") return run();
      if (window.requestIdleCallback) window.requestIdleCallback(run, { timeout: 2000 });
      else window.setTimeout(run, 0);
    }
    if (strategy === "async" || document.readyState === "complete") ready();
    else window.addEventListener("load", ready, { once: true });
  };
  api.loadScript = function (src, options = {}) {
    let url;
    try { url = new URL(src, document.baseURI).href; }
    catch (error) { return Promise.reject(error); }
    if (scripts.has(url)) return scripts.get(url);
    // Register before scheduling so duplicate declarations cannot race.
    let resolve, reject;
    const promise = new Promise(function (onLoad, onError) {
      resolve = onLoad;
      reject = onError;
    });
    scripts.set(url, promise);
    api.schedule(options.strategy, function () {
      // A consent decision may have changed while waiting for load/idle.
      if (options.enabled && !options.enabled()) {
        scripts.delete(url);
        resolve(null);
        return;
      }
      const script = document.createElement("script");
      script.async = true;
      if (options.nonce) script.nonce = options.nonce;
      if (options.integrity) {
        script.integrity = options.integrity;
        script.crossOrigin = "anonymous";
      }
      script.src = url;
      script.addEventListener("load", function () { resolve(script); }, { once: true });
      script.addEventListener("error", function () {
        scripts.delete(url);
        script.remove();
        reject(new Error("Third-party script failed to load: " + url));
      }, { once: true });
      document.head.appendChild(script);
    });
    return promise;
  };
  api.reportError = function (error) {
    window.dispatchEvent(new CustomEvent("proa:script-error", { detail: { message: error.message } }));
  };
})();
