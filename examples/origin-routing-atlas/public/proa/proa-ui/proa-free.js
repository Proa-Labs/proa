/**
 * Proa UI - Free Component Runtime
 *
 * Allowlisted runtime for the MIT OR Apache-2.0 Free component source.
 * Generated from the Free sections of proa.js; do not add Pro handlers here.
 */

(function() {
  'use strict';

  // Component state stored on elements via WeakMap (no memory leaks)
  const componentState = new WeakMap();

  // Get or create state for an element
  function getState(el, defaults = {}) {
    if (!componentState.has(el)) {
      componentState.set(el, { ...defaults });
    }
    return componentState.get(el);
  }

  // Root-owned compound components render their parts independently, so a
  // descendant cannot truthfully duplicate a root's `default_open` value in
  // SSR. These helpers copy the canonical root state to animated parts when
  // the runtime starts and establish the ID/ARIA relationships that require
  // knowledge of both the root and its descendants.
  let generatedContractId = 0;

  function findOwnedPart(root, rootSelector, partSelector) {
    for (const part of root.querySelectorAll(partSelector)) {
      if (part.closest(rootSelector) === root) return part;
    }
    return null;
  }

  function findOwnedDescendant(owner, ownerSelector, partSelector) {
    if (!owner || !partSelector) return null;
    for (const part of owner.querySelectorAll(partSelector)) {
      if (part.closest(ownerSelector) === owner) return part;
    }
    return null;
  }

  function ensureContractId(root, dataAttribute, prefix) {
    let id = root.getAttribute(dataAttribute) || root.id;
    if (!id) {
      generatedContractId += 1;
      id = `proa-${prefix}-${generatedContractId}`;
    }
    if (!root.id) root.id = id;
    root.setAttribute(dataAttribute, id);
    return id;
  }

  function wireModalContract(root, config) {
    const portal = findOwnedPart(root, config.root, config.portal);
    const content = findOwnedPart(root, config.root, config.content);
    const overlay = findOwnedPart(root, config.root, config.overlay);
    const trigger = findOwnedPart(root, config.root, config.trigger);
    const baseId = ensureContractId(root, config.idAttribute, config.idPrefix);

    if (trigger && !trigger.id) trigger.id = `${baseId}--trigger`;
    if (content && !content.id) content.id = `${baseId}--content`;

    const title = findOwnedDescendant(content, config.content, config.title);
    const description = findOwnedDescendant(content, config.content, config.description);
    if (title && !title.id) title.id = `${baseId}--title`;
    if (description && !description.id) description.id = `${baseId}--description`;

    if (trigger && content) {
      trigger.setAttribute('aria-controls', content.id);
    }
    if (content && title) {
      content.setAttribute('aria-labelledby', title.id);
    }
    if (content && description) {
      content.setAttribute('aria-describedby', description.id);
    }

    return { portal, content, overlay, trigger };
  }

  function syncModalContract(root, config, open) {
    const parts = wireModalContract(root, config);
    const state = open ? 'open' : 'closed';

    root.dataset.state = state;
    root.style.setProperty('--proa-overlay-display', open ? 'block' : 'none');
    if (parts.portal) {
      parts.portal.dataset.state = state;
      parts.portal.style.display = open ? 'block' : 'none';
    }
    if (parts.content) {
      parts.content.dataset.state = state;
      parts.content.setAttribute('aria-hidden', open ? 'false' : 'true');
    }
    if (parts.overlay) parts.overlay.dataset.state = state;
    if (parts.trigger) {
      parts.trigger.setAttribute('aria-expanded', open ? 'true' : 'false');
    }

    return parts;
  }

  function syncModalScrollLock() {
    if (!document.body) return;
    const openModal = document.querySelector(
      '[data-dialog-root][data-state="open"]'
    );
    document.body.style.overflow = openModal ? 'hidden' : '';
  }

  function initializeOnReady(callback) {
    if (document.readyState === 'loading') {
      document.addEventListener('DOMContentLoaded', callback);
    } else {
      callback();
    }
    document.addEventListener('turbo:load', callback);
    document.addEventListener('turbo:render', callback);
    document.addEventListener('turbo:frame-load', callback);
  }

  // ============================================================
  // DIALOG COMPONENT
  // ============================================================
  const DIALOG_CONTRACT = {
    root: '[data-dialog-root]',
    trigger: '[data-dialog-trigger]',
    portal: '[data-dialog-portal]',
    content: '[data-dialog-content], [data-sheet-content]',
    overlay: '[data-dialog-overlay]',
    title: '[data-dialog-title], [data-sheet-title]',
    description: '[data-dialog-description], [data-sheet-description]',
    idAttribute: 'data-dialog-id',
    idPrefix: 'dialog',
  };

  const Dialog = {
    selectors: {
      root: DIALOG_CONTRACT.root,
      trigger: DIALOG_CONTRACT.trigger,
      portal: DIALOG_CONTRACT.portal,
      content: DIALOG_CONTRACT.content,
      overlay: DIALOG_CONTRACT.overlay,
      close: '[data-dialog-close]',
    },

    init() {
      document.querySelectorAll(this.selectors.root).forEach(root => {
        syncModalContract(root, DIALOG_CONTRACT, root.dataset.state === 'open');
      });
      syncModalScrollLock();
    },

    open(root) {
      const state = getState(root, { isAnimating: false });
      if (state.isAnimating) return;

      const { portal, content, overlay, trigger } = wireModalContract(root, DIALOG_CONTRACT);

      if (!portal || !content) return;

      root.dataset.state = 'open';
      root.style.setProperty('--proa-overlay-display', 'block');
      portal.style.display = 'block';
      portal.dataset.state = 'open';
      content.dataset.state = 'open';
      content.setAttribute('aria-hidden', 'false');
      if (overlay) overlay.dataset.state = 'open';
      if (trigger) trigger.setAttribute('aria-expanded', 'true');
      syncModalScrollLock();

      requestAnimationFrame(() => {
        if (root.hasAttribute('data-alert-dialog-root')) {
          const cancel = findOwnedDescendant(
            content,
            this.selectors.content,
            '[data-alert-dialog-cancel]'
          );
          if (cancel) {
            cancel.focus();
            return;
          }
        }
        content.focus();
      });
    },

    close(root) {
      const state = getState(root, { isAnimating: false });
      if (state.isAnimating || root.dataset.state !== 'open') return;

      const { portal, content, overlay, trigger } = wireModalContract(root, DIALOG_CONTRACT);

      if (!portal || !content) return;
      if (trigger) trigger.focus();

      state.isAnimating = true;
      root.dataset.state = 'closed';
      root.style.setProperty('--proa-overlay-display', 'none');
      portal.dataset.state = 'closed';
      content.dataset.state = 'closed';
      content.setAttribute('aria-hidden', 'true');
      if (overlay) overlay.dataset.state = 'closed';
      if (trigger) trigger.setAttribute('aria-expanded', 'false');
      syncModalScrollLock();

      const onAnimationEnd = () => {
        content.removeEventListener('animationend', onAnimationEnd);
        state.isAnimating = false;
        if (root.dataset.state === 'closed') {
          portal.style.display = 'none';
        }
      };
      content.addEventListener('animationend', onAnimationEnd);

      // Fallback timeout
      setTimeout(() => {
        if (state.isAnimating) {
          state.isAnimating = false;
          if (root.dataset.state === 'closed') {
            portal.style.display = 'none';
          }
        }
      }, 200);

    },

    toggle(root) {
      if (root.dataset.state === 'open') {
        this.close(root);
      } else {
        this.open(root);
      }
    },

    // Handle focus trap
    handleTab(root, e) {
      const content = findOwnedPart(root, this.selectors.root, this.selectors.content);
      if (!content) return;

      const focusable = Array.from(content.querySelectorAll(
        'button, [href], input, select, textarea, [tabindex]'
      )).filter(element => {
        if (element.disabled || element.tabIndex < 0) return false;
        if (element.closest('[hidden], [aria-hidden="true"]')) return false;
        return element.getClientRects().length > 0;
      });
      const active = document.activeElement;

      if (focusable.length === 0) {
        e.preventDefault();
        content.focus();
        return;
      }

      const first = focusable[0];
      const last = focusable[focusable.length - 1];

      if (active === content || !content.contains(active)) {
        e.preventDefault();
        (e.shiftKey ? last : first).focus();
      } else if (e.shiftKey && active === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && active === last) {
        e.preventDefault();
        first.focus();
      }
    },

    handleClick(target, e) {
      // Trigger click
      const trigger = target.closest(this.selectors.trigger);
      if (trigger) {
        const root = trigger.closest(this.selectors.root);
        if (root) this.toggle(root);
        return true;
      }

      // Close button click
      const closeBtn = target.closest(this.selectors.close);
      if (closeBtn) {
        const root = closeBtn.closest(this.selectors.root);
        if (root) this.close(root);
        return true;
      }

      // Overlay click (only direct clicks)
      const overlay = target.closest(this.selectors.overlay);
      if (overlay && e.target === overlay) {
        const root = overlay.closest(this.selectors.root);
        if (root && root.hasAttribute('data-alert-dialog-root')) {
          e.preventDefault();
          return true;
        }
        if (root) this.close(root);
        return true;
      }

      return false;
    },

    handleKeydown(e) {
      // Escape to close
      if (e.key === 'Escape') {
        const openRoot = document.querySelector(`${this.selectors.root}[data-state="open"]`);
        if (openRoot) {
          if (openRoot.hasAttribute('data-alert-dialog-root')) {
            e.preventDefault();
            return true;
          }
          const state = getState(openRoot, { isAnimating: false });
          if (!state.isAnimating) {
            this.close(openRoot);
            return true;
          }
        }
      }

      // Tab for focus trap
      if (e.key === 'Tab') {
        const openRoot = document.querySelector(`${this.selectors.root}[data-state="open"]`);
        if (openRoot) {
          this.handleTab(openRoot, e);
          return true;
        }
      }

      return false;
    }
  };

  initializeOnReady(() => Dialog.init());

  // ============================================================
  // POPOVER COMPONENT
  // ============================================================
  const Popover = {
    selectors: {
      root: '[data-popover-root]',
      trigger: '[data-popover-trigger]',
      content: '[data-popover-content]',
      close: '[data-popover-close]',
    },

    wire(root) {
      const trigger = findOwnedPart(root, this.selectors.root, this.selectors.trigger);
      const content = findOwnedPart(root, this.selectors.root, this.selectors.content);
      const id = ensureContractId(root, 'data-popover-id', 'popover');

      if (trigger && !trigger.id) trigger.id = `${id}--trigger`;
      if (content && !content.id) content.id = `${id}--content`;
      if (trigger && content) {
        trigger.setAttribute('aria-controls', content.id);
        content.setAttribute('aria-labelledby', trigger.id);
      }

      return { trigger, content };
    },

    sync(root, open) {
      const { trigger, content } = this.wire(root);
      const state = open ? 'open' : 'closed';

      root.dataset.state = state;
      root.style.setProperty('--proa-overlay-display', open ? 'block' : 'none');
      if (trigger) trigger.setAttribute('aria-expanded', open ? 'true' : 'false');
      if (content) {
        content.dataset.state = state;
        content.setAttribute('aria-hidden', open ? 'false' : 'true');
        content.style.display = open ? 'block' : 'none';
      }

      return { trigger, content };
    },

    init() {
      document.querySelectorAll(this.selectors.root).forEach(root => {
        this.sync(root, root.dataset.state === 'open');
        if (root.dataset.state === 'open') this.position(root);
      });
    },

    // Position the popover relative to root (which wraps the trigger)
    position(root) {
      const content = findOwnedPart(root, this.selectors.root, this.selectors.content);
      if (!content) return;

      const side = root.dataset.side || 'bottom';
      const align = root.dataset.align || 'center';
      const gap = 8; // pixels between trigger and content

      // Use offsetWidth/offsetHeight for reliable measurement
      const rootWidth = root.offsetWidth;
      const rootHeight = root.offsetHeight;
      const contentWidth = content.offsetWidth;
      const contentHeight = content.offsetHeight;

      let top = 0;
      let left = 0;

      // Calculate position based on side
      switch (side) {
        case 'top':
          top = -contentHeight - gap;
          break;
        case 'bottom':
          top = rootHeight + gap;
          break;
        case 'left':
          left = -contentWidth - gap;
          top = (rootHeight - contentHeight) / 2;
          break;
        case 'right':
          left = rootWidth + gap;
          top = (rootHeight - contentHeight) / 2;
          break;
      }

      // Calculate alignment for top/bottom
      if (side === 'top' || side === 'bottom') {
        switch (align) {
          case 'start':
            left = 0;
            break;
          case 'center':
            left = (rootWidth - contentWidth) / 2;
            break;
          case 'end':
            left = rootWidth - contentWidth;
            break;
        }
      }

      // Calculate alignment for left/right
      if (side === 'left' || side === 'right') {
        switch (align) {
          case 'start':
            top = 0;
            break;
          case 'center':
            top = (rootHeight - contentHeight) / 2;
            break;
          case 'end':
            top = rootHeight - contentHeight;
            break;
        }
      }

      content.style.left = `${left}px`;
      content.style.top = `${top}px`;
      content.dataset.side = side;
    },

    open(root) {
      const state = getState(root, { isAnimating: false });
      if (state.isAnimating) return;

      const { content, trigger } = this.wire(root);
      if (!content) return;

      root.dataset.state = 'open';
      root.style.setProperty('--proa-overlay-display', 'block');
      content.style.display = 'block';
      content.dataset.state = 'open';
      content.setAttribute('aria-hidden', 'false');
      if (trigger) trigger.setAttribute('aria-expanded', 'true');

      // Position after display
      this.position(root);

      requestAnimationFrame(() => content.focus());
    },

    close(root) {
      const state = getState(root, { isAnimating: false });
      if (state.isAnimating || root.dataset.state !== 'open') return;

      const { content, trigger } = this.wire(root);
      if (!content) return;

      state.isAnimating = true;
      root.dataset.state = 'closed';
      content.dataset.state = 'closed';
      content.setAttribute('aria-hidden', 'true');
      if (trigger) trigger.setAttribute('aria-expanded', 'false');

      const onAnimationEnd = () => {
        content.removeEventListener('animationend', onAnimationEnd);
        state.isAnimating = false;
        if (root.dataset.state === 'closed') {
          content.style.display = 'none';
          root.style.setProperty('--proa-overlay-display', 'none');
        }
      };
      content.addEventListener('animationend', onAnimationEnd);

      // Fallback timeout
      setTimeout(() => {
        if (state.isAnimating) {
          state.isAnimating = false;
          if (root.dataset.state === 'closed') {
            content.style.display = 'none';
            root.style.setProperty('--proa-overlay-display', 'none');
          }
        }
      }, 200);

      if (trigger) trigger.focus();
    },

    toggle(root) {
      if (root.dataset.state === 'open') {
        this.close(root);
      } else {
        this.open(root);
      }
    },

    handleClick(target, e) {
      // Trigger click
      const trigger = target.closest(this.selectors.trigger);
      if (trigger) {
        const root = trigger.closest(this.selectors.root);
        if (root) {
          this.toggle(root);
          return true;
        }
      }

      // Close button click
      const closeBtn = target.closest(this.selectors.close);
      if (closeBtn) {
        const root = closeBtn.closest(this.selectors.root);
        if (root) {
          this.close(root);
          return true;
        }
      }

      // Click outside to close
      const openRoot = document.querySelector(`${this.selectors.root}[data-state="open"]`);
      if (openRoot) {
        const { content, trigger } = this.wire(openRoot);
        // If click is outside both content and trigger, close
        if (content && !content.contains(target) && trigger && !trigger.contains(target)) {
          this.close(openRoot);
          // Don't return true - let other handlers process the click
        }
      }

      return false;
    },

    handleKeydown(e) {
      // Escape to close
      if (e.key === 'Escape') {
        const openRoot = document.querySelector(`${this.selectors.root}[data-state="open"]`);
        if (openRoot) {
          const state = getState(openRoot, { isAnimating: false });
          if (!state.isAnimating) {
            this.close(openRoot);
            return true;
          }
        }
      }

      return false;
    }
  };

  initializeOnReady(() => Popover.init());

  // ============================================================
  // TOOLTIP COMPONENT
  // ============================================================
  const Tooltip = {
    selectors: {
      root: '[data-tooltip-root]',
      trigger: '[data-tooltip-trigger]',
      content: '[data-tooltip-content]',
    },

    listenersInstalled: false,

    wire(root) {
      const trigger = findOwnedPart(root, this.selectors.root, this.selectors.trigger);
      const content = findOwnedPart(root, this.selectors.root, this.selectors.content);
      const id = ensureContractId(root, 'data-tooltip-id', 'tooltip');

      if (trigger && !trigger.id) trigger.id = `${id}--trigger`;
      if (content && !content.id) content.id = `${id}--content`;
      if (trigger && content) trigger.setAttribute('aria-describedby', content.id);

      return { trigger, content };
    },

    sync(root, open) {
      const { content } = this.wire(root);
      const state = open ? 'open' : 'closed';

      root.dataset.state = state;
      root.style.setProperty('--proa-overlay-display', open ? 'block' : 'none');
      if (content) {
        content.dataset.state = state;
        content.setAttribute('aria-hidden', open ? 'false' : 'true');
        content.style.display = open ? 'block' : 'none';
      }
    },

    // Position the tooltip relative to root (which wraps the trigger).
    // The offsets are root-relative, but collision handling has to happen in
    // viewport space: flip to the opposite side when the preferred side lacks
    // room, then clamp so the bubble never renders outside the window.
    position(root) {
      const content = findOwnedPart(root, this.selectors.root, this.selectors.content);
      if (!content) return;

      const preferredSide = root.dataset.side || 'top';
      const align = root.dataset.align || 'center';
      const gap = 6; // smaller gap for tooltips
      const margin = 8; // minimum clearance from the viewport edge

      const rootRect = root.getBoundingClientRect();
      const viewportWidth = window.innerWidth;
      const viewportHeight = window.innerHeight;

      // Cap the width before measuring: the recipe sizes the bubble to its
      // text (max-content), so this cap is what makes long text wrap instead
      // of overflowing the window.
      content.style.maxWidth = `${Math.max(0, viewportWidth - margin * 2)}px`;

      // Use offsetWidth/offsetHeight for more reliable measurement
      const rootWidth = root.offsetWidth;
      const rootHeight = root.offsetHeight;
      const contentWidth = content.offsetWidth;
      const contentHeight = content.offsetHeight;

      // Flip to the opposite side when the preferred side cannot fit the
      // bubble and the other side has more room.
      const spaceAbove = rootRect.top - gap - margin;
      const spaceBelow = viewportHeight - rootRect.bottom - gap - margin;
      const spaceLeft = rootRect.left - gap - margin;
      const spaceRight = viewportWidth - rootRect.right - gap - margin;

      let side = preferredSide;
      if (side === 'top' && contentHeight > spaceAbove && spaceBelow > spaceAbove) {
        side = 'bottom';
      } else if (side === 'bottom' && contentHeight > spaceBelow && spaceAbove > spaceBelow) {
        side = 'top';
      } else if (side === 'left' && contentWidth > spaceLeft && spaceRight > spaceLeft) {
        side = 'right';
      } else if (side === 'right' && contentWidth > spaceRight && spaceLeft > spaceRight) {
        side = 'left';
      }

      let top = 0;
      let left = 0;

      // Calculate position based on side
      switch (side) {
        case 'top':
          top = -contentHeight - gap;
          break;
        case 'bottom':
          top = rootHeight + gap;
          break;
        case 'left':
          left = -contentWidth - gap;
          top = (rootHeight - contentHeight) / 2;
          break;
        case 'right':
          left = rootWidth + gap;
          top = (rootHeight - contentHeight) / 2;
          break;
      }

      // Calculate alignment for top/bottom
      if (side === 'top' || side === 'bottom') {
        switch (align) {
          case 'start':
            left = 0;
            break;
          case 'center':
            left = (rootWidth - contentWidth) / 2;
            break;
          case 'end':
            left = rootWidth - contentWidth;
            break;
        }
      }

      // Calculate alignment for left/right
      if (side === 'left' || side === 'right') {
        switch (align) {
          case 'start':
            top = 0;
            break;
          case 'center':
            top = (rootHeight - contentHeight) / 2;
            break;
          case 'end':
            top = rootHeight - contentHeight;
            break;
        }
      }

      // Clamp to the viewport, converting the viewport-space bounds into
      // root-relative offsets.
      left = Math.max(
        margin - rootRect.left,
        Math.min(left, viewportWidth - margin - contentWidth - rootRect.left)
      );
      top = Math.max(
        margin - rootRect.top,
        Math.min(top, viewportHeight - margin - contentHeight - rootRect.top)
      );

      content.style.left = `${left}px`;
      content.style.top = `${top}px`;
      content.dataset.side = side;
    },

    show(root) {
      const state = getState(root, { isAnimating: false, showTimeout: null, hideTimeout: null });

      // Clear any pending hide
      if (state.hideTimeout) {
        clearTimeout(state.hideTimeout);
        state.hideTimeout = null;
      }

      // If already open or animating open, do nothing
      if (root.dataset.state === 'open') return;

      const content = findOwnedPart(root, this.selectors.root, this.selectors.content);
      if (!content) return;

      // Get delay from data attribute (default 200ms)
      const delay = parseInt(root.dataset.delay || '200', 10);

      // Set timeout for showing
      state.showTimeout = setTimeout(() => {
        this.sync(root, true);

        // Position after display
        this.position(root);
      }, delay);
    },

    hide(root) {
      const state = getState(root, { isAnimating: false, showTimeout: null, hideTimeout: null });

      // Clear any pending show
      if (state.showTimeout) {
        clearTimeout(state.showTimeout);
        state.showTimeout = null;
      }

      // If already closed, do nothing
      if (root.dataset.state !== 'open') return;

      const content = findOwnedPart(root, this.selectors.root, this.selectors.content);
      if (!content) return;

      // Hide immediately (no delay on hide)
      root.dataset.state = 'closed';
      content.dataset.state = 'closed';
      content.setAttribute('aria-hidden', 'true');

      const onAnimationEnd = () => {
        content.removeEventListener('animationend', onAnimationEnd);
        if (root.dataset.state === 'closed') {
          content.style.display = 'none';
          root.style.setProperty('--proa-overlay-display', 'none');
        }
      };
      content.addEventListener('animationend', onAnimationEnd);

      // Fallback timeout
      setTimeout(() => {
        if (root.dataset.state === 'closed') {
          content.style.display = 'none';
          root.style.setProperty('--proa-overlay-display', 'none');
        }
      }, 150);
    },

    // Setup hover/focus listeners on init
    init() {
      document.querySelectorAll(this.selectors.root).forEach(root => {
        this.sync(root, root.dataset.state === 'open');
      });

      if (this.listenersInstalled) return;
      this.listenersInstalled = true;

      // Mouseenter on trigger - show tooltip
      document.addEventListener('mouseenter', (e) => {
        if (!e.target || !e.target.closest) return;
        const trigger = e.target.closest(this.selectors.trigger);
        if (trigger) {
          const root = trigger.closest(this.selectors.root);
          if (root) this.show(root);
        }
      }, true);

      // Mouseleave on trigger - hide tooltip
      document.addEventListener('mouseleave', (e) => {
        if (!e.target || !e.target.closest) return;
        const trigger = e.target.closest(this.selectors.trigger);
        if (trigger) {
          const root = trigger.closest(this.selectors.root);
          if (root) this.hide(root);
        }
      }, true);

      // Focus on trigger - show tooltip
      document.addEventListener('focusin', (e) => {
        if (!e.target || !e.target.closest) return;
        const trigger = e.target.closest(this.selectors.trigger);
        if (trigger) {
          const root = trigger.closest(this.selectors.root);
          if (root) this.show(root);
        }
      });

      // Blur on trigger - hide tooltip
      document.addEventListener('focusout', (e) => {
        if (!e.target || !e.target.closest) return;
        const trigger = e.target.closest(this.selectors.trigger);
        if (trigger) {
          const root = trigger.closest(this.selectors.root);
          if (root) this.hide(root);
        }
      });
    },

    handleKeydown(e) {
      // Escape to hide all tooltips
      if (e.key === 'Escape') {
        const openRoot = document.querySelector(`${this.selectors.root}[data-state="open"]`);
        if (openRoot) {
          this.hide(openRoot);
          return true;
        }
      }
      return false;
    }
  };

  // Initialize tooltip hover/focus listeners
  initializeOnReady(() => Tooltip.init());

  // ============================================================
  // THEME TOGGLE COMPONENT
  // ============================================================
  const ThemeToggle = {
    selectors: {
      button: '[data-theme-toggle-button]',
    },

    // Shared with the docs-site chrome toggle: same storage key, same root
    // class, so component instances and host-page toggles stay in agreement.
    storageKey: 'theme',

    apply(dark) {
      const root = document.documentElement;
      root.classList.toggle('dark', dark);
      root.style.colorScheme = dark ? 'dark' : 'light';
    },

    // Stored choice wins; otherwise follow the OS preference.
    preferredDark() {
      let saved = null;
      try { saved = localStorage.getItem(this.storageKey); } catch (_) { /* storage unavailable */ }
      if (saved === 'dark') return true;
      if (saved === 'light') return false;
      return !!(window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches);
    },

    toggle() {
      const dark = !document.documentElement.classList.contains('dark');
      this.apply(dark);
      try { localStorage.setItem(this.storageKey, dark ? 'dark' : 'light'); } catch (_) { /* storage unavailable */ }
    },

    handleClick(target) {
      if (!target || !target.closest) return false;
      const button = target.closest(this.selectors.button);
      if (!button) return false;
      this.toggle();
      return true;
    },

    // Apply the stored/OS theme on load — but only on pages that actually
    // render a toggle. Hosts without one keep full control of the root class.
    init() {
      if (!document.querySelector(this.selectors.button)) return;
      this.apply(this.preferredDark());
    },
  };

  initializeOnReady(() => ThemeToggle.init());

  // ============================================================
  // ACCORDION COMPONENT
  // ============================================================
  const Accordion = {
    selectors: {
      root: '[data-accordion-root]',
      item: '[data-accordion-item]',
      trigger: '[data-accordion-trigger]',
      content: '[data-accordion-content]',
    },

    open(item) {
      const root = item.closest(this.selectors.root);
      if (!root) return;

      const state = getState(item, { isAnimating: false });
      if (state.isAnimating) return;

      // Check if disabled
      if (item.dataset.disabled === 'true') return;

      const content = item.querySelector(this.selectors.content);
      const trigger = item.querySelector(this.selectors.trigger);
      if (!content) return;

      // For single mode, close other items first
      const accordionType = root.dataset.type || 'single';
      if (accordionType === 'single') {
        const openItems = root.querySelectorAll(`${this.selectors.item}[data-state="open"]`);
        openItems.forEach(openItem => {
          if (openItem !== item) {
            this.close(openItem);
          }
        });
      }

      // Update state
      item.dataset.state = 'open';
      content.dataset.state = 'open';
      content.hidden = false;
      if (trigger) trigger.setAttribute('aria-expanded', 'true');

      // Store content height for CSS animation
      const height = content.scrollHeight;
      content.style.setProperty('--proa-accordion-height', `${height}px`);

      // Update root value
      this.updateRootValue(root);
    },

    close(item) {
      const root = item.closest(this.selectors.root);
      if (!root) return;

      const state = getState(item, { isAnimating: false });
      if (state.isAnimating || item.dataset.state !== 'open') return;

      // In single non-collapsible mode, prevent closing
      const accordionType = root.dataset.type || 'single';
      const collapsible = root.dataset.collapsible === 'true';
      if (accordionType === 'single' && !collapsible) {
        // Check if this is the only open item
        const openItems = root.querySelectorAll(`${this.selectors.item}[data-state="open"]`);
        if (openItems.length === 1 && openItems[0] === item) {
          return; // Can't close the only open item
        }
      }

      const content = item.querySelector(this.selectors.content);
      const trigger = item.querySelector(this.selectors.trigger);
      if (!content) return;

      state.isAnimating = true;
      item.dataset.state = 'closed';
      content.dataset.state = 'closed';
      if (trigger) trigger.setAttribute('aria-expanded', 'false');

      const onAnimationEnd = () => {
        content.removeEventListener('animationend', onAnimationEnd);
        state.isAnimating = false;
        if (item.dataset.state === 'closed') {
          content.hidden = true;
        }
      };
      content.addEventListener('animationend', onAnimationEnd);

      // Fallback timeout
      setTimeout(() => {
        if (state.isAnimating) {
          state.isAnimating = false;
          if (item.dataset.state === 'closed') {
            content.hidden = true;
          }
        }
      }, 200);

      // Update root value
      this.updateRootValue(root);
    },

    toggle(item) {
      if (item.dataset.state === 'open') {
        this.close(item);
      } else {
        this.open(item);
      }
    },

    updateRootValue(root) {
      const openItems = root.querySelectorAll(`${this.selectors.item}[data-state="open"]`);
      const values = Array.from(openItems).map(item => item.dataset.value).filter(Boolean);
      root.dataset.value = values.join(',');
    },

    // Initialize default open items
    init() {
      document.querySelectorAll(this.selectors.root).forEach(root => {
        const defaultValue = root.dataset.value;
        if (defaultValue) {
          const values = defaultValue.split(',').map(v => v.trim());
          values.forEach(value => {
            const item = root.querySelector(`${this.selectors.item}[data-value="${value}"]`);
            if (item) {
              // Open without animation on init
              const content = item.querySelector(this.selectors.content);
              const trigger = item.querySelector(this.selectors.trigger);
              if (content) {
                item.dataset.state = 'open';
                content.dataset.state = 'open';
                content.hidden = false;
                const height = content.scrollHeight;
                content.style.setProperty('--proa-accordion-height', `${height}px`);
              }
              if (trigger) trigger.setAttribute('aria-expanded', 'true');
            }
          });
        }
      });
    },

    handleClick(target, e) {
      const trigger = target.closest(this.selectors.trigger);
      if (trigger) {
        const item = trigger.closest(this.selectors.item);
        if (item) {
          this.toggle(item);
          return true;
        }
      }
      return false;
    },

    handleKeydown(e) {
      const trigger = e.target.closest(this.selectors.trigger);
      if (!trigger) return false;

      const item = trigger.closest(this.selectors.item);
      const root = item?.closest(this.selectors.root);
      if (!root) return false;

      const items = Array.from(root.querySelectorAll(this.selectors.item));
      const triggers = items.map(i => i.querySelector(this.selectors.trigger)).filter(Boolean);
      const currentIndex = triggers.indexOf(trigger);

      switch (e.key) {
        case 'ArrowDown':
          e.preventDefault();
          const nextIndex = (currentIndex + 1) % triggers.length;
          triggers[nextIndex]?.focus();
          return true;

        case 'ArrowUp':
          e.preventDefault();
          const prevIndex = (currentIndex - 1 + triggers.length) % triggers.length;
          triggers[prevIndex]?.focus();
          return true;

        case 'Home':
          e.preventDefault();
          triggers[0]?.focus();
          return true;

        case 'End':
          e.preventDefault();
          triggers[triggers.length - 1]?.focus();
          return true;

        case 'Enter':
        case ' ':
          e.preventDefault();
          this.toggle(item);
          return true;
      }

      return false;
    }
  };

  // Initialize accordion default values
  Accordion.init();

  // ============================================================
  // COLLAPSIBLE COMPONENT
  // ============================================================
  const Collapsible = {
    selectors: {
      root: '[data-collapsible-root]',
      trigger: '[data-collapsible-trigger]',
      content: '[data-collapsible-content]',
    },

    init() {
      document.querySelectorAll(this.selectors.root).forEach(root => {
        this.sync(root, root.dataset.state === 'open');
      });
    },

    sync(root, open) {
      const state = open ? 'open' : 'closed';
      const trigger = findOwnedPart(root, this.selectors.root, this.selectors.trigger);
      const content = findOwnedPart(root, this.selectors.root, this.selectors.content);
      const id = ensureContractId(root, 'data-collapsible-id', 'collapsible');
      const isDisabled = root.dataset.disabled === 'true';

      root.dataset.state = state;
      root.style.setProperty('--proa-collapsible-display', open ? 'block' : 'none');

      if (trigger) {
        if (!trigger.id) trigger.id = `${id}--trigger`;
        trigger.setAttribute('aria-expanded', open ? 'true' : 'false');
        trigger.setAttribute('aria-disabled', isDisabled ? 'true' : 'false');
        if ('disabled' in trigger) trigger.disabled = isDisabled;
      }

      if (content) {
        if (!content.id) content.id = `${id}--content`;
        content.dataset.state = state;
        content.setAttribute('aria-hidden', open ? 'false' : 'true');
        content.hidden = !open;
      }

      if (trigger && content) trigger.setAttribute('aria-controls', content.id);
    },

    toggle(root) {
      if (root.dataset.disabled === 'true') return;
      this.sync(root, root.dataset.state !== 'open');
    },

    handleClick(target) {
      const trigger = target.closest(this.selectors.trigger);
      if (!trigger) return false;
      const root = trigger.closest(this.selectors.root);
      if (!root) return false;
      this.toggle(root);
      return true;
    },

    handleKeydown(e) {
      if (e.key !== 'Enter' && e.key !== ' ') return false;
      const trigger = e.target.closest(this.selectors.trigger);
      if (!trigger) return false;
      const root = trigger.closest(this.selectors.root);
      if (!root) return false;
      e.preventDefault();
      this.toggle(root);
      return true;
    },
  };

  Collapsible.init();
  document.addEventListener('DOMContentLoaded', () => Collapsible.init());
  document.addEventListener('turbo:load', () => Collapsible.init());
  document.addEventListener('turbo:render', () => Collapsible.init());

  // ============================================================
  // AVATAR COMPONENT
  // ============================================================
  const Avatar = {
    selectors: {
      root: '[data-avatar]',
      image: '[data-avatar-image]',
      fallback: '[data-avatar-fallback]',
    },

    // Initialize all avatars on page load
    init() {
      document.querySelectorAll(this.selectors.root).forEach(avatar => {
        this.initAvatar(avatar);
      });
    },

    initAvatar(avatar) {
      // Idempotency guard: this attaches load/error listeners to the image, and
      // the bootstrap below re-runs on every Turbo navigation. Without this a
      // morph refresh would stack duplicate listeners on the same element.
      if (avatar._avatarInit) return;
      avatar._avatarInit = true;

      const image = avatar.querySelector(this.selectors.image);
      const fallback = avatar.querySelector(this.selectors.fallback);

      if (!fallback) return;

      // If no image, show fallback immediately
      if (!image) {
        fallback.style.display = '';
        return;
      }

      // Hide fallback initially (image will show)
      fallback.style.display = 'none';

      // Handle image load error - show fallback
      image.addEventListener('error', () => {
        image.style.display = 'none';
        fallback.style.display = '';
      });

      // Handle successful load - ensure image is shown
      image.addEventListener('load', () => {
        image.style.display = '';
        fallback.style.display = 'none';
      });

      // Check if image is already loaded or errored (cached images)
      if (image.complete) {
        if (image.naturalWidth === 0) {
          // Image failed to load
          image.style.display = 'none';
          fallback.style.display = '';
        } else {
          // Image loaded successfully
          fallback.style.display = 'none';
        }
      }
    },
  };

  // Initialize avatars on DOM ready. /proa.js is a <head> script and Turbo
  // never re-evaluates it, nor does DOMContentLoaded fire again, so bootstrap
  // on the Turbo lifecycle events too. Avatar.initAvatar is guarded above.
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', () => Avatar.init());
  } else {
    Avatar.init();
  }
  document.addEventListener('turbo:load', () => Avatar.init());
  document.addEventListener('turbo:render', () => Avatar.init());
  document.addEventListener('turbo:frame-load', () => Avatar.init());

  // ============================================================
  // TABS COMPONENT
  // ============================================================
  const Tabs = {
    selectors: {
      root: '[data-tabs-root]',
      list: '[data-tabs-list]',
      trigger: '[data-tabs-trigger]',
      content: '[data-tabs-content]',
    },

    wire(root) {
      const baseId = ensureContractId(root, 'data-tabs-id', 'tabs');
      const triggers = Array.from(root.querySelectorAll(this.selectors.trigger));
      const contents = Array.from(root.querySelectorAll(this.selectors.content));

      triggers.forEach((trigger, index) => {
        const value = trigger.dataset.value;
        const content = contents.find(candidate => candidate.dataset.value === value);
        if (!trigger.id) trigger.id = `${baseId}--trigger-${index + 1}`;
        if (!content) return;
        if (!content.id) content.id = `${baseId}--panel-${index + 1}`;
        trigger.setAttribute('aria-controls', content.id);
        content.setAttribute('aria-labelledby', trigger.id);
      });

      return { triggers, contents };
    },

    // Activate a specific tab by its trigger
    activate(trigger) {
      const root = trigger.closest(this.selectors.root);
      if (!root) return;

      // Check if disabled
      if (trigger.dataset.disabled === 'true') return;

      const value = trigger.dataset.value;
      if (!value) return;

      // Deactivate all triggers and content in this root
      const { triggers, contents } = this.wire(root);

      triggers.forEach(t => {
        t.dataset.state = 'inactive';
        t.setAttribute('aria-selected', 'false');
        t.setAttribute('tabindex', '-1');
      });

      contents.forEach(c => {
        c.dataset.state = 'inactive';
        c.hidden = true;
      });

      // Activate the selected trigger
      trigger.dataset.state = 'active';
      trigger.setAttribute('aria-selected', 'true');
      trigger.setAttribute('tabindex', '0');

      // Activate the matching content
      const content = contents.find(candidate => candidate.dataset.value === value);
      if (content) {
        content.dataset.state = 'active';
        content.hidden = false;
      }

      // Update root value
      root.dataset.value = value;
    },

    // Initialize tabs with default value
    init() {
      document.querySelectorAll(this.selectors.root).forEach(root => {
        const defaultValue = root.dataset.value;
        const { triggers } = this.wire(root);

        if (triggers.length === 0) return;

        // Idempotency guard: the bootstrap below re-runs on every Turbo
        // navigation. A morph refresh preserves the live DOM, so re-activating
        // the default here would silently discard the user's current tab.
        if (triggers.some(trigger => trigger.dataset.state === 'active')) return;

        // Find the trigger to activate
        let triggerToActivate = null;

        if (defaultValue) {
          // Find trigger matching default value
          triggerToActivate = triggers.find(trigger => trigger.dataset.value === defaultValue);
        }

        // If no default or not found, activate first non-disabled trigger
        if (!triggerToActivate) {
          triggerToActivate = triggers.find(trigger => trigger.dataset.disabled !== 'true');
        }

        if (triggerToActivate) {
          this.activate(triggerToActivate);
        }
      });
    },

    // Get all triggers in a tabs list
    getTriggers(root) {
      const list = root.querySelector(this.selectors.list);
      if (!list) return [];
      return Array.from(list.querySelectorAll(this.selectors.trigger));
    },

    // Get non-disabled triggers
    getEnabledTriggers(root) {
      return this.getTriggers(root).filter(t => t.dataset.disabled !== 'true');
    },

    handleClick(target, e) {
      const trigger = target.closest(this.selectors.trigger);
      if (trigger) {
        this.activate(trigger);
        return true;
      }
      return false;
    },

    handleKeydown(e) {
      const trigger = e.target.closest(this.selectors.trigger);
      if (!trigger) return false;

      const root = trigger.closest(this.selectors.root);
      if (!root) return false;

      const list = root.querySelector(this.selectors.list);
      if (!list) return false;

      const orientation = root.dataset.orientation || 'horizontal';
      const activationMode = root.dataset.activationMode || 'automatic';
      const loop = list.dataset.loop !== 'false'; // Default to true

      const triggers = this.getEnabledTriggers(root);
      const currentIndex = triggers.indexOf(trigger);

      // Determine which arrow keys to use based on orientation
      const nextKey = orientation === 'vertical' ? 'ArrowDown' : 'ArrowRight';
      const prevKey = orientation === 'vertical' ? 'ArrowUp' : 'ArrowLeft';

      let newIndex = -1;

      switch (e.key) {
        case nextKey:
          e.preventDefault();
          if (loop) {
            newIndex = (currentIndex + 1) % triggers.length;
          } else {
            newIndex = Math.min(currentIndex + 1, triggers.length - 1);
          }
          break;

        case prevKey:
          e.preventDefault();
          if (loop) {
            newIndex = (currentIndex - 1 + triggers.length) % triggers.length;
          } else {
            newIndex = Math.max(currentIndex - 1, 0);
          }
          break;

        case 'Home':
          e.preventDefault();
          newIndex = 0;
          break;

        case 'End':
          e.preventDefault();
          newIndex = triggers.length - 1;
          break;

        case 'Enter':
        case ' ':
          // Manual mode: activate on Enter/Space
          if (activationMode === 'manual') {
            e.preventDefault();
            this.activate(trigger);
            return true;
          }
          break;

        default:
          return false;
      }

      if (newIndex >= 0 && newIndex < triggers.length) {
        const newTrigger = triggers[newIndex];
        newTrigger.focus();

        // Automatic mode: activate on focus
        if (activationMode === 'automatic') {
          this.activate(newTrigger);
        }
        return true;
      }

      return false;
    }
  };

  // Initialize tabs on page load. /proa.js is a <head> script and Turbo never
  // re-evaluates it, nor does DOMContentLoaded fire again, so bootstrap on the
  // Turbo lifecycle events too. Tabs.init is guarded above.
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', () => Tabs.init());
  } else {
    Tabs.init();
  }
  document.addEventListener('turbo:load', () => Tabs.init());
  document.addEventListener('turbo:render', () => Tabs.init());
  document.addEventListener('turbo:frame-load', () => Tabs.init());

  // ============================================================
  // TOGGLE COMPONENT
  // ============================================================
  const Toggle = {
    selectors: {
      toggle: '[data-toggle]',
    },

    // Toggle the pressed state
    toggle(button) {
      // Check if disabled
      if (button.disabled || button.dataset.disabled === 'true') return;

      const isPressed = button.dataset.state === 'on';
      const newState = !isPressed;

      // Update state
      button.dataset.state = newState ? 'on' : 'off';
      button.setAttribute('aria-pressed', String(newState));

      // Dispatch change event
      button.dispatchEvent(new CustomEvent('change', {
        bubbles: true,
        detail: { pressed: newState }
      }));
    },

    handleClick(target, e) {
      const toggle = target.closest(this.selectors.toggle);
      if (toggle) {
        this.toggle(toggle);
        return true;
      }
      return false;
    },

    handleKeydown(e) {
      const toggle = e.target.closest(this.selectors.toggle);
      if (!toggle) return false;

      // Toggle on Enter or Space
      if (e.key === 'Enter' || e.key === ' ') {
        e.preventDefault();
        this.toggle(toggle);
        return true;
      }

      return false;
    }
  };

  // ============================================================
  // SWITCH COMPONENT
  // ============================================================
  const Switch = {
    selectors: {
      switch: '[data-switch]',
      thumb: '[data-switch-thumb]',
    },

    // Toggle the switch state
    toggle(button) {
      // Check if disabled
      if (button.disabled) return;

      const isChecked = button.dataset.state === 'checked';
      const newState = !isChecked;
      const stateStr = newState ? 'checked' : 'unchecked';

      // Update button state
      button.dataset.state = stateStr;
      button.setAttribute('aria-checked', String(newState));

      // Update thumb state
      const thumb = button.querySelector(this.selectors.thumb);
      if (thumb) {
        thumb.dataset.state = stateStr;
      }

      // Dispatch change event
      button.dispatchEvent(new CustomEvent('change', {
        bubbles: true,
        detail: { checked: newState }
      }));
    },

    handleClick(target, e) {
      const sw = target.closest(this.selectors.switch);
      if (sw) {
        this.toggle(sw);
        return true;
      }
      return false;
    },

    handleKeydown(e) {
      const sw = e.target.closest(this.selectors.switch);
      if (!sw) return false;

      // Toggle on Enter or Space
      if (e.key === 'Enter' || e.key === ' ') {
        e.preventDefault();
        this.toggle(sw);
        return true;
      }

      return false;
    }
  };

  // ============================================================
  // GLOBAL EVENT DELEGATION (FREE COMPONENTS)
  // ============================================================

  document.addEventListener('click', (event) => {
    const target = event.target;
    if (Dialog.handleClick(target, event)) return;
    if (Popover.handleClick(target, event)) return;
    if (Accordion.handleClick(target, event)) return;
    if (Collapsible.handleClick(target, event)) return;
    if (Tabs.handleClick(target, event)) return;
    if (Toggle.handleClick(target, event)) return;
    if (ThemeToggle.handleClick(target)) return;
    if (Switch.handleClick(target, event)) return;
  });

  document.addEventListener('keydown', (event) => {
    if (Dialog.handleKeydown(event)) return;
    if (Popover.handleKeydown(event)) return;
    if (Tooltip.handleKeydown(event)) return;
    if (Accordion.handleKeydown(event)) return;
    if (Collapsible.handleKeydown(event)) return;
    if (Tabs.handleKeydown(event)) return;
    if (Toggle.handleKeydown(event)) return;
    if (Switch.handleKeydown(event)) return;
  });

  window.Proa = {
    Accordion,
    Avatar,
    Collapsible,
    Dialog,
    Popover,
    Switch,
    Tabs,
    ThemeToggle,
    Toggle,
    Tooltip,
  };

})();
