// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "../..");
const runtimePath = path.join(repoRoot, "rsjs/runtime/rsjs-runtime.js");
const queryRuntimePath = path.join(repoRoot, "rsjs/runtime/rsjs-runtime-query.js");

let failures = 0;

export function assert(condition, message) {
  if (!condition) {
    failures += 1;
    console.error(`FAIL: ${message}`);
  }
}

export function finish(label) {
  if (failures > 0) {
    process.exit(1);
  }
  console.log(`${label}: ok`);
}

// Derives the asset id the compiler baked into a snapshot module by
// reading its hydrate("...") registration (islands) or its exported
// mount_<asset_id> function (component fragments). Runtime scripts must
// use this instead of hardcoding hash-suffixed ids: a re-blessed
// snapshot gets a fresh hash, and a stale hardcoded id makes hydration
// silently match nothing.
export async function assetIdFromSnapshot(snapshotRelativePath) {
  const snapshotPath = path.join(repoRoot, snapshotRelativePath);
  const source = await fs.readFile(snapshotPath, "utf8");
  const match =
    /\bhydrate\(\s*"([A-Za-z0-9_]+)"/.exec(source) ??
    /\bexport function mount_([A-Za-z0-9_]+)\s*\(/.exec(source);
  if (!match) {
    throw new Error(
      `assetIdFromSnapshot: ${snapshotRelativePath} contains no hydrate("...") call or ` +
        "mount_<asset_id> export, so the island asset id cannot be derived and hydration " +
        "would silently match nothing",
    );
  }
  return match[1];
}

export function makeElement(tag, attrs = {}) {
  const node = {
    nodeType: 1,
    tagName: tag.toUpperCase(),
    attrs: { ...attrs },
    children: [],
    parentElement: null,
    nextElementSibling: null,
    style: { display: "" },
    textContent: "",
    value: "",
    checked: false,
    _listeners: new Map(),
    getAttribute(name) {
      return Object.prototype.hasOwnProperty.call(this.attrs, name) ? this.attrs[name] : null;
    },
    hasAttribute(name) {
      return Object.prototype.hasOwnProperty.call(this.attrs, name);
    },
    setAttribute(name, value) {
      this.attrs[name] = String(value);
    },
    removeAttribute(name) {
      delete this.attrs[name];
    },
    appendChild(child) {
      return this.insertBefore(child, null);
    },
    // The structural-mutation subset bind_list relies on: firstChild /
    // nextSibling walks for SSR adoption, insertBefore with DOM move
    // semantics (an already-connected node is detached first), and
    // removeChild. The `children` array stays the source of truth;
    // `_syncSiblings` keeps the legacy `nextElementSibling` data
    // property consistent for the existing tests.
    get parentNode() {
      return this.parentElement;
    },
    get firstChild() {
      return this.children[0] ?? null;
    },
    get nextSibling() {
      const siblings = this.parentElement?.children;
      if (!siblings) {
        return null;
      }
      const index = siblings.indexOf(this);
      return index >= 0 ? siblings[index + 1] ?? null : null;
    },
    insertBefore(child, ref) {
      if (child.parentElement != null) {
        child.parentElement.removeChild(child);
      }
      const index = ref == null ? this.children.length : this.children.indexOf(ref);
      if (index < 0) {
        throw new Error("insertBefore: ref is not a child");
      }
      this.children.splice(index, 0, child);
      child.parentElement = this;
      this._syncSiblings();
      return child;
    },
    removeChild(child) {
      const index = this.children.indexOf(child);
      if (index < 0) {
        throw new Error("removeChild: node is not a child");
      }
      this.children.splice(index, 1);
      child.parentElement = null;
      child.nextElementSibling = null;
      this._syncSiblings();
      return child;
    },
    _syncSiblings() {
      for (let i = 0; i < this.children.length; i += 1) {
        this.children[i].nextElementSibling = this.children[i + 1] ?? null;
      }
    },
    addEventListener(name, handler, options) {
      if (!this._listeners.has(name)) {
        this._listeners.set(name, []);
      }
      const capture = options === true || Boolean(options?.capture);
      this._listeners.get(name).push({ handler, capture });
    },
    removeEventListener(name, handler, options) {
      const listeners = this._listeners.get(name);
      if (!listeners) {
        return;
      }
      const capture = options === true || Boolean(options?.capture);
      this._listeners.set(
        name,
        listeners.filter((listener) => listener.handler !== handler || listener.capture !== capture),
      );
    },
    dispatchEvent(event) {
      if (!event.target) {
        event.target = this;
      }
      const path = [];
      let walker = this;
      while (walker != null) {
        path.push(walker);
        walker = walker.parentElement;
      }
      for (const node of [...path].reverse()) {
        const listeners = node._listeners.get(event.type) ?? [];
        for (const { handler, capture } of listeners) {
          if (capture) handler(event);
        }
      }
      for (const node of path) {
        const listeners = node._listeners.get(event.type) ?? [];
        for (const { handler, capture } of listeners) {
          if (!capture) handler(event);
        }
      }
    },
    closest(selector) {
      const match = /^\[([a-zA-Z0-9-]+)\]$/.exec(selector);
      if (!match) {
        throw new Error(`unsupported selector in DOM shim: ${selector}`);
      }
      const attr = match[1];
      let walker = this;
      while (walker != null) {
        if (walker.hasAttribute?.(attr)) {
          return walker;
        }
        walker = walker.parentElement;
      }
      return null;
    },
    contains(other) {
      let walker = other;
      while (walker != null) {
        if (walker === this) {
          return true;
        }
        walker = walker.parentElement;
      }
      return false;
    },
    querySelector(selector) {
      return this.querySelectorAll(selector)[0] ?? null;
    },
    querySelectorAll(selector) {
      const exact = /^\[([a-zA-Z0-9-]+)="([^"]*)"\]$/.exec(selector);
      const presence = /^\[([a-zA-Z0-9-]+)\]$/.exec(selector);
      if (!exact && !presence) {
        throw new Error(`unsupported selector in DOM shim: ${selector}`);
      }
      const attr = exact?.[1] ?? presence[1];
      const value = exact?.[2];
      const matches = [];
      const visit = (candidate) => {
        for (const child of candidate.children ?? []) {
          if (
            child.hasAttribute?.(attr) &&
            (value === undefined || child.getAttribute(attr) === value)
          ) {
            matches.push(child);
          }
          visit(child);
        }
      };
      visit(this);
      return matches;
    },
  };
  return node;
}

export function installDocument(root, extraNodes = []) {
  globalThis.document = {
    readyState: "complete",
    createElement(tag) {
      return makeElement(tag);
    },
    getElementById(id) {
      if (root.getAttribute("id") === id) {
        return root;
      }
      return extraNodes.find((node) => node.getAttribute?.("id") === id) ?? null;
    },
    querySelectorAll(selector) {
      const match = /^rsjs-island\[id="([^"]+)"\]$/.exec(selector);
      if (match) {
        return root.getAttribute("id") === match[1] ? [root] : [];
      }
      if (selector === "rsjs-island[id]") {
        return [root];
      }
      // The runtime looks islands up by marker attribute first and only
      // falls back to the legacy id lookup when this returns nothing, so
      // an unmatched marker selector must return [] rather than throw.
      const marker = /^rsjs-island\[data-rsjs-island-root="([^"]+)"\]$/.exec(selector);
      if (marker) {
        return root.getAttribute("data-rsjs-island-root") === marker[1] ? [root] : [];
      }
      if (selector === "rsjs-island[data-rsjs-island-root]") {
        return root.hasAttribute?.("data-rsjs-island-root") ? [root] : [];
      }
      if (selector === 'script[type="application/json"][data-rsjs-island-queries]') {
        return extraNodes.filter(
          (node) =>
            node.tagName === "SCRIPT" &&
            node.getAttribute("type") === "application/json" &&
            node.hasAttribute("data-rsjs-island-queries"),
        );
      }
      if (selector === 'script[type="application/json"][data-rsjs-island-mutations]') {
        return extraNodes.filter(
          (node) =>
            node.tagName === "SCRIPT" &&
            node.getAttribute("type") === "application/json" &&
            node.hasAttribute("data-rsjs-island-mutations"),
        );
      }
      throw new Error(`unsupported selector in document shim: ${selector}`);
    },
  };
}

