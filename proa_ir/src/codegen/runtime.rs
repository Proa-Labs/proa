// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Runtime library generation
//!
//! Generates the shared proa.js runtime that gets loaded once and cached.
//! This contains all the helper functions and the Form class.

/// The version of the runtime (used for cache busting)
pub const RUNTIME_VERSION: &str = "0.1.0";

/// Default CDN URL for the runtime
pub const DEFAULT_CDN_URL: &str = "https://cdn.proa.so";

/// Get the full URL for the runtime script
///
/// If cdn_base ends with .js, use it as-is (direct path to runtime).
/// Otherwise, treat it as a CDN base URL and append version path.
pub fn runtime_url(cdn_base: Option<&str>) -> String {
    match cdn_base {
        Some(url) if url.ends_with(".js") => url.to_string(),
        Some(base) => format!("{}/v{}/proa.min.js", base, RUNTIME_VERSION),
        None => format!("{}/v{}/proa.min.js", DEFAULT_CDN_URL, RUNTIME_VERSION),
    }
}

/// Generate the runtime JavaScript library
///
/// This should be hosted on a CDN and loaded once per page.
pub fn generate_runtime() -> String {
    RUNTIME_JS.to_string()
}

/// Generate a minified version of the runtime
pub fn generate_runtime_minified() -> String {
    // For now, just return the non-minified version
    // In production, you'd want to use a proper minifier
    minify_js(RUNTIME_JS)
}

/// Simple JS minifier (basic, not production-grade)
fn minify_js(js: &str) -> String {
    let mut result = String::with_capacity(js.len());
    let mut chars = js.chars().peekable();
    let mut in_string = false;
    let mut string_char = '"';
    let mut prev_char = ' ';
    let mut in_single_comment = false;
    let mut in_multi_comment = false;

    while let Some(c) = chars.next() {
        // Handle comments
        if !in_string {
            if c == '/' {
                if let Some(&next) = chars.peek() {
                    if next == '/' && !in_multi_comment {
                        in_single_comment = true;
                        continue;
                    } else if next == '*' && !in_single_comment {
                        in_multi_comment = true;
                        chars.next();
                        continue;
                    }
                }
            }
            if in_single_comment {
                if c == '\n' {
                    in_single_comment = false;
                }
                continue;
            }
            if in_multi_comment {
                if c == '*' {
                    if let Some(&next) = chars.peek() {
                        if next == '/' {
                            in_multi_comment = false;
                            chars.next();
                        }
                    }
                }
                continue;
            }
        }

        // Handle strings
        if (c == '"' || c == '\'' || c == '`') && prev_char != '\\' {
            if in_string && c == string_char {
                in_string = false;
            } else if !in_string {
                in_string = true;
                string_char = c;
            }
        }

        if in_string {
            result.push(c);
            prev_char = c;
            continue;
        }

        // Skip unnecessary whitespace
        if c.is_whitespace() {
            if !result.is_empty() {
                let last = result.chars().last().unwrap_or(' ');
                // Keep space only if needed between identifiers/keywords
                if last.is_alphanumeric() || last == '_' || last == '$' {
                    if let Some(&next) = chars.peek() {
                        if next.is_alphanumeric() || next == '_' || next == '$' {
                            result.push(' ');
                        }
                    }
                }
            }
            prev_char = c;
            continue;
        }

        result.push(c);
        prev_char = c;
    }

    result
}

/// The runtime JavaScript source
const RUNTIME_JS: &str = r#"/**
 * Proa Forms Runtime
 * Shared library for form handling, validation, and reactivity
 */

// =============================================================================
// Utilities
// =============================================================================

/** Query selector shorthand */
export const $ = (s, root = document) => root.querySelector(s);

/** Query selector all shorthand */
export const $$ = (s, root = document) => root.querySelectorAll(s);

/** Debounce timers storage */
const debounceTimers = new Map();

/** Debounce a function call */
export function debounce(key, ms, fn) {
  if (debounceTimers.has(key)) {
    clearTimeout(debounceTimers.get(key));
  }
  debounceTimers.set(key, setTimeout(() => {
    debounceTimers.delete(key);
    fn();
  }, ms));
}

/** Throttle timers storage */
const throttleTimers = new Map();

