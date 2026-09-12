// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

/*!
 * proa_docs default behaviour
 *
 * Wires the interactive components `proa_docs_build` emits. Everything else --
 * tabs, the code explorer, accordions -- is CSS-only and needs no JavaScript.
 *
 * Targets only markup this crate emits, so it is safe to drop into any app.
 * Progressive enhancement: without it, code blocks still render and read; only
 * the copy buttons go inert.
 *
 * Contract: https://proa.so/docs/proa-docs/styling-contract
 */

(function () {
  if (typeof document === 'undefined') return;

  document.addEventListener('click', function (event) {
    var target = eventTargetElement(event.target);
    if (!target) return;

    var codeCopy = target.closest('[data-code-copy]');
    if (codeCopy) {
      event.preventDefault();
      copyCodeBlock(codeCopy);
      return;
    }

    var copyMarkdown = target.closest('[data-docs-copy-markdown]');
    if (copyMarkdown) {
      event.preventDefault();
      copyPageMarkdown(copyMarkdown);
    }
  });

  /** Copy the source of the `[data-code-block]` this button belongs to. */
  function copyCodeBlock(button) {
    var block = button.closest('[data-code-block]');
    var code = block ? block.querySelector('pre code') : null;
    if (!code) return;

    writeClipboard(code.textContent || '')
      .then(function () {
        showCopied(button);
      })
      .catch(function () {});
  }

  /**
   * Copy a page's Markdown representation.
   *
   * The button carries `data-docs-markdown-src`, which points at the `.md`
   * artifact `proa_docs_build` emits alongside every page.
   */
  function copyPageMarkdown(button) {
    var src = button.getAttribute('data-docs-markdown-src') || '';
    if (!src || typeof fetch !== 'function') return;
    if (!window.navigator || !navigator.clipboard) return;

    fetch(src)
      .then(function (response) {
        return response.ok ? response.text() : Promise.reject(response.status);
      })
      .then(function (text) {
        return navigator.clipboard.writeText(text);
      })
      .then(function () {
        showCopied(button);
      })
      .catch(function () {});
  }

  /** Clipboard API where available, textarea fallback on insecure origins. */
  function writeClipboard(text) {
    if (
      window.navigator &&
      navigator.clipboard &&
      typeof navigator.clipboard.writeText === 'function' &&
      window.isSecureContext !== false
    ) {
      return navigator.clipboard.writeText(text);
    }

    return new Promise(function (resolve, reject) {
      var textarea = document.createElement('textarea');
      textarea.value = text;
      textarea.setAttribute('readonly', '');
      textarea.style.position = 'fixed';
      textarea.style.top = '-1000px';
      textarea.style.left = '-1000px';
      document.body.appendChild(textarea);
      textarea.select();

      try {
        if (document.execCommand('copy')) resolve();
        else reject(new Error('copy failed'));
      } catch (error) {
        reject(error);
      } finally {
        document.body.removeChild(textarea);
      }
    });
  }

  /** Flip the button to `data-copied="true"` for 1.6s. Style it yourself. */
  function showCopied(button) {
    if (typeof button._proaCopyHtml === 'undefined') button._proaCopyHtml = button.innerHTML;
    if (button._proaCopyTimer) clearTimeout(button._proaCopyTimer);

    button.setAttribute('data-copied', 'true');
    button.textContent = 'Copied';
    button._proaCopyTimer = setTimeout(function () {
      button.removeAttribute('data-copied');
      button.innerHTML = button._proaCopyHtml;
      button._proaCopyTimer = null;
    }, 1600);
  }

  function eventTargetElement(target) {
    if (!target) return null;
    if (target.nodeType === 1) return target;
    return target.parentElement || null;
  }
})();