function rewriteRuntimeImport(source) {
  return source
    .replaceAll('from "/rsjs-runtime.js"', 'from "./rsjs-runtime.js"')
    .replaceAll('from "/rsjs-runtime-query.js"', 'from "./rsjs-runtime-query.js"');
}

export async function importSnapshot(snapshotRelativePath, moduleName, extraModules = []) {
  const tmp = await fs.mkdtemp(path.join(os.tmpdir(), "rsjs-conformance-runtime-"));
  try {
    await fs.writeFile(path.join(tmp, "package.json"), '{"type":"module"}\n');
    await fs.writeFile(path.join(tmp, "rsjs-runtime.js"), await fs.readFile(runtimePath, "utf8"));
    await fs.writeFile(
      path.join(tmp, "rsjs-runtime-query.js"),
      await fs.readFile(queryRuntimePath, "utf8"),
    );
    for (const extra of extraModules) {
      const extraPath = path.join(repoRoot, extra.snapshotRelativePath);
      const extraSource = rewriteRuntimeImport(await fs.readFile(extraPath, "utf8"));
      await fs.writeFile(path.join(tmp, extra.moduleName), extraSource);
    }
    const snapshotPath = path.join(repoRoot, snapshotRelativePath);
    const source = rewriteRuntimeImport(await fs.readFile(snapshotPath, "utf8"));
    const modulePath = path.join(tmp, moduleName);
    await fs.writeFile(modulePath, source);
    return await import(pathToFileURL(modulePath).href);
  } finally {
    await fs.rm(tmp, { recursive: true, force: true });
  }
}

