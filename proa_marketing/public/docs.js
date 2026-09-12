// Docs-only page helpers. The global search modal is an rsjs island; its
// results are rendered by search.mjs (Orama in the browser).

(function () {
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
    if (!copyMarkdown) return;

    event.preventDefault();
    copyDocsMarkdown(copyMarkdown);
  });

  onReady(initDocsTocActive);
  onReady(revealActiveSidebarItem);

  function copyDocsMarkdown(button) {
    var src = button.getAttribute('data-docs-markdown-src') || '';
    if (!src || typeof fetch !== 'function') return;
    if (!window.navigator || !navigator.clipboard || typeof navigator.clipboard.writeText !== 'function') return;

    fetch(src)
      .then(function (response) {
        return response.ok ? response.text() : Promise.reject(response.status);
      })
      .then(function (text) {
        return navigator.clipboard.writeText(text);
      })
      .then(function () {
        showDocsCopied(button);
      })
      .catch(function () {});
  }

  function copyCodeBlock(button) {
    var block = button.closest('[data-code-block]');
    var code = block ? block.querySelector('pre code') : null;
    if (!code) return;

    writeClipboard(code.textContent || '')
      .then(function () {
        showDocsCopied(button);
      })
      .catch(function () {});
  }

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

  function showDocsCopied(button) {
    if (typeof button._docsCopyHtml === 'undefined') button._docsCopyHtml = button.innerHTML;
    if (button._docsCopyTimer) clearTimeout(button._docsCopyTimer);

    button.setAttribute('data-copied', 'true');
    button.textContent = 'Copied';
    button._docsCopyTimer = setTimeout(function () {
      button.removeAttribute('data-copied');
      button.innerHTML = button._docsCopyHtml;
      button._docsCopyTimer = null;
    }, 1600);
  }

  function initDocsTocActive() {
    if (typeof window.IntersectionObserver !== 'function') return;

    var article = document.querySelector('.docs-article');
    var toc = document.querySelector('.docs-toc');
    if (!article || !toc) return;

    var links = toc.querySelectorAll('a[href^="#"]');
    if (!links.length) return;

    var firstId = '';
    var linksById = {};
    links.forEach(function (link) {
      var id = tocLinkId(link);
      if (!id) return;
      if (!firstId) firstId = id;
      if (!linksById[id]) linksById[id] = [];
      linksById[id].push(link);
    });
    if (!firstId) return;

    var activeId = firstId;
    setActiveTocLink(links, firstId);

    var headings = article.querySelectorAll('h1[id], h2[id], h3[id], h4[id], h5[id], h6[id]');
    if (!headings.length) return;

    var observer = new IntersectionObserver(function (entries) {
      var next = null;
      entries.forEach(function (entry) {
        if (!entry.isIntersecting || !entry.target || !linksById[entry.target.id]) return;
        if (!next || entry.boundingClientRect.top < next.boundingClientRect.top) next = entry;
      });
      if (!next || next.target.id === activeId) return;
      setActiveTocLink(links, next.target.id);
      activeId = next.target.id;
    }, { rootMargin: '0px 0px -70% 0px', threshold: 0 });

    headings.forEach(function (heading) {
      if (heading.id && linksById[heading.id]) observer.observe(heading);
    });
  }

  function setActiveTocLink(links, id) {
    links.forEach(function (link) {
      if (tocLinkId(link) === id) link.setAttribute('data-active', 'true');
      else link.removeAttribute('data-active');
    });
  }

  function tocLinkId(link) {
    var href = link.getAttribute('href') || '';
    if (href.charAt(0) !== '#') return '';
    return safeDecode(href.slice(1));
  }

  function safeDecode(value) {
    try {
      return decodeURIComponent(value);
    } catch (_) {
      return value;
    }
  }

  // Every sidebar link is a full document navigation, so `.docs-sidebar-content`
  // re-renders at scrollTop 0 and a deep nav item lands off-screen. Put the
  // active item back in view without touching window scroll: assigning
  // scrollTop on the container cannot move the page, whereas scrollIntoView
  // can walk up and scroll ancestors including the document.
  function revealActiveSidebarItem() {
    // The page renders two of these: the mobile drawer copy comes first in
    // the DOM and is zero-height on desktop, so querySelector would silently
    // target the wrong one. Walk all of them and skip any that is not laid
    // out (clientHeight 0) or has nothing to scroll.
    var containers = document.querySelectorAll('.docs-sidebar-content');
    for (var i = 0; i < containers.length; i++) revealIn(containers[i]);
  }

  function revealIn(container) {
    if (!container || !container.clientHeight) return;
    if (container.scrollHeight <= container.clientHeight) return;

    var active = container.querySelector('[data-active="true"]');
    if (!active) return;

    // Already fully visible: leave the container alone.
    var itemTop = active.offsetTop;
    var itemBottom = itemTop + active.offsetHeight;
    var viewTop = container.scrollTop;
    var viewBottom = viewTop + container.clientHeight;
    if (itemTop >= viewTop && itemBottom <= viewBottom) return;

    // Center it, clamped to the scrollable range.
    var target = itemTop - (container.clientHeight - active.offsetHeight) / 2;
    var max = container.scrollHeight - container.clientHeight;
    container.scrollTop = Math.max(0, Math.min(target, max));
  }

  function onReady(fn) {
    if (document.readyState === 'loading') {
      document.addEventListener('DOMContentLoaded', fn);
    } else {
      fn();
    }
  }

  function eventTargetElement(target) {
    if (!target) return null;
    if (target.nodeType === 1) return target;
    return target.parentElement || null;
  }
})();