/** Throttle a function call */
export function throttle(key, ms, fn) {
  if (throttleTimers.has(key)) return;
  fn();
  throttleTimers.set(key, setTimeout(() => {
    throttleTimers.delete(key);
  }, ms));
}

// =============================================================================
// Input Formatters
// =============================================================================

/** Format phone number as (123) 456-7890 */
export function formatPhone(value) {
  const digits = value.replace(/\D/g, '').slice(0, 10);
  if (digits.length === 0) return '';
  if (digits.length <= 3) return `(${digits}`;
  if (digits.length <= 6) return `(${digits.slice(0, 3)}) ${digits.slice(3)}`;
  return `(${digits.slice(0, 3)}) ${digits.slice(3, 6)}-${digits.slice(6)}`;
}

/** Format credit card as 1234 5678 9012 3456 */
export function formatCreditCard(value) {
  const digits = value.replace(/\D/g, '').slice(0, 16);
  const parts = [];
  for (let i = 0; i < digits.length; i += 4) {
    parts.push(digits.slice(i, i + 4));
  }
  return parts.join(' ');
}

/** Format expiry as MM/YY with month validation */
export function formatExpiry(value) {
  const digits = value.replace(/\D/g, '').slice(0, 4);
  if (digits.length === 0) return '';

  // Validate and constrain month (01-12)
  let month = digits.slice(0, 2);
  if (digits.length === 1) {
    // If first digit is > 1, assume they meant 0X
    if (parseInt(digits[0]) > 1) {
      month = '0' + digits[0];
    } else {
      return digits;
    }
  } else if (digits.length >= 2) {
    const monthNum = parseInt(month);
    if (monthNum === 0) month = '01';
    else if (monthNum > 12) month = '12';
  }

  if (digits.length <= 2) return month;
  return `${month}/${digits.slice(2)}`;
}

/** Format SSN as 123-45-6789 */
export function formatSSN(value) {
  const digits = value.replace(/\D/g, '').slice(0, 9);
  if (digits.length <= 3) return digits;
  if (digits.length <= 5) return `${digits.slice(0, 3)}-${digits.slice(3)}`;
  return `${digits.slice(0, 3)}-${digits.slice(3, 5)}-${digits.slice(5)}`;
}

/** Format zip code as 12345 or 12345-6789 */
export function formatZipCode(value) {
  const digits = value.replace(/\D/g, '').slice(0, 9);
  if (digits.length <= 5) return digits;
  return `${digits.slice(0, 5)}-${digits.slice(5)}`;
}

/** Format CVC as 3-4 digits only */
export function formatCVC(value) {
  return value.replace(/\D/g, '').slice(0, 4);
}

