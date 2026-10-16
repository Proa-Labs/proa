// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

/* Proa dev live reload client. Injected only by `proa dev`; never shipped. */
(function () {
  if (window.__proaLiveReload) return;
  window.__proaLiveReload = true;

  var PORT = __PROA_DEV_RELOAD_PORT__;
  var PATH = __PROA_DEV_EVENTS_PATH__;
  var SAME_ORIGIN = __PROA_DEV_RELOAD_SAME_ORIGIN__;
  /* Proves this page came from the running dev server. Without it any site
     open in the same browser could read the stream, and a tab left over from
     an earlier session could be reloaded by a server that never served it. */
  var TOKEN = "__PROA_DEV_RELOAD_TOKEN__";
  /* The event stream runs beside this page on the dev machine. Resolving the
     host from `location` keeps it reachable however the page was opened:
     localhost, 127.0.0.1, or a LAN address when the site binds 0.0.0.0. */
  var endpoint = (SAME_ORIGIN ? window.location.origin :
    window.location.protocol +
    "//" +
    window.location.hostname +
    ":" +
    PORT) +
    PATH +
    "?token=" +
    TOKEN;

  var boot = null;
  var badge = null;
  var badgeTimer = null;
  var overlay = null;

  function status(text) {
    clearTimeout(badgeTimer);
    if (!text) {
      if (badge) badge.style.opacity = "0";
      return;
    }
    if (!badge) {
      badge = document.createElement("div");
      badge.setAttribute("data-proa-dev-status", "");
      badge.style.cssText =
        "position:fixed;right:12px;bottom:12px;z-index:2147483646;padding:6px 12px;" +
        "border-radius:999px;font:500 12px/1.4 ui-monospace,SFMono-Regular,Menlo,monospace;" +
        "color:#fafafa;background:#18181b;box-shadow:0 2px 12px rgba(0,0,0,.28);" +
        "pointer-events:none;transition:opacity .15s ease";
      (document.body || document.documentElement).appendChild(badge);
    }
    badge.textContent = text;
    badge.style.opacity = "1";
  }

  function dismissOverlay() {
    if (!overlay) return;
    overlay.remove();
    overlay = null;
  }

  function showOverlay(message) {
    dismissOverlay();
    overlay = document.createElement("div");
    overlay.setAttribute("data-proa-dev-error", "");
    overlay.style.cssText =
      "position:fixed;inset:0;z-index:2147483647;overflow:auto;padding:32px;" +
      "background:rgba(9,9,11,.94);color:#fafafa;" +
      "font:14px/1.6 ui-monospace,SFMono-Regular,Menlo,monospace";

    var heading = document.createElement("div");
    heading.textContent = "Build failed";
    heading.style.cssText =
      "font:600 15px/1.4 ui-sans-serif,system-ui,sans-serif;color:#fca5a5;margin-bottom:4px";

    /* Cargo streams rustc diagnostics straight to the terminal, so the
       overlay reports the failure and points at where the detail is. */
    var hint = document.createElement("div");
    hint.textContent =
      "The previous build is still being served. Full compiler output is in the proa dev terminal. Press Escape to dismiss.";
    hint.style.cssText =
      "font:13px/1.5 ui-sans-serif,system-ui,sans-serif;color:#a1a1aa;margin-bottom:20px";

    var detail = document.createElement("pre");
    detail.textContent = message;
    detail.style.cssText =
      "margin:0;white-space:pre-wrap;word-break:break-word;color:#e4e4e7";

    overlay.appendChild(heading);
    overlay.appendChild(hint);
    overlay.appendChild(detail);
    overlay.addEventListener("click", dismissOverlay);
    (document.body || document.documentElement).appendChild(overlay);
  }

  /* Swap stylesheets without navigating, so scroll position, form values, and
     open dialogs all survive a style-only edit. Each link is replaced by a
     cache-busted copy and only removed once the copy has loaded, which avoids
     a flash of unstyled content. */
  function reloadStylesheets() {
    var links = document.querySelectorAll('link[rel~="stylesheet"][href]');
    var stamp = Date.now();
    for (var i = 0; i < links.length; i++) {
      var link = links[i];
      var url;
      try {
        url = new URL(link.href, window.location.href);
      } catch (error) {
        continue;
      }
      if (url.origin !== window.location.origin) continue;
      url.searchParams.set("proa_dev", String(stamp));

      var next = link.cloneNode();
      next.href = url.href;
      (function (previous, replacement) {
        var drop = function () {
          if (previous.parentNode) previous.remove();
        };
        replacement.addEventListener("load", drop);
        replacement.addEventListener("error", drop);
        setTimeout(drop, 3000);
      })(link, next);
      link.parentNode.insertBefore(next, link.nextSibling);
    }
  }

  function payload(event) {
    try {
      return JSON.parse(event.data);
    } catch (error) {
      return {};
    }
  }

  var source = new EventSource(endpoint);

  source.addEventListener("hello", function (event) {
    failures = 0;
    markRecovery(false);
    var generation = payload(event).generation;
    /* A differing generation means this tab was disconnected across at least
       one rebuild and missed its reload. */
    if (boot !== null && generation !== boot) {
      window.location.reload();
      return;
    }
    boot = generation;
    dismissOverlay();
    status(null);
  });

  source.addEventListener("building", function () {
    status("rebuilding");
  });

  source.addEventListener("reload", function () {
    status("reloading");
    window.location.reload();
  });

  source.addEventListener("css", function (event) {
    boot = payload(event).generation;
    dismissOverlay();
    reloadStylesheets();
    status("styles updated");
    badgeTimer = setTimeout(function () {
      status(null);
    }, 1200);
  });

  source.addEventListener("failed", function (event) {
    var data = payload(event);
    boot = data.generation;
    status(null);
    showOverlay(data.message || "The rebuild failed. See the proa dev output.");
  });

  /* Two ways this page can end up talking to the wrong stream, both fixed by
     loading the page again and taking the endpoint it comes with:

     CLOSED means the server refused the request outright, which is what a
     restarted `proa dev` looks like, since the session token baked into this
     page is no longer the current one.

     Repeated CONNECTING means the endpoint itself is unreachable: the reload
     port moved when `proa dev` restarted, or the page is being served over
     https or a tunnel that does not carry it.

     Recovery reloads the page at most once per endpoint. The marker lives in
     sessionStorage rather than a local variable because the reload itself
     starts a fresh script: a permanently unreachable stream would otherwise
     reload the tab about once a second, forever. */
  var STREAM_GONE_AFTER = 5;
  var failures = 0;
  /* The marker has to outlive the reload, or every fresh page would be
     entitled to one more, and an endpoint that is permanently unreachable
     (an https origin, a tunnel that does not carry the port) would reload
     the tab forever. A successful greeting clears it. */
  var RECOVERY_KEY = "proa-dev-reload-recovered:" + PORT + ":" + TOKEN;

  function alreadyRecovered() {
    try {
      return window.sessionStorage.getItem(RECOVERY_KEY) === "1";
    } catch (error) {
      return false;
    }
  }

  function markRecovery(value) {
    try {
      if (value) window.sessionStorage.setItem(RECOVERY_KEY, "1");
      else window.sessionStorage.removeItem(RECOVERY_KEY);
      return true;
    } catch (error) {
      /* Private mode and file:// origins refuse storage. Only a failed write
         matters: without somewhere to record the attempt, a reload could
         happen again and again. Clearing is allowed to fail silently, or the
         first successful greeting would disable recovery for the session. */
      return false;
    }
  }

  var recovered = false;

  function recover() {
    if (recovered || alreadyRecovered()) {
      status("live reload disconnected");
      return;
    }
    recovered = true;
    if (!markRecovery(true)) {
      /* Storage refused the write, so a second reload could not be prevented.
         A stale page can be fixed by hand; a reload loop cannot. */
      status("live reload disconnected");
      return;
    }
    source.close();
    status("reconnecting");
    var attempts = 0;
    var poll = setInterval(function () {
      if (++attempts > 60) {
        clearInterval(poll);
        status("live reload disconnected");
        return;
      }
      fetch(window.location.href, { method: "HEAD", cache: "no-store" })
        .then(function () {
          clearInterval(poll);
          window.location.reload();
        })
        .catch(function () {});
    }, 1000);
  }

  source.onerror = function () {
    if (source.readyState === EventSource.CLOSED) {
      recover();
      return;
    }
    status("reconnecting");
    /* EventSource retries on its own, so give it a few rounds before deciding
       the endpoint is gone rather than the server merely restarting. */
    if (++failures >= STREAM_GONE_AFTER) recover();
  };

  document.addEventListener("keydown", function (event) {
    if (event.key === "Escape") dismissOverlay();
  });
})();