// Like `importSnapshot`, but also returns the runtime module instance
// the snapshot itself imported, so tests can call registry-backed
// helpers such as `dispose_hydrated()` against the hydration the
// snapshot just performed. A separately imported runtime copy (via
// `importRuntime`) would carry its own empty registry.
export async function importSnapshotWithRuntime(snapshotRelativePath, moduleName, extraModules = []) {
  const tmp = await fs.mkdtemp(path.join(os.tmpdir(), "rsjs-conformance-runtime-"));
  try {
    await fs.writeFile(path.join(tmp, "package.json"), '{"type":"module"}\n');
    await fs.writeFile(path.join(tmp, "rsjs-runtime.js"), await fs.readFile(runtimePath, "utf8"));
    await fs.writeFile(
      path.join(tmp, "rsjs-runtime-query.js"),
      await fs.readFile(queryRuntimePath, "utf8"),
    );
    for (const extra of extraModules) {
      const extraPath = path.join(repoRoot, extra.snapshotRelativePath);
      const extraSource = rewriteRuntimeImport(await fs.readFile(extraPath, "utf8"));
      await fs.writeFile(path.join(tmp, extra.moduleName), extraSource);
    }
    const snapshotPath = path.join(repoRoot, snapshotRelativePath);
    const source = rewriteRuntimeImport(await fs.readFile(snapshotPath, "utf8"));
    const modulePath = path.join(tmp, moduleName);
    await fs.writeFile(modulePath, source);
    const module = await import(pathToFileURL(modulePath).href);
    const runtime = await import(pathToFileURL(path.join(tmp, "rsjs-runtime.js")).href);
    return { module, runtime };
  } finally {
    await fs.rm(tmp, { recursive: true, force: true });
  }
}

export async function importRuntime() {
  const tmp = await fs.mkdtemp(path.join(os.tmpdir(), "rsjs-conformance-runtime-"));
  try {
    await fs.writeFile(path.join(tmp, "package.json"), '{"type":"module"}\n');
    await fs.writeFile(path.join(tmp, "rsjs-runtime.js"), await fs.readFile(runtimePath, "utf8"));
    await fs.writeFile(
      path.join(tmp, "rsjs-runtime-query.js"),
      await fs.readFile(queryRuntimePath, "utf8"),
    );
    return await import(pathToFileURL(path.join(tmp, "rsjs-runtime.js")).href);
  } finally {
    await fs.rm(tmp, { recursive: true, force: true });
  }
}