/** Format name - only letters, spaces, hyphens, apostrophes, periods */
export function formatName(value, maxLen = 50) {
  return value.replace(/[^a-zA-Z\s\-'\.]/g, '').slice(0, maxLen);
}

/** Get formatter function by name */
export function getFormatter(type) {
  const formatters = {
    phone: formatPhone,
    credit_card: formatCreditCard,
    expiry: formatExpiry,
    ssn: formatSSN,
    zip_code: formatZipCode,
    cvc: formatCVC,
    name: formatName,
  };
  return formatters[type];
}

// =============================================================================
// Credit Card Detection
// =============================================================================

/** Card type patterns based on IIN ranges */
const CARD_PATTERNS = [
  { type: 'visa', pattern: /^4/ },
  { type: 'mastercard', pattern: /^(5[1-5]|2[2-7])/ },
  { type: 'amex', pattern: /^3[47]/ },
  { type: 'discover', pattern: /^(6011|65|64[4-9])/ },
  { type: 'diners', pattern: /^(36|38|30[0-5])/ },
  { type: 'jcb', pattern: /^35/ },
];

/** Detect credit card type from number */
export function detectCardType(number) {
  const digits = number.replace(/\D/g, '');
  for (const { type, pattern } of CARD_PATTERNS) {
    if (pattern.test(digits)) {
      return type;
    }
  }
  return 'unknown';
}

/** Get card type display info */
export function getCardInfo(type) {
  const cards = {
    visa: { name: 'Visa', icon: '💳', color: '#1A1F71' },
    mastercard: { name: 'Mastercard', icon: '💳', color: '#EB001B' },
    amex: { name: 'American Express', icon: '💳', color: '#006FCF' },
    discover: { name: 'Discover', icon: '💳', color: '#FF6000' },
    diners: { name: 'Diners Club', icon: '💳', color: '#004A97' },
    jcb: { name: 'JCB', icon: '💳', color: '#0E4C96' },
    unknown: { name: 'Card', icon: '💳', color: '#666' },
  };
  return cards[type] || cards.unknown;
}

// =============================================================================
// Toast Notifications
// =============================================================================

/** Show a toast notification */
export function toast(message, level = 'info', duration = 3000) {
  // Try to use existing toast system if available
  if (typeof window.toast === 'function') {
    window.toast(message, level, duration);
    return;
  }

  // Fallback: create a simple toast
  const container = $('#proa-toasts') || createToastContainer();
  const toast = document.createElement('div');
  toast.className = `proa-toast proa-toast-${level}`;
  toast.textContent = message;
  toast.setAttribute('role', 'alert');
  container.appendChild(toast);

  // Auto-remove
  setTimeout(() => {
    toast.classList.add('proa-toast-exit');
    setTimeout(() => toast.remove(), 300);
  }, duration);
}

function createToastContainer() {
  const container = document.createElement('div');
  container.id = 'proa-toasts';
  container.setAttribute('aria-live', 'polite');
  document.body.appendChild(container);
  return container;
}

// =============================================================================
// Validation Error
// =============================================================================

export class ValidationError extends Error {
  constructor(message, errors) {
    super(message);
    this.name = 'ValidationError';
    this.errors = errors;
  }
}

// =============================================================================
// Form Class
// =============================================================================

export class Form {
  /** @type {string} */
  id;

  /** @type {HTMLFormElement|null} */
  el;

  /** @type {Record<string, any>} */
  signals;

  /** @type {Record<string, string>} */
  fields;

  /** @type {Record<string, string|null>} */
  errors;

  /** @type {Record<string, boolean>} */
  touched;

  /** @type {Record<string, boolean>} */
  dirty;

  /** @type {Record<string, Array<Function>>} */
  rules;

  /** @type {Record<string, Array<{endpoint: string, debounce: number, message: string}>>} */
  asyncRules;

  /** @type {Record<string, string>} */
  formats;

  /** @type {Function|null} */
  onSubmit;

  /** @type {Array<{trigger: string, handler: Function}>} */
  effects;

  /** @type {Set<Function>} */
  #subscribers = new Set();

  /** @type {Record<string, any>} */
  #initialFields;

  /**
   * Create a new Form instance
   * @param {string} id - Form ID (matches the form element's id attribute)
   * @param {Object} config - Form configuration
   */
  constructor(id, config = {}) {
    this.id = id;
    this.el = $(`#${id}`) || $(`form[name="${id}"]`);

    // Initialize state
    this.signals = config.signals || {};
    this.fields = config.fields || {};
    this.#initialFields = { ...this.fields };
    this.errors = {};
    this.touched = {};
    this.dirty = {};
    this.rules = config.rules || {};
    this.asyncRules = config.asyncRules || {};
    this.formats = config.formats || {};
    this.onSubmit = config.onSubmit || null;
    this.effects = config.effects || [];

    // Initialize error/touched/dirty state for each field
    for (const name of Object.keys(this.fields)) {
      this.errors[name] = null;
      this.touched[name] = false;
      this.dirty[name] = false;
    }

    // Bind to DOM
    this.#bindFields();
    this.#setupSubmit();
    this.#setupEffects();

    // Run mount effects
    this.#runEffects('mount');
  }

  // ===========================================================================
  // Field Binding
  // ===========================================================================

  #bindFields() {
    if (!this.el) return;

    for (const name of Object.keys(this.fields)) {
      const input = $(`[name="${name}"]`, this.el);
      if (!input) continue;

      // Set initial value
      if (input.type === 'checkbox') {
        input.checked = !!this.fields[name];
      } else if (input.type === 'radio') {
        const radios = $$(`[name="${name}"]`, this.el);
        radios.forEach(r => {
          r.checked = r.value === this.fields[name];
        });
      } else {
        input.value = this.fields[name];
      }

      // Input event - update value, validate if touched
      input.addEventListener('input', (e) => {
        // Apply input formatting if configured
        const formatType = this.formats[name];
        if (formatType) {
          const formatter = getFormatter(formatType);
          if (formatter) {
            const pos = e.target.selectionStart;
            const oldLen = e.target.value.length;
            e.target.value = formatter(e.target.value);
            const newLen = e.target.value.length;
            // Adjust cursor position after formatting
            const newPos = Math.max(0, pos + (newLen - oldLen));
            e.target.setSelectionRange(newPos, newPos);
          }
        }
        this.#updateFieldValue(name, e.target);
        if (this.touched[name]) {
          this.validate(name);
          // Trigger debounced async validation on input if field has async rules
          this.#triggerAsyncValidation(name, input);
        }
        this.#runEffects('input', name);
      });

      // Change event - for select, checkbox, radio
      input.addEventListener('change', (e) => {
        this.#updateFieldValue(name, e.target);
        this.#runEffects('change', name);
      });

      // Blur event - mark touched, validate
      input.addEventListener('blur', () => {
        this.touched[name] = true;
        this.validate(name);
        // Trigger debounced async validation on blur
        this.#triggerAsyncValidation(name, input);
        this.#runEffects('blur', name);
      });

      // Focus event
      input.addEventListener('focus', () => {
        this.#runEffects('focus', name);
      });
    }
  }

  #updateFieldValue(name, input) {
    let value;
    if (input.type === 'checkbox') {
      value = input.checked;
    } else if (input.type === 'number') {
      value = input.valueAsNumber;
    } else {
      value = input.value;
    }

    this.fields[name] = value;
    this.dirty[name] = value !== this.#initialFields[name];
    this.#notify();
  }

  // ===========================================================================
  // Validation
  // ===========================================================================

  /**
   * Validate a single field
   * @param {string} name - Field name
   * @returns {boolean} - Whether the field is valid
   */
  validate(name) {
    const value = this.fields[name];
    const rules = this.rules[name];

    if (!rules || rules.length === 0) {
      this.errors[name] = null;
      this.#updateErrorDisplay(name);
      return true;
    }

    for (const rule of rules) {
      const result = rule(value, this.fields, this.signals);
      if (result !== true) {
        this.errors[name] = result;
        this.#updateErrorDisplay(name);
        this.#notify();
        return false;
      }
    }

    this.errors[name] = null;
    this.#updateErrorDisplay(name);
    this.#notify();
    return true;
  }

  /**
   * Validate a field asynchronously
   * @param {string} name - Field name
   * @returns {Promise<boolean>}
   */
  async validateAsync(name) {
    // First run sync validation
    if (!this.validate(name)) return false;

    const asyncRules = this.asyncRules[name];
    if (!asyncRules || asyncRules.length === 0) return true;

    for (const rule of asyncRules) {
      try {
        const url = new URL(rule.endpoint, window.location.origin);
        url.searchParams.set('value', this.fields[name]);

        const res = await fetch(url.toString());
        const data = await res.json();

        if (!data.valid) {
          // Use server message if provided, otherwise fall back to static message
          this.errors[name] = data.message || rule.message;
          this.#updateErrorDisplay(name);
          this.#notify();
          return false;
        }
      } catch (e) {
        console.error('Async validation failed:', e);
        // Don't fail on network error, let server-side catch it
      }
    }

    return true;
  }

  /**
   * Trigger async validation with debounce for a field
   * @param {string} name - Field name
   * @param {HTMLElement} input - Input element
   */
  #triggerAsyncValidation(name, input) {
    const asyncRules = this.asyncRules[name];
    if (!asyncRules || asyncRules.length === 0) return;

    // Only trigger if sync validation passed
    if (this.errors[name]) return;

    // Use the debounce from the first async rule (they should all have the same debounce)
    const debounceMs = asyncRules[0].debounce || 300;
    const debounceKey = `async-${name}`;

    // Set validating state on the input's container
    const container = input.closest('div');
    if (container) {
      container.setAttribute('data-validating', 'true');
    }

    debounce(debounceKey, debounceMs, async () => {
      await this.validateAsync(name);

      // Clear validating state
      if (container) {
        container.removeAttribute('data-validating');
      }
    });
  }

  /**
   * Validate all fields
   * @returns {boolean}
   */
  validateAll() {
    let valid = true;
    for (const name of Object.keys(this.fields)) {
      this.touched[name] = true;
      if (!this.validate(name)) {
        valid = false;
      }
    }
    return valid;
  }

  /**
   * Check if the form is valid (all errors are null)
   * @returns {boolean}
   */
  isValid() {
    return Object.values(this.errors).every(e => e === null);
  }

  /**
   * Check if any field is dirty
   * @returns {boolean}
   */
  isDirty() {
    return Object.values(this.dirty).some(d => d);
  }

  #updateErrorDisplay(name) {
    if (!this.el) return;

    const errorEl = $(`[data-error="${name}"]`, this.el);
    if (errorEl) {
      errorEl.textContent = this.errors[name] || '';
      errorEl.hidden = !this.errors[name];
    }

    const input = $(`[name="${name}"]`, this.el);
    if (input) {
      input.setAttribute('aria-invalid', this.errors[name] ? 'true' : 'false');
    }
  }

  // ===========================================================================
  // Submit Handling
  // ===========================================================================

  #setupSubmit() {
    if (!this.el) return;

    this.el.addEventListener('submit', async (e) => {
      e.preventDefault();

      // Validate all fields
      if (!this.validateAll()) {
        // Focus first error field
        const firstError = Object.keys(this.errors).find(k => this.errors[k]);
        if (firstError) {
          $(`[name="${firstError}"]`, this.el)?.focus();
        }
        return;
      }

      // Run submit handler if provided
      if (this.onSubmit) {
        await this.onSubmit(this, {
          set: (key, value) => this.setSignal(key, value),
          setField: (key, value) => this.setField(key, value),
          setError: (key, error) => this.setError(key, error),
          redirect: (url) => { window.location.href = url; },
          toast: toast,
          fetch: this.#fetch.bind(this),
        });
      }
    });
  }

  async #fetch(url, options = {}) {
    const res = await fetch(url, {
      method: options.method || 'POST',
      headers: {
        'Content-Type': 'application/json',
        ...options.headers,
      },
      body: options.body ? JSON.stringify(options.body) : JSON.stringify(this.fields),
      credentials: options.credentials || 'same-origin',
    });

    const contentType = res.headers.get('content-type');
    const data = contentType?.includes('application/json') ? await res.json() : await res.text();

    // Handle validation errors (400 with errors object)
    if (res.status === 400 && data?.errors) {
      this.setErrors(data.errors);
      const firstError = Object.keys(data.errors)[0];
      if (firstError) {
        $(`[name="${firstError}"]`, this.el)?.focus();
      }
      throw new ValidationError('Validation failed', data.errors);
    }

    if (!res.ok) {
      throw new Error(data?.message || res.statusText);
    }

    return data;
  }

  /**
   * Set multiple field errors at once
   * @param {Record<string, string>} errors
   */
  setErrors(errors) {
    for (const [name, error] of Object.entries(errors)) {
      this.errors[name] = error;
      this.touched[name] = true;
      this.#updateErrorDisplay(name);
    }
    this.#notify();
  }

  // ===========================================================================
  // Effects
  // ===========================================================================

  #setupEffects() {
    for (const effect of this.effects) {
      if (effect.trigger === 'click') {
        $(effect.selector)?.addEventListener('click', (e) => {
          effect.handler(this, e);
        });
      } else if (effect.trigger === 'interval') {
        setInterval(() => effect.handler(this), effect.ms);
      }
      // Other triggers are handled in #runEffects
    }
  }

  #runEffects(trigger, fieldName = null) {
    for (const effect of this.effects) {
      if (effect.trigger === trigger) {
        if (fieldName && effect.field && effect.field !== fieldName) {
          continue;
        }
        effect.handler(this);
      }
    }
  }

  // ===========================================================================
  // State Management
  // ===========================================================================

  /**
   * Set a signal value
   * @param {string} key
   * @param {any} value
   */
  setSignal(key, value) {
    this.signals[key] = value;
    this.#notify();
  }

  /**
   * Get a signal value
   * @param {string} key
   * @returns {any}
   */
  getSignal(key) {
    return this.signals[key];
  }

  /**
   * Set a field value programmatically
   * @param {string} name
   * @param {any} value
   */
  setField(name, value) {
    this.fields[name] = value;
    this.dirty[name] = value !== this.#initialFields[name];

    // Update DOM
    if (this.el) {
      const input = $(`[name="${name}"]`, this.el);
      if (input) {
        if (input.type === 'checkbox') {
          input.checked = !!value;
        } else {
          input.value = value;
        }
      }
    }

    this.#notify();
  }

  /**
   * Set a field error programmatically
   * @param {string} name
   * @param {string|null} error
   */
  setError(name, error) {
    this.errors[name] = error;
    this.#updateErrorDisplay(name);
    this.#notify();
  }

  /**
   * Reset form to initial values
   */
  reset() {
    for (const name of Object.keys(this.fields)) {
      this.fields[name] = this.#initialFields[name];
      this.errors[name] = null;
      this.touched[name] = false;
      this.dirty[name] = false;

      if (this.el) {
        const input = $(`[name="${name}"]`, this.el);
        if (input) {
          input.value = this.fields[name];
          input.setAttribute('aria-invalid', 'false');
        }
        const errorEl = $(`[data-error="${name}"]`, this.el);
        if (errorEl) {
          errorEl.textContent = '';
          errorEl.hidden = true;
        }
      }
    }
    this.#notify();
  }

  /**
   * Get form data as a plain object
   * @returns {Record<string, any>}
   */
  getData() {
    return { ...this.fields };
  }

  // ===========================================================================
  // Subscriptions (for reactive UI updates)
  // ===========================================================================

  /**
   * Subscribe to form state changes
   * @param {Function} callback
   * @returns {Function} Unsubscribe function
   */
  subscribe(callback) {
    this.#subscribers.add(callback);
    return () => this.#subscribers.delete(callback);
  }

  #notify() {
    for (const callback of this.#subscribers) {
      callback(this);
    }
  }

  // ===========================================================================
  // Cleanup
  // ===========================================================================

  /**
   * Clean up event listeners and subscriptions
   */
  destroy() {
    this.#subscribers.clear();
    this.#runEffects('unmount');
  }
}

