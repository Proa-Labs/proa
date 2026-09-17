// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

(function () {
  const tag = document.currentScript;
  const id = tag.dataset.measurementId;
  // Honor the documented GA opt-out flag, including application host guards.
  if (window["ga-disable-" + id] === true) return;
  const api = window.proaThirdParty;
  if (!api.googleAnalytics) {
    const records = new Map();
    let decision = null;
    let initialized = false;
    let scriptUrl;
    window.dataLayer = window.dataLayer || [];
    window.gtag = window.gtag || function () { window.dataLayer.push(arguments); };
    function consent(granted) {
      return {
        analytics_storage: granted ? "granted" : "denied",
        ad_storage: "denied",
        ad_user_data: "denied",
        ad_personalization: "denied",
      };
    }
    function start(record) {
      window["ga-disable-" + record.id] = false;
      if (!initialized) {
        initialized = true;
        window.gtag("consent", "default", consent(true));
        window.gtag("js", new Date());
      }
      if (!record.configured) {
        record.configured = true;
        window.gtag("config", record.id, {
          allow_google_signals: false,
          allow_ad_personalization_signals: false,
          send_page_view: record.pageView,
        });
      }
      scriptUrl = scriptUrl || "https://www.googletagmanager.com/gtag/js?id=" + encodeURIComponent(record.id);
      api.loadScript(scriptUrl, {
        strategy: record.strategy,
        nonce: record.nonce,
        enabled: function () { return Array.from(records.values()).some(value => value.granted); },
      }).catch(api.reportError);
    }
    api.googleAnalytics = {
      register: function (record) {
        if (records.has(record.id)) return;
        record.granted = decision === null ? record.granted : decision;
        records.set(record.id, record);
        window["ga-disable-" + record.id] = !record.granted;
        if (record.granted) start(record);
      },
      // A site's consent decision applies to every GA property rendered here.
      setConsent: function (granted) {
        if (typeof granted !== "boolean") throw new TypeError("Consent must be a boolean");
        decision = granted;
        if (initialized) window.gtag("consent", "update", consent(granted));
        for (const record of records.values()) {
          record.granted = granted;
          window["ga-disable-" + record.id] = !granted;
          if (granted) start(record);
        }
      },
      track: function (measurementId, eventName, parameters = {}) {
        const record = records.get(measurementId);
        if (!record || !record.granted || window["ga-disable-" + measurementId] === true) return false;
        window.gtag("event", eventName, { ...parameters, send_to: measurementId });
        return true;
      },
    };
  }
  api.googleAnalytics.register({
    id,
    granted: tag.dataset.consent === "granted",
    strategy: tag.dataset.strategy,
    nonce: tag.nonce,
    pageView: tag.dataset.pageView !== "false",
    configured: false,
  });
})();
