// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

/*__PROA_IMPORTS__*/

// Inject layout-shift prevention CSS (idempotent)
if (!document.getElementById("__proa_island_css")) {
  const s = document.createElement("style");
  s.id = "__proa_island_css";
  s.textContent = "[data-island]{transition:min-height .2s ease-out,opacity .15s ease-in}[data-island][data-hydrated]{min-height:0!important}";
  document.head.appendChild(s);
}

const runtimeState = window.__proaReactState || (window.__proaReactState = {
  registry: new Map(),
  roots: new Map(),
  hydrateIslands: null,
  boundaryListenerInstalled: false,
});

const registry = runtimeState.registry;
for (const [name, entry] of [
/*__PROA_REGISTRY__*/
]) registry.set(name, entry);

const roots = runtimeState.roots;

const DEFAULT_SELECTOR = /*__PROA_SELECTOR__*/;

function parseProps(id) {
  if (!id) return {};
  const node = document.getElementById(id);
  if (!node) return {};
  try {
    return JSON.parse(node.textContent ?? "{}");
  } catch (err) {
    console.error("Failed to parse props for island", id, err);
    return {};
  }
}

async function loadComponent(entry) {
  const mod = await import(entry.modulePath);
  if (entry.exportName && mod[entry.exportName]) {
    return mod[entry.exportName];
  }
  if ("default" in mod) {
    return mod.default;
  }
  console.warn("React island module has no matching export", entry);
  return null;
}

function getErrorConfig(el) {
  const retryCount = parseInt(el.getAttribute("data-error-retry-count") || "3", 10);
  const retryDelay = parseInt(el.getAttribute("data-error-retry-delay") || "1000", 10);
  const reportErrors = el.getAttribute("data-error-report") === "true";
  const errorFallback = el.getAttribute("data-error-fallback") || null;
  return { retryCount, retryDelay, reportErrors, errorFallback };
}

function showErrorFallback(el, errorFallback) {
  if (errorFallback) {
    el.innerHTML = errorFallback;
    el.setAttribute("data-hydration-failed", "true");
  }
}

function reportError(islandName, error, attempt) {
  if (typeof window.__proaReportError === "function") {
    window.__proaReportError({
      island: islandName,
      error: error.message,
      stack: error.stack,
      attempt: attempt,
      timestamp: Date.now(),
    });
  }
}

async function hydrateIsland(el, entry, attempt = 1) {
  const errorConfig = getErrorConfig(el);

  try {
    const component = await loadComponent(entry);
    if (!component) {
      throw new Error(`Component not found: ${entry.modulePath}`);
    }

    const propsId = el.getAttribute("data-props-id");
    const props = parseProps(propsId);
    const element = React.createElement(component, props);
    /*__PROA_MOUNT__*/

    // Mark as successfully hydrated
    el.setAttribute("data-hydrated", "true");
  } catch (err) {
    console.error(`Failed to hydrate island ${entry.modulePath} (attempt ${attempt}/${errorConfig.retryCount})`, err);

    // Report error if configured
    if (errorConfig.reportErrors) {
      reportError(el.getAttribute("data-island"), err, attempt);
    }

    // Retry with exponential backoff
    if (attempt < errorConfig.retryCount) {
      const delay = errorConfig.retryDelay * Math.pow(2, attempt - 1);
      console.log(`Retrying hydration in ${delay}ms...`);
      setTimeout(() => hydrateIsland(el, entry, attempt + 1), delay);
    } else {
      // All retries exhausted, show error fallback
      console.error(`All retry attempts exhausted for island ${entry.modulePath}`);
      showErrorFallback(el, errorConfig.errorFallback);
    }
  }
}

function scheduleHydration(el, entry) {
  const runner = () => hydrateIsland(el, entry);
  switch (entry.strategy.type) {
    case "immediate":
      runner();
      break;
    case "idle":
      if ("requestIdleCallback" in window) {
        window.requestIdleCallback(() => runner());
      } else {
        setTimeout(runner, 1);
      }
      break;
    case "visible": {
      const observer = new IntersectionObserver((entries, obs) => {
        for (const item of entries) {
          if (item.isIntersecting) {
            obs.disconnect();
            runner();
            break;
          }
        }
      });
      observer.observe(el);
      break;
    }
    case "interaction": {
      const eventName = entry.strategy.event || "click";
      const handler = () => {
        el.removeEventListener(eventName, handler);
        runner();
      };
      el.addEventListener(eventName, handler, { once: true });
      break;
    }
    case "media": {
      const query = entry.strategy.query;
      if (!query || typeof window.matchMedia !== "function") {
        runner();
        break;
      }
      const media = window.matchMedia(query);
      if (!media || media.matches) {
        runner();
        break;
      }
      const handler = (event) => {
        if (!event || !event.matches) return;
        if (typeof media.removeEventListener === "function") {
          media.removeEventListener("change", handler);
        } else if (typeof media.removeListener === "function") {
          media.removeListener(handler);
        }
        runner();
      };
      if (typeof media.addEventListener === "function") {
        media.addEventListener("change", handler);
      } else if (typeof media.addListener === "function") {
        media.addListener(handler);
      } else {
        runner();
      }
      break;
    }
    default:
      console.warn("Unknown hydration strategy", entry.strategy);
      runner();
      break;
  }
}

function unmountAll() {
  for (const [id, root] of roots) {
    const el = document.getElementById(id);
    if (el && el.hasAttribute("data-turbo-permanent")) continue;
    try { root.unmount(); } catch (_) {}
    roots.delete(id);
    if (el) el.removeAttribute("data-hydrated");
  }
}

function hydrateIslands(selector = DEFAULT_SELECTOR) {
  document.querySelectorAll(selector).forEach((el) => {
    if (el.hasAttribute("data-hydrated")) return;
    const name = el.getAttribute("data-island");
    if (!name) return;
    const entry = registry.get(name);
    if (!entry) {
      console.warn("No React module registered for island", name);
      return;
    }
    scheduleHydration(el, entry);
  });
}

runtimeState.hydrateIslands = hydrateIslands;
if (!runtimeState.boundaryListenerInstalled) {
  document.addEventListener("proa:boundary-ready", () => {
    runtimeState.hydrateIslands?.();
  });
  runtimeState.boundaryListenerInstalled = true;
}

/*__PROA_FOOTER__*/