// =============================================================================
// Reactive Bindings
// =============================================================================

/**
 * ReactiveBindings class - manages data-bind-* attributes for progressive enhancement
 *
 * Elements with data-bind-* attributes will be automatically updated when form state changes.
 * The attribute value is a JavaScript expression that will be evaluated in the context of:
 * - formState: { isValid, isSubmitting, isDirty, isTouched, submitCount }
 * - fields: { [fieldName]: { value, error, hasError, touched, dirty } }
 */
export class ReactiveBindings {
  /** @type {Form} */
  #form;

  /** @type {Map<Element, Map<string, string>>} */
  #bindings = new Map();

  /**
   * Create reactive bindings for a form
   * @param {Form} form
   */
  constructor(form) {
    this.#form = form;
    this.#discoverBindings();
    this.#setupSubscription();
    // Initial update
    this.update();
  }

  /**
   * Discover all data-bind-* attributes in the form
   */
  #discoverBindings() {
    if (!this.#form.el) return;

    // Find all elements with data-bind-* attributes
    const elements = this.#form.el.querySelectorAll('[data-bind-disabled], [data-bind-loading], [data-bind-aria-invalid], [data-bind-class], [data-bind-hidden], [data-bind-text]');

    for (const el of elements) {
      const bindings = new Map();

      // Collect all data-bind-* attributes
      for (const attr of el.attributes) {
        if (attr.name.startsWith('data-bind-')) {
          const bindingName = attr.name.slice('data-bind-'.length);
          bindings.set(bindingName, attr.value);
        }
      }

      if (bindings.size > 0) {
        this.#bindings.set(el, bindings);
      }
    }
  }

