// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

(function () {
  const tag = document.currentScript;
  window.proaThirdParty.loadScript(tag.dataset.src, {
    strategy: tag.dataset.strategy,
    integrity: tag.dataset.integrity,
    nonce: tag.nonce,
  }).catch(window.proaThirdParty.reportError);
})();
