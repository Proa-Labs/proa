#!/usr/bin/env node
// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { assert, finish, importRuntime, installDocument, makeElement } from "./harness.mjs";

const runtime = await importRuntime();

function islandRoot(id) {
  return makeElement("rsjs-island", { id });
}

{
  const root = islandRoot("visible_strategy");
  installDocument(root);

  let observerCallback = null;
  let observed = null;
  let disconnects = 0;
  globalThis.IntersectionObserver = class {
    constructor(callback) {
      observerCallback = callback;
    }
    observe(node) {
      observed = node;
    }
    disconnect() {
      disconnects += 1;
    }
  };

  let hydrated = 0;
  let disposed = 0;
  const dispose = runtime.hydrate_when_visible("visible_strategy", () => {
    hydrated += 1;
    return () => {
      disposed += 1;
    };
  });

  assert(hydrated === 0, "visible hydration should wait for an intersection");
  assert(observed === root, "visible hydration should observe the island root");
  observerCallback([{ target: root, isIntersecting: true, intersectionRatio: 1 }]);
  assert(hydrated === 1, "visible hydration should run after the root intersects");
  assert(disconnects === 1, "visible hydration should disconnect its observer after hydration");
  dispose();
  assert(disposed === 1, "visible hydration disposer should run child cleanup");
}

{
  const root = islandRoot("visible_replacement");
  installDocument(root);

  const observers = [];
  globalThis.IntersectionObserver = class {
    constructor(callback) {
      this.callback = callback;
      this.disconnected = false;
      observers.push(this);
    }
    observe(node) {
      this.node = node;
    }
    disconnect() {
      this.disconnected = true;
    }
    trigger(entry) {
      if (!this.disconnected) {
        this.callback([entry]);
      }
    }
  };

  const hydrated = [];
  runtime.hydrate_when_visible("visible_replacement", () => {
    hydrated.push("first");
  });
  runtime.hydrate_when_visible("visible_replacement", () => {
    hydrated.push("second");
  });

  assert(observers.length === 2, "visible replacement should create a replacement observer");
  assert(observers[0].disconnected, "visible replacement should disconnect stale pending observer");
  observers[0].trigger({ target: root, isIntersecting: true, intersectionRatio: 1 });
  assert(hydrated.length === 0, "stale visible observer should not hydrate");
  observers[1].trigger({ target: root, isIntersecting: true, intersectionRatio: 1 });
  assert(hydrated.join(",") === "second", "latest visible request should hydrate");
  runtime.dispose_hydrated("visible_replacement", root);
}

{
  const root = islandRoot("idle_strategy");
  installDocument(root);

  let idleCallback = null;
  globalThis.requestIdleCallback = (callback) => {
    idleCallback = callback;
    return 7;
  };
  globalThis.cancelIdleCallback = () => {};

  let hydrated = 0;
  const dispose = runtime.hydrate_when_idle("idle_strategy", () => {
    hydrated += 1;
  });

  assert(hydrated === 0, "idle hydration should wait for requestIdleCallback");
  idleCallback();
  assert(hydrated === 1, "idle hydration should run from the idle callback");
  dispose();
}

{
  const root = islandRoot("interaction_strategy");
  installDocument(root);

  let hydrated = 0;
  const dispose = runtime.hydrate_on_interaction("interaction_strategy", () => {
    hydrated += 1;
  }, ["click"]);

  assert(hydrated === 0, "interaction hydration should wait for the configured event");
  assert(
    (root._listeners.get("click") ?? []).length === 1,
    "interaction hydration should install one capture listener",
  );
  root.dispatchEvent({ type: "click", target: root });
  assert(hydrated === 1, "interaction hydration should run on first matching event");
  assert(
    (root._listeners.get("click") ?? []).length === 0,
    "interaction hydration should remove the trigger listener after hydration",
  );
  dispose();
}

{
  const root = islandRoot("media_strategy");
  installDocument(root);

  let mediaCallback = null;
  let removeCalls = 0;
  globalThis.matchMedia = (query) => ({
    query,
    matches: false,
    addEventListener(event, callback) {
      assert(event === "change", "media hydration should listen for media query changes");
      mediaCallback = callback;
    },
    removeEventListener(event, callback) {
      assert(event === "change", "media hydration should remove media query change listener");
      assert(callback === mediaCallback, "media hydration should remove the registered callback");
      removeCalls += 1;
    },
  });

  let hydrated = 0;
  const dispose = runtime.hydrate_when_media("media_strategy", () => {
    hydrated += 1;
  }, "(min-width: 768px)");

  assert(hydrated === 0, "media hydration should wait while the query does not match");
  mediaCallback({ matches: true });
  assert(hydrated === 1, "media hydration should run once the query matches");
  assert(removeCalls === 1, "media hydration should tear down its media listener");
  dispose();
}

finish("rsjs conformance runtime hydration strategies");