  /**
   * Subscribe to form changes and update bindings
   */
  #setupSubscription() {
    this.#form.subscribe(() => this.update());
  }

  /**
   * Update all bindings based on current form state
   */
  update() {
    const context = this.#buildContext();

    for (const [el, bindings] of this.#bindings) {
      for (const [bindingName, expr] of bindings) {
        try {
          const value = this.#evaluate(expr, context);
          this.#applyBinding(el, bindingName, value);
        } catch (e) {
          console.warn(`Failed to evaluate binding "${bindingName}": ${expr}`, e);
        }
      }
    }
  }

  /**
   * Build the context object for binding evaluation
   */
  #buildContext() {
    const fields = {};

    for (const name of Object.keys(this.#form.fields)) {
      fields[name] = {
        value: this.#form.fields[name],
        error: this.#form.errors[name],
        hasError: !!this.#form.errors[name],
        touched: this.#form.touched[name],
        dirty: this.#form.dirty[name],
      };
    }

    return {
      formState: {
        isValid: this.#form.isValid(),
        isSubmitting: this.#form.signals.isSubmitting || false,
        isDirty: this.#form.isDirty(),
        isTouched: Object.values(this.#form.touched).some(t => t),
        submitCount: this.#form.signals.submitCount || 0,
      },
      fields,
    };
  }

  /**
   * Safely evaluate a binding expression
   * @param {string} expr - JavaScript expression
   * @param {Object} context - Context object
   * @returns {any}
   */
  #evaluate(expr, context) {
    const fn = new Function('formState', 'fields', `return (${expr})`);
    return fn(context.formState, context.fields);
  }

  /**
   * Apply a binding value to an element
   * @param {Element} el
   * @param {string} bindingName
   * @param {any} value
   */
  #applyBinding(el, bindingName, value) {
    switch (bindingName) {
      case 'disabled':
        if (value) {
          el.setAttribute('disabled', '');
        } else {
          el.removeAttribute('disabled');
        }
        break;

      case 'loading':
        if (value) {
          el.setAttribute('disabled', '');
          el.setAttribute('data-loading', 'true');
        } else {
          el.removeAttribute('data-loading');
          const disabledBinding = this.#bindings.get(el)?.get('disabled');
          if (!disabledBinding) {
            el.removeAttribute('disabled');
          }
        }
        break;

      case 'aria-invalid':
        el.setAttribute('aria-invalid', value ? 'true' : 'false');
        break;

      case 'class':
        if (typeof value === 'string') {
          el.className = el.getAttribute('data-base-class') || '';
          if (value) {
            el.classList.add(...value.split(' ').filter(c => c));
          }
        }
        break;

      case 'hidden':
        el.hidden = !!value;
        break;

      case 'text':
        el.textContent = value ?? '';
        break;

      default:
        if (typeof value === 'boolean') {
          if (value) {
            el.setAttribute(bindingName, '');
          } else {
            el.removeAttribute(bindingName);
          }
        } else {
          el.setAttribute(bindingName, value);
        }
    }
  }
}

// =============================================================================
// Auto-initialization
// =============================================================================

// Forms registered before DOMContentLoaded
const pendingForms = [];

/**
 * Register a form for initialization
 * @param {string} id
 * @param {Object} config
 * @returns {Form|Promise<Form>}
 */
export function createForm(id, config) {
  if (document.readyState === 'loading') {
    return new Promise(resolve => {
      pendingForms.push({ id, config, resolve });
    });
  }
  const form = new Form(id, config);
  // Automatically set up reactive bindings
  new ReactiveBindings(form);
  return form;
}

// Initialize pending forms when DOM is ready
if (document.readyState === 'loading') {
  document.addEventListener('DOMContentLoaded', () => {
    for (const { id, config, resolve } of pendingForms) {
      const form = new Form(id, config);
      new ReactiveBindings(form);
      resolve(form);
    }
    pendingForms.length = 0;
  });
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_generation() {
        let runtime = generate_runtime();
        assert!(runtime.contains("export class Form"));
        assert!(runtime.contains("export class ReactiveBindings"));
        assert!(runtime.contains("export function debounce"));
        assert!(runtime.contains("export function toast"));
        assert!(runtime.contains("validate(name)"));
    }

    #[test]
    fn test_runtime_url() {
        let url = runtime_url(None);
        assert!(url.contains("cdn.proa.so"));
        assert!(url.contains(RUNTIME_VERSION));

        let custom_url = runtime_url(Some("https://mycdn.com"));
        assert!(custom_url.starts_with("https://mycdn.com"));
    }

    #[test]
    fn test_minify() {
        let minified = generate_runtime_minified();
        // Minified should be smaller
        assert!(minified.len() < generate_runtime().len());
        // But still contain key exports
        assert!(minified.contains("export class Form"));
    }
}
