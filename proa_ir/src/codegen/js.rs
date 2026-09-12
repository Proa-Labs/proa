// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! JavaScript code generation
//!
//! Compiles FormIR to minimal, optimized JavaScript.

use crate::expr::Expr;
use crate::form::FormIR;
use crate::stmt::{Block, LogLevel, ScrollBehavior, Stmt};
use crate::trigger::{Effect, KeyFilter, Trigger};
use crate::types::ToastLevel;
use crate::validate::ValidationRule;
use crate::value::Value;

/// JavaScript code generator
pub struct JsCodegen {
    output: String,
    indent: usize,
    minify: bool,
}

impl JsCodegen {
    pub fn new() -> Self {
        Self {
            output: String::new(),
            indent: 0,
            minify: false,
        }
    }

    pub fn minified() -> Self {
        Self {
            output: String::new(),
            indent: 0,
            minify: true,
        }
    }

    /// Generate JavaScript from a FormIR
    pub fn generate(mut self, form: &FormIR) -> String {
        self.emit_form(form);
        self.output
    }

    fn emit_form(&mut self, form: &FormIR) {
        // Wrap in IIFE
        self.write("(function(){");
        self.newline();
        self.indent += 1;

        // Form ID
        self.write_indent();
        self.write(&format!("const $id = \"{}\";", escape_string(form.id)));
        self.newline();

        // Signals state object
        if !form.signals.is_empty() {
            self.write_indent();
            self.write("const $s = {");
            for (i, signal) in form.signals.iter().enumerate() {
                if i > 0 {
                    self.write(",");
                }
                self.write(&format!("{}: ", signal.id.0));
                self.emit_value(&signal.initial);
            }
            self.write("};");
            self.newline();
        }

        // Fields state object
        if !form.fields.is_empty() {
            // Field values
            self.write_indent();
            self.write("const $f = {");
            for (i, field) in form.fields.iter().enumerate() {
                if i > 0 {
                    self.write(",");
                }
                self.write(&format!("{}: ", field.id.0));
                if let Some(ref initial) = field.initial {
                    self.emit_value(initial);
                } else {
                    self.write("\"\"");
                }
            }
            self.write("};");
            self.newline();

            // Field errors
            self.write_indent();
            self.write("const $e = {");
            for (i, field) in form.fields.iter().enumerate() {
                if i > 0 {
                    self.write(",");
                }
                self.write(&format!("{}: null", field.id.0));
            }
            self.write("};");
            self.newline();

            // Field touched state
            self.write_indent();
            self.write("const $t = {");
            for (i, field) in form.fields.iter().enumerate() {
                if i > 0 {
                    self.write(",");
                }
                self.write(&format!("{}: false", field.id.0));
            }
            self.write("};");
            self.newline();
        }

        // Validation functions
        if form.fields.iter().any(|f| !f.rules.is_empty()) {
            self.newline();
            self.emit_validators(form);
        }

        // Helper functions
        self.newline();
        self.emit_helpers();

        // Element references
        self.newline();
        self.emit_element_refs(form);

        // Effects / Event handlers
        for effect in &form.effects {
            self.newline();
            self.emit_effect(effect);
        }

        // Field event handlers (input, blur, change)
        for field in &form.fields {
            self.newline();
            self.emit_field_handlers(field);
        }

        // Form submit handler
        if form.options.on_submit.is_some() || form.options.action.is_some() {
            self.newline();
            self.emit_submit_handler(form);
        }

        self.indent -= 1;
        self.write_indent();
        self.write("})();");
    }

    fn emit_validators(&mut self, form: &FormIR) {
        self.write_indent();
        self.write("const $v = {");
        self.newline();
        self.indent += 1;

        for (i, field) in form.fields.iter().enumerate() {
            if field.rules.is_empty() {
                continue;
            }

            self.write_indent();
            self.write(&format!("{}: [", field.id.0));
            self.newline();
            self.indent += 1;

            for (j, rule) in field.rules.iter().enumerate() {
                self.write_indent();
                self.emit_validation_rule(rule);
                if j < field.rules.len() - 1 {
                    self.write(",");
                }
                self.newline();
            }

            self.indent -= 1;
            self.write_indent();
            self.write("]");
            if i < form.fields.len() - 1 {
                self.write(",");
            }
            self.newline();
        }

        self.indent -= 1;
        self.write_indent();
        self.write("};");
        self.newline();
    }

    fn emit_validation_rule(&mut self, rule: &ValidationRule) {
        match rule {
            ValidationRule::Required { message } => {
                self.write(&format!(
                    "(v) => v.trim().length > 0 || \"{}\"",
                    escape_string(message)
                ));
            }
            ValidationRule::MinLength { min, message } => {
                self.write(&format!(
                    "(v) => v.length >= {} || \"{}\"",
                    min,
                    escape_string(message)
                ));
            }
            ValidationRule::MaxLength { max, message } => {
                self.write(&format!(
                    "(v) => v.length <= {} || \"{}\"",
                    max,
                    escape_string(message)
                ));
            }
            ValidationRule::Email { message } => {
                // Skip validation if empty (let required handle empty case)
                self.write(&format!(
                    "(v) => !v || /^[^\\s@]+@[^\\s@]+\\.[^\\s@]+$/.test(v) || \"{}\"",
                    escape_string(message)
                ));
            }
            ValidationRule::Pattern { pattern, message } => {
                // Skip validation if empty (let required handle empty case).
                // Use new RegExp(JSON-escaped-string) to avoid JS injection via
                // unescaped `/` or other regex delimiter characters.
                self.write(&format!(
                    "(v) => !v || new RegExp(\"{}\", \"u\").test(v) || \"{}\"",
                    escape_string(pattern),
                    escape_string(message)
                ));
            }
            ValidationRule::Matches { other, message } => {
                self.write(&format!(
                    "(v, f) => v === f.{} || \"{}\"",
                    other.0,
                    escape_string(message)
                ));
            }
            ValidationRule::Min { min, message } => {
                self.write(&format!(
                    "(v) => parseFloat(v) >= {} || \"{}\"",
                    min,
                    escape_string(message)
                ));
            }
            ValidationRule::Max { max, message } => {
                self.write(&format!(
                    "(v) => parseFloat(v) <= {} || \"{}\"",
                    max,
                    escape_string(message)
                ));
            }
            ValidationRule::Async {
                endpoint,
                debounce_ms,
                message,
                ..
            } => {
                self.write(&format!(
                    "{{async: true, endpoint: \"{}\", debounce: {}, message: \"{}\"}}",
                    endpoint,
                    debounce_ms,
                    escape_string(message)
                ));
            }
            ValidationRule::Custom { validate, message } => {
                self.write("(v, f) => (");
                self.emit_expr(validate);
                self.write(&format!(") || \"{}\"", escape_string(message)));
            }
            // For composite rules, emit as array
            ValidationRule::All { rules } => {
                self.write("[");
                for (i, rule) in rules.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.emit_validation_rule(rule);
                }
                self.write("]");
            }
            _ => {
                // Default fallback for unimplemented rules
                self.write("() => true");
            }
        }
    }

    fn emit_helpers(&mut self) {
        // Query selector helper
        self.write_indent();
        self.write("const $ = (s) => document.querySelector(s);");
        self.newline();

        // Query all helper
        self.write_indent();
        self.write("const $$ = (s) => document.querySelectorAll(s);");
        self.newline();

        // Validate field helper
        self.write_indent();
        self.write("const validate = (name) => {");
        self.newline();
        self.indent += 1;
        self.write_indent();
        self.write("const rules = $v[name];");
        self.newline();
        self.write_indent();
        self.write("if (!rules) return true;");
        self.newline();
        self.write_indent();
        self.write("const val = $f[name];");
        self.newline();
        self.write_indent();
        self.write("for (const rule of rules) {");
        self.newline();
        self.indent += 1;
        self.write_indent();
        self.write("if (typeof rule === 'function') {");
        self.newline();
        self.indent += 1;
        self.write_indent();
        self.write("const result = rule(val, $f);");
        self.newline();
        self.write_indent();
        self.write("if (result !== true) { $e[name] = result; updateError(name); return false; }");
        self.newline();
        self.indent -= 1;
        self.write_indent();
        self.write("}");
        self.newline();
        self.indent -= 1;
        self.write_indent();
        self.write("}");
        self.newline();
        self.write_indent();
        self.write("$e[name] = null; updateError(name); return true;");
        self.newline();
        self.indent -= 1;
        self.write_indent();
        self.write("};");
        self.newline();

        // Update error display helper
        self.write_indent();
        self.write("const updateError = (name) => {");
        self.newline();
        self.indent += 1;
        self.write_indent();
        self.write("const el = $(`[data-error=\"${name}\"]`);");
        self.newline();
        self.write_indent();
        self.write("if (el) { el.textContent = $e[name] || ''; el.hidden = !$e[name]; }");
        self.newline();
        self.write_indent();
        self.write("const input = $(`[name=\"${name}\"]`);");
        self.newline();
        self.write_indent();
        self.write("if (input) input.setAttribute('aria-invalid', $e[name] ? 'true' : 'false');");
        self.newline();
        self.indent -= 1;
        self.write_indent();
        self.write("};");
        self.newline();

        // Form valid helper
        self.write_indent();
        self.write("const isFormValid = () => Object.values($e).every(e => e === null);");
        self.newline();

        // Debounce helper
        self.write_indent();
        self.write("const debounceTimers = {};");
        self.newline();
        self.write_indent();
        self.write("const debounce = (key, ms, fn) => {");
        self.newline();
        self.indent += 1;
        self.write_indent();
        self.write("clearTimeout(debounceTimers[key]);");
        self.newline();
        self.write_indent();
        self.write("debounceTimers[key] = setTimeout(fn, ms);");
        self.newline();
        self.indent -= 1;
        self.write_indent();
        self.write("};");
        self.newline();
    }

    fn emit_element_refs(&mut self, form: &FormIR) {
        self.write_indent();
        self.write(&format!(
            "const $form = $('form#{}') || $('form');",
            escape_string(form.id)
        ));
        self.newline();
    }

    fn emit_effect(&mut self, effect: &Effect) {
        match &effect.trigger {
            Trigger::Mount => {
                self.write_indent();
                self.write("document.addEventListener('DOMContentLoaded', () => {");
                self.newline();
                self.indent += 1;
                self.emit_block(&effect.body);
                self.indent -= 1;
                self.write_indent();
                self.write("});");
                self.newline();
            }
            Trigger::Click { selector } => {
                self.write_indent();
                self.write(&format!(
                    "$('{}')?.addEventListener('click', (e) => {{",
                    selector.0
                ));
                self.newline();
                self.indent += 1;
                self.emit_block(&effect.body);
                self.indent -= 1;
                self.write_indent();
                self.write("});");
                self.newline();
            }
            Trigger::SignalChange { signal } => {
                // For reactive signals, we'd need a more complex system
                // For now, emit a comment
                self.write_indent();
                self.write(&format!(
                    "// TODO: Signal change watcher for '{}'",
                    signal.0
                ));
                self.newline();
            }
            Trigger::Submit => {
                self.write_indent();
                self.write("$form?.addEventListener('submit', async (e) => {");
                self.newline();
                self.indent += 1;
                self.write_indent();
                self.write("e.preventDefault();");
                self.newline();
                self.emit_block(&effect.body);
                self.indent -= 1;
                self.write_indent();
                self.write("});");
                self.newline();
            }
            Trigger::KeyDown { selector, key } => {
                self.write_indent();
                self.write(&format!(
                    "$('{}')?.addEventListener('keydown', (e) => {{",
                    selector.0
                ));
                self.newline();
                self.indent += 1;
                if let Some(filter) = key {
                    self.write_indent();
                    self.emit_key_filter(filter);
                    self.newline();
                }
                self.emit_block(&effect.body);
                self.indent -= 1;
                self.write_indent();
                self.write("});");
                self.newline();
            }
            Trigger::Interval { ms } => {
                self.write_indent();
                self.write("setInterval(() => {");
                self.newline();
                self.indent += 1;
                self.emit_block(&effect.body);
                self.indent -= 1;
                self.write_indent();
                self.write(&format!("}}, {});", ms));
                self.newline();
            }
            _ => {
                self.write_indent();
                self.write(&format!("// TODO: Trigger {:?}", effect.trigger));
                self.newline();
            }
        }
    }

    fn emit_key_filter(&mut self, filter: &KeyFilter) {
        let mut conditions = vec![format!("e.key === '{}'", filter.key)];
        if filter.ctrl {
            conditions.push("e.ctrlKey".to_string());
        }
        if filter.shift {
            conditions.push("e.shiftKey".to_string());
        }
        if filter.alt {
            conditions.push("e.altKey".to_string());
        }
        if filter.meta {
            conditions.push("e.metaKey".to_string());
        }
        self.write(&format!("if (!({}) return;", conditions.join(" && ")));
    }

    fn emit_field_handlers(&mut self, field: &crate::form::FieldDef) {
        let name = &field.id.0;

        // Input handler - update value
        self.write_indent();
        self.write(&format!(
            "$('[name=\"{}\"]')?.addEventListener('input', (e) => {{",
            name
        ));
        self.newline();
        self.indent += 1;
        self.write_indent();
        self.write(&format!("$f.{} = e.target.value;", name));
        self.newline();
        // Validate on input if touched
        self.write_indent();
        self.write(&format!("if ($t.{}) validate('{}');", name, name));
        self.newline();
        self.indent -= 1;
        self.write_indent();
        self.write("});");
        self.newline();

        // Blur handler - mark touched and validate
        self.write_indent();
        self.write(&format!(
            "$('[name=\"{}\"]')?.addEventListener('blur', () => {{",
            name
        ));
        self.newline();
        self.indent += 1;
        self.write_indent();
        self.write(&format!("$t.{} = true;", name));
        self.newline();
        self.write_indent();
        self.write(&format!("validate('{}');", name));
        self.newline();
        self.indent -= 1;
        self.write_indent();
        self.write("});");
        self.newline();
    }

    fn emit_submit_handler(&mut self, form: &FormIR) {
        self.write_indent();
        self.write("$form?.addEventListener('submit', async (e) => {");
        self.newline();
        self.indent += 1;

        self.write_indent();
        self.write("e.preventDefault();");
        self.newline();

        // Validate all fields
        self.write_indent();
        self.write("let valid = true;");
        self.newline();
        for field in &form.fields {
            self.write_indent();
            self.write(&format!("$t.{} = true;", field.id.0));
            self.newline();
            self.write_indent();
            self.write(&format!("if (!validate('{}')) valid = false;", field.id.0));
            self.newline();
        }

        self.write_indent();
        self.write("if (!valid) return;");
        self.newline();

        // Execute on_submit body if provided
        if let Some(ref body) = form.options.on_submit {
            self.emit_block(body);
        }

        self.indent -= 1;
        self.write_indent();
        self.write("});");
        self.newline();
    }

    fn emit_block(&mut self, block: &Block) {
        for stmt in block {
            self.emit_stmt(stmt);
        }
    }

    fn emit_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { var, value } => {
                self.write_indent();
                self.write(&format!("const {} = ", var.0));
                self.emit_expr(value);
                self.write(";");
                self.newline();
            }
            Stmt::SetSignal { id, value } => {
                self.write_indent();
                self.write(&format!("$s.{} = ", id.0));
                self.emit_expr(value);
                self.write(";");
                self.newline();
            }
            Stmt::SetField { id, value } => {
                self.write_indent();
                self.write(&format!("$f.{} = ", id.0));
                self.emit_expr(value);
                self.write(";");
                self.newline();
                // Also update DOM
                self.write_indent();
                self.write(&format!(
                    "const _el = $('[name=\"{}\"]'); if (_el) _el.value = $f.{};",
                    id.0, id.0
                ));
                self.newline();
            }
            Stmt::SetFieldError { id, error } => {
                self.write_indent();
                self.write(&format!("$e.{} = ", id.0));
                self.emit_expr(error);
                self.write(";");
                self.newline();
                self.write_indent();
                self.write(&format!("updateError('{}');", id.0));
                self.newline();
            }
            Stmt::If { cond, then, else_ } => {
                self.write_indent();
                self.write("if (");
                self.emit_expr(cond);
                self.write(") {");
                self.newline();
                self.indent += 1;
                self.emit_block(then);
                self.indent -= 1;
                if let Some(else_block) = else_ {
                    self.write_indent();
                    self.write("} else {");
                    self.newline();
                    self.indent += 1;
                    self.emit_block(else_block);
                    self.indent -= 1;
                }
                self.write_indent();
                self.write("}");
                self.newline();
            }
            Stmt::For {
                var,
                index_var,
                iter,
                body,
            } => {
                self.write_indent();
                if let Some(idx) = index_var {
                    self.write(&format!("for (let {} = 0; {} < (", idx.0, idx.0));
                    self.emit_expr(iter);
                    self.write(&format!(").length; {}++) {{", idx.0));
                    self.newline();
                    self.indent += 1;
                    self.write_indent();
                    self.write(&format!("const {} = (", var.0));
                    self.emit_expr(iter);
                    self.write(&format!(")[{}];", idx.0));
                    self.newline();
                } else {
                    self.write(&format!("for (const {} of ", var.0));
                    self.emit_expr(iter);
                    self.write(") {");
                    self.newline();
                    self.indent += 1;
                }
                self.emit_block(body);
                self.indent -= 1;
                self.write_indent();
                self.write("}");
                self.newline();
            }
            Stmt::Show { selector } => {
                self.write_indent();
                self.write(&format!("$('{}')?.removeAttribute('hidden');", selector.0));
                self.newline();
            }
            Stmt::Hide { selector } => {
                self.write_indent();
                self.write(&format!("$('{}')?.setAttribute('hidden', '');", selector.0));
                self.newline();
            }
            Stmt::Toggle { selector } => {
                self.write_indent();
                self.write(&format!("$('{}')?.toggleAttribute('hidden');", selector.0));
                self.newline();
            }
            Stmt::SetText { selector, value } => {
                self.write_indent();
                self.write(&format!("$('{}')?.textContent = ", selector.0));
                self.emit_expr(value);
                self.write(";");
                self.newline();
            }
            Stmt::AddClass { selector, class } => {
                self.write_indent();
                self.write(&format!("$('{}')?.classList.add(", selector.0));
                self.emit_expr(class);
                self.write(");");
                self.newline();
            }
            Stmt::RemoveClass { selector, class } => {
                self.write_indent();
                self.write(&format!("$('{}')?.classList.remove(", selector.0));
                self.emit_expr(class);
                self.write(");");
                self.newline();
            }
            Stmt::ToggleClass { selector, class } => {
                self.write_indent();
                self.write(&format!("$('{}')?.classList.toggle(", selector.0));
                self.emit_expr(class);
                self.write(");");
                self.newline();
            }
            Stmt::Focus { selector } => {
                self.write_indent();
                self.write(&format!("$('{}')?.focus();", selector.0));
                self.newline();
            }
            Stmt::Blur { selector } => {
                self.write_indent();
                self.write(&format!("$('{}')?.blur();", selector.0));
                self.newline();
            }
            Stmt::ScrollTo { selector, behavior } => {
                let behavior_str = match behavior {
                    ScrollBehavior::Smooth => "smooth",
                    ScrollBehavior::Instant => "instant",
                    ScrollBehavior::Auto => "auto",
                };
                self.write_indent();
                self.write(&format!(
                    "$('{}')?.scrollIntoView({{behavior: '{}'}});",
                    selector.0, behavior_str
                ));
                self.newline();
            }
            Stmt::Redirect { url } => {
                self.write_indent();
                self.write("window.location.href = ");
                self.emit_expr(url);
                self.write(";");
                self.newline();
            }
            Stmt::Toast {
                message,
                level,
                duration_ms,
            } => {
                let level_str = match level {
                    ToastLevel::Info => "info",
                    ToastLevel::Success => "success",
                    ToastLevel::Warning => "warning",
                    ToastLevel::Error => "error",
                };
                self.write_indent();
                self.write("window.toast?.(");
                self.emit_expr(message);
                self.write(&format!(
                    ", '{}', {})",
                    level_str,
                    duration_ms.unwrap_or(3000)
                ));
                self.write(";");
                self.newline();
            }
            Stmt::Log { value, level } => {
                let method = match level {
                    LogLevel::Log => "log",
                    LogLevel::Info => "info",
                    LogLevel::Warn => "warn",
                    LogLevel::Error => "error",
                    LogLevel::Debug => "debug",
                };
                self.write_indent();
                self.write(&format!("console.{}(", method));
                self.emit_expr(value);
                self.write(");");
                self.newline();
            }
            Stmt::Fetch {
                url,
                options,
                on_success,
                on_error,
            } => {
                self.write_indent();
                self.write("try {");
                self.newline();
                self.indent += 1;

                self.write_indent();
                self.write("const response = await fetch(");
                self.emit_expr(url);

                if options.method != crate::types::Method::Get || options.body.is_some() {
                    self.write(", {");
                    self.write(&format!("method: '{:?}'", options.method).to_uppercase());
                    if let Some(ref body) = options.body {
                        self.write(", body: JSON.stringify(");
                        self.emit_expr(body);
                        self.write(")");
                    }
                    self.write(", headers: {'Content-Type': 'application/json'}");
                    self.write("}");
                }
                self.write(");");
                self.newline();

                self.write_indent();
                self.write("if (!response.ok) throw new Error(response.statusText);");
                self.newline();

                self.write_indent();
                self.write("const $response = await response.json();");
                self.newline();

                self.emit_block(on_success);

                self.indent -= 1;
                self.write_indent();
                self.write("} catch ($error) {");
                self.newline();
                self.indent += 1;

                self.emit_block(on_error);

                self.indent -= 1;
                self.write_indent();
                self.write("}");
                self.newline();
            }
            Stmt::Debounce { key, ms, body } => {
                self.write_indent();
                self.write(&format!("debounce('{}', {}, () => {{", key, ms));
                self.newline();
                self.indent += 1;
                self.emit_block(body);
                self.indent -= 1;
                self.write_indent();
                self.write("});");
                self.newline();
            }
            Stmt::Delay { ms, body } => {
                self.write_indent();
                self.write("setTimeout(() => {");
                self.newline();
                self.indent += 1;
                self.emit_block(body);
                self.indent -= 1;
                self.write_indent();
                self.write("}, ");
                self.emit_expr(ms);
                self.write(");");
                self.newline();
            }
            Stmt::CopyToClipboard { value } => {
                self.write_indent();
                self.write("navigator.clipboard?.writeText(");
                self.emit_expr(value);
                self.write(");");
                self.newline();
            }
            Stmt::SetLocalStorage { key, value } => {
                self.write_indent();
                self.write(&format!("localStorage.setItem('{}', JSON.stringify(", key));
                self.emit_expr(value);
                self.write("));");
                self.newline();
            }
            Stmt::PreventDefault => {
                self.write_indent();
                self.write("e.preventDefault();");
                self.newline();
            }
            Stmt::StopPropagation => {
                self.write_indent();
                self.write("e.stopPropagation();");
                self.newline();
            }
            Stmt::SubmitForm => {
                self.write_indent();
                self.write("$form?.submit();");
                self.newline();
            }
            Stmt::ResetForm => {
                self.write_indent();
                self.write("$form?.reset();");
                self.newline();
            }
            _ => {
                self.write_indent();
                self.write(&format!("// TODO: {:?}", stmt));
                self.newline();
            }
        }
    }

    fn emit_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Lit { value } => self.emit_value(value),
            Expr::Signal { id } => self.write(&format!("$s.{}", id.0)),
            Expr::Field { id } => self.write(&format!("$f.{}", id.0)),
            Expr::Var { id } => self.write(&id.0),
            // Server data scope. Client runtimes that mirror server data
            // must define `$scope`; see SEMANTICS.md.
            Expr::Scope { path } => {
                self.write("$scope");
                for segment in path {
                    self.write(&format!("[\"{}\"]", escape_string(segment)));
                }
            }
            Expr::Response => self.write("$response"),
            Expr::Error => self.write("$error"),
            Expr::Event => self.write("e"),
            Expr::Target => self.write("e.target"),
            Expr::Now => self.write("Date.now()"),

            Expr::Prop { expr, prop } => {
                self.write("(");
                self.emit_expr(expr);
                self.write(&format!(").{}", prop));
            }
            Expr::Index { expr, index } => {
                self.write("(");
                self.emit_expr(expr);
                self.write(")[");
                self.emit_expr(index);
                self.write("]");
            }

            // Arithmetic
            Expr::Add { left, right } => self.emit_binary(left, "+", right),
            Expr::Sub { left, right } => self.emit_binary(left, "-", right),
            Expr::Mul { left, right } => self.emit_binary(left, "*", right),
            Expr::Div { left, right } => self.emit_binary(left, "/", right),
            Expr::Mod { left, right } => self.emit_binary(left, "%", right),
            Expr::Neg { expr } => {
                self.write("-(");
                self.emit_expr(expr);
                self.write(")");
            }

            // Comparison
            Expr::Eq { left, right } => self.emit_binary(left, "===", right),
            Expr::Ne { left, right } => self.emit_binary(left, "!==", right),
            Expr::Lt { left, right } => self.emit_binary(left, "<", right),
            Expr::Le { left, right } => self.emit_binary(left, "<=", right),
            Expr::Gt { left, right } => self.emit_binary(left, ">", right),
            Expr::Ge { left, right } => self.emit_binary(left, ">=", right),

            // Logical
            Expr::And { left, right } => self.emit_binary(left, "&&", right),
            Expr::Or { left, right } => self.emit_binary(left, "||", right),
            Expr::Not { expr } => {
                self.write("!(");
                self.emit_expr(expr);
                self.write(")");
            }

            // Conditional
            Expr::Ternary { cond, then, else_ } => {
                self.write("(");
                self.emit_expr(cond);
                self.write(" ? ");
                self.emit_expr(then);
                self.write(" : ");
                self.emit_expr(else_);
                self.write(")");
            }
            Expr::Coalesce { left, right } => self.emit_binary(left, "??", right),

            // String operations
            Expr::Len { expr } => {
                self.write("(");
                self.emit_expr(expr);
                self.write(").length");
            }
            Expr::Trim { expr } => {
                self.write("(");
                self.emit_expr(expr);
                self.write(").trim()");
            }
            Expr::Lower { expr } => {
                self.write("(");
                self.emit_expr(expr);
                self.write(").toLowerCase()");
            }
            Expr::Upper { expr } => {
                self.write("(");
                self.emit_expr(expr);
                self.write(").toUpperCase()");
            }
            Expr::Contains { haystack, needle } => {
                self.write("(");
                self.emit_expr(haystack);
                self.write(").includes(");
                self.emit_expr(needle);
                self.write(")");
            }
            Expr::StartsWith { string, prefix } => {
                self.write("(");
                self.emit_expr(string);
                self.write(").startsWith(");
                self.emit_expr(prefix);
                self.write(")");
            }
            Expr::EndsWith { string, suffix } => {
                self.write("(");
                self.emit_expr(string);
                self.write(").endsWith(");
                self.emit_expr(suffix);
                self.write(")");
            }
            Expr::Concat { parts } => {
                self.write("(");
                for (i, part) in parts.iter().enumerate() {
                    if i > 0 {
                        self.write(" + ");
                    }
                    self.emit_expr(part);
                }
                self.write(")");
            }
            Expr::Matches { string, pattern } => {
                self.write(&format!("/{}/u.test(", pattern));
                self.emit_expr(string);
                self.write(")");
            }
            Expr::Substring { string, start, end } => {
                self.write("(");
                self.emit_expr(string);
                self.write(").substring(");
                self.emit_expr(start);
                if let Some(end) = end {
                    self.write(", ");
                    self.emit_expr(end);
                }
                self.write(")");
            }
            Expr::Replace { string, from, to } => {
                self.write("(");
                self.emit_expr(string);
                self.write(").replace(");
                self.emit_expr(from);
                self.write(", ");
                self.emit_expr(to);
                self.write(")");
            }
            Expr::Split { string, delimiter } => {
                self.write("(");
                self.emit_expr(string);
                self.write(").split(");
                self.emit_expr(delimiter);
                self.write(")");
            }
            Expr::Join { array, delimiter } => {
                self.write("(");
                self.emit_expr(array);
                self.write(").join(");
                self.emit_expr(delimiter);
                self.write(")");
            }

            // Array operations
            Expr::Array { items } => {
                self.write("[");
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.emit_expr(item);
                }
                self.write("]");
            }
            Expr::Object { entries } => {
                self.write("{ ");
                for (i, (key, value)) in entries.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.write(&format!("{}: ", key));
                    self.emit_expr(value);
                }
                self.write(" }");
            }
            Expr::Map { array, var, body } => {
                self.write("(");
                self.emit_expr(array);
                self.write(&format!(").map({} => ", var.0));
                self.emit_expr(body);
                self.write(")");
            }
            Expr::Filter {
                array,
                var,
                predicate,
            } => {
                self.write("(");
                self.emit_expr(array);
                self.write(&format!(").filter({} => ", var.0));
                self.emit_expr(predicate);
                self.write(")");
            }
            Expr::Find {
                array,
                var,
                predicate,
            } => {
                self.write("(");
                self.emit_expr(array);
                self.write(&format!(").find({} => ", var.0));
                self.emit_expr(predicate);
                self.write(")");
            }
            Expr::FindIndex {
                array,
                var,
                predicate,
            } => {
                self.write("(");
                self.emit_expr(array);
                self.write(&format!(").findIndex({} => ", var.0));
                self.emit_expr(predicate);
                self.write(")");
            }
            Expr::Some {
                array,
                var,
                predicate,
            } => {
                self.write("(");
                self.emit_expr(array);
                self.write(&format!(").some({} => ", var.0));
                self.emit_expr(predicate);
                self.write(")");
            }
            Expr::Every {
                array,
                var,
                predicate,
            } => {
                self.write("(");
                self.emit_expr(array);
                self.write(&format!(").every({} => ", var.0));
                self.emit_expr(predicate);
                self.write(")");
            }
            Expr::Reduce {
                array,
                acc_var,
                item_var,
                body,
                initial,
            } => {
                self.write("(");
                self.emit_expr(array);
                self.write(&format!(").reduce(({}, {}) => ", acc_var.0, item_var.0));
                self.emit_expr(body);
                self.write(", ");
                self.emit_expr(initial);
                self.write(")");
            }
            Expr::Includes { array, value } => {
                self.write("(");
                self.emit_expr(array);
                self.write(").includes(");
                self.emit_expr(value);
                self.write(")");
            }
            Expr::Slice { array, start, end } => {
                self.write("(");
                self.emit_expr(array);
                self.write(").slice(");
                self.emit_expr(start);
                if let Some(end) = end {
                    self.write(", ");
                    self.emit_expr(end);
                }
                self.write(")");
            }
            Expr::Flat { array, depth } => {
                self.write("(");
                self.emit_expr(array);
                match depth {
                    Some(depth) => self.write(&format!(").flat({depth})")),
                    None => self.write(").flat()"),
                }
            }
            Expr::First { array } => {
                self.write("(");
                self.emit_expr(array);
                self.write(")[0]");
            }
            Expr::Last { array } => {
                self.write("(");
                self.emit_expr(array);
                self.write(").at(-1)");
            }

            // Type checking
            Expr::IsNull { expr } => {
                self.write("(");
                self.emit_expr(expr);
                self.write(" == null)");
            }
            Expr::IsNumber { expr } => {
                self.write("(typeof ");
                self.emit_expr(expr);
                self.write(" === 'number')");
            }
            Expr::IsString { expr } => {
                self.write("(typeof ");
                self.emit_expr(expr);
                self.write(" === 'string')");
            }
            Expr::IsArray { expr } => {
                self.write("Array.isArray(");
                self.emit_expr(expr);
                self.write(")");
            }

            // Conversion
            Expr::ParseInt { expr } => {
                self.write("parseInt(");
                self.emit_expr(expr);
                self.write(", 10)");
            }
            Expr::ParseFloat { expr } => {
                self.write("parseFloat(");
                self.emit_expr(expr);
                self.write(")");
            }
            Expr::ToString { expr } => {
                self.write("String(");
                self.emit_expr(expr);
                self.write(")");
            }
            Expr::ToNumber { expr } => {
                self.write("Number(");
                self.emit_expr(expr);
                self.write(")");
            }

            // Math
            Expr::Abs { expr } => {
                self.write("Math.abs(");
                self.emit_expr(expr);
                self.write(")");
            }
            Expr::Round { expr } => {
                self.write("Math.round(");
                self.emit_expr(expr);
                self.write(")");
            }
            Expr::Floor { expr } => {
                self.write("Math.floor(");
                self.emit_expr(expr);
                self.write(")");
            }
            Expr::Ceil { expr } => {
                self.write("Math.ceil(");
                self.emit_expr(expr);
                self.write(")");
            }
            Expr::Clamp { value, min, max } => {
                self.write("Math.max(");
                self.emit_expr(min);
                self.write(", Math.min(");
                self.emit_expr(max);
                self.write(", ");
                self.emit_expr(value);
                self.write("))");
            }
            Expr::Min { values } => {
                self.write("Math.min(");
                for (i, v) in values.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.emit_expr(v);
                }
                self.write(")");
            }
            Expr::Max { values } => {
                self.write("Math.max(");
                for (i, v) in values.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.emit_expr(v);
                }
                self.write(")");
            }

            // Number formatting (must agree with the server evaluator's
            // FormatNumber implementation; see SEMANTICS.md)
            Expr::FormatNumber { value, decimals } => {
                self.write("(");
                self.emit_expr(value);
                self.write(&format!(").toFixed({})", decimals));
            }

            // Form-specific
            Expr::FormValid => self.write("isFormValid()"),
            Expr::FormDirty => self.write("Object.keys($f).some(k => $f[k] !== '')"),
            Expr::FormData => self.write("{...$f}"),
            Expr::FieldHasError { id } => self.write(&format!("($e.{} !== null)", id.0)),
            Expr::FieldError { id } => self.write(&format!("$e.{}", id.0)),
            Expr::FieldTouched { id } => self.write(&format!("$t.{}", id.0)),
            Expr::FieldDirty { id } => self.write(&format!("($f.{} !== '')", id.0)),

            // Validation helpers
            Expr::IsEmail { expr } => {
                self.write("/^[^\\s@]+@[^\\s@]+\\.[^\\s@]+$/.test(");
                self.emit_expr(expr);
                self.write(")");
            }
            Expr::IsUrl { expr } => {
                self.write("(() => { try { new URL(");
                self.emit_expr(expr);
                self.write("); return true; } catch { return false; } })()");
            }

            _ => {
                self.write(&format!("/* TODO: {:?} */", expr));
            }
        }
    }

    fn emit_binary(&mut self, left: &Expr, op: &str, right: &Expr) {
        self.write("(");
        self.emit_expr(left);
        self.write(&format!(" {} ", op));
        self.emit_expr(right);
        self.write(")");
    }

    fn emit_value(&mut self, value: &Value) {
        match value {
            Value::Null => self.write("null"),
            Value::Bool(b) => self.write(if *b { "true" } else { "false" }),
            Value::Int(n) => self.write(&n.to_string()),
            Value::Float(n) => self.write(&n.to_string()),
            Value::String(s) => self.write(&format!("\"{}\"", escape_string(s))),
            Value::Array(items) => {
                self.write("[");
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.emit_value(item);
                }
                self.write("]");
            }
            Value::Object(entries) => {
                self.write("{");
                for (i, (key, val)) in entries.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.write(&format!("{}: ", key));
                    self.emit_value(val);
                }
                self.write("}");
            }
        }
    }

    fn write(&mut self, s: &str) {
        self.output.push_str(s);
    }

    fn write_indent(&mut self) {
        if !self.minify {
            for _ in 0..self.indent {
                self.output.push_str("  ");
            }
        }
    }

    fn newline(&mut self) {
        if !self.minify {
            self.output.push('\n');
        }
    }
}

impl Default for JsCodegen {
    fn default() -> Self {
        Self::new()
    }
}

/// Render a single [`Expr`] as a standalone JavaScript expression string.
///
/// The generated code references the runtime names this codegen uses for
/// client roots (`$s`, `$f`, `$scope`, `e`, ...), so it is self-contained
/// only for the server dialect (literals, `Scope`, `Var`, and pure ops).
/// Used for evaluator/emitter parity testing and tooling.
pub fn expr_to_js(expr: &Expr) -> String {
    let mut codegen = JsCodegen::minified();
    codegen.emit_expr(expr);
    codegen.output
}

/// Escape `s` for embedding inside a JavaScript string literal that may be
/// emitted inline in a `<script>` element. Beyond the usual quote/backslash/
/// control-character escapes, this masks the three byte sequences that could
/// break out of an inline script context:
///
/// * `<` -> `\x3C`. `\x3C` is valid JavaScript and decodes back to `<` for the
///   JS engine, yet the HTML tokenizer never sees the literal bytes `</script>`
///   (which would close the script early) or `<!--`. Masking `<` covers both.
/// * `U+2028` (LINE SEPARATOR) -> ` ` and `U+2029` (PARAGRAPH
///   SEPARATOR) -> ` `. Before ES2019 these were line terminators inside
///   string literals on many JS parsers, allowing string breakout.
///
/// This mirrors the canonical script-context masking in
/// `proa_react/src/island.rs` (see the "CVE review Gap M" comment block).
fn escape_string(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\'', "\\'")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
        .replace('<', "\\x3C")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::{block, form};

    #[test]
    fn test_basic_codegen() {
        let form_ir = form("signup")
            .signal("loading", false)
            .field("email", |f| {
                f.required("Email is required").email("Invalid email")
            })
            .field("password", |f| {
                f.required("Password is required")
                    .min_length(8, "At least 8 characters")
            })
            .on_submit(block().set("loading", true).build())
            .build();

        let js = JsCodegen::new().generate(&form_ir);

        // Verify key parts are present
        assert!(
            js.contains("const $id = \"signup\""),
            "Missing form id. JS:\n{}",
            js
        );
        assert!(
            js.contains("$s = {loading: false}"),
            "Missing signals. JS:\n{}",
            js
        );
        assert!(
            js.contains("$f = {email: \"\"") || js.contains("$f = {email:"),
            "Missing fields. JS:\n{}",
            js
        );
        assert!(
            js.contains("$e = {email: null") || js.contains("$e = {email:"),
            "Missing errors. JS:\n{}",
            js
        );
        assert!(
            js.contains("const validate = (name)"),
            "Missing validate fn. JS:\n{}",
            js
        );
        assert!(
            js.contains("$form?.addEventListener('submit'"),
            "Missing submit handler. JS:\n{}",
            js
        );
    }

    #[test]
    fn test_validation_rules_codegen() {
        let form_ir = form("test")
            .field("email", |f| f.email("Invalid email"))
            .field("password", |f| {
                f.min_length(8, "Too short")
                    .matches("confirm", "Must match")
            })
            .build();

        let js = JsCodegen::new().generate(&form_ir);

        assert!(js.contains("/^[^\\s@]+@[^\\s@]+\\.[^\\s@]+$/.test(v)"));
        assert!(js.contains("v.length >= 8"));
        assert!(js.contains("v === f.confirm"));
    }

    #[test]
    fn escape_string_keeps_existing_escapes() {
        assert_eq!(escape_string("a\\b"), "a\\\\b");
        assert_eq!(escape_string("a\"b"), "a\\\"b");
        assert_eq!(escape_string("a'b"), "a\\'b");
        assert_eq!(escape_string("a\nb"), "a\\nb");
        assert_eq!(escape_string("a\rb"), "a\\rb");
        assert_eq!(escape_string("a\tb"), "a\\tb");
    }

    #[test]
    fn escape_string_masks_closing_script_tag() {
        // `</script>` inside an inline <script> would close it early. `<` is
        // masked to `\x3C`, so the byte sequence `</` never reaches the HTML
        // tokenizer.
        let escaped = escape_string("</script><img src=x onerror=alert(1)>");
        assert!(
            !escaped.contains("</"),
            "unmasked `</` in escaped JS: {escaped}"
        );
        assert!(
            !escaped.contains("</script>"),
            "unmasked closing script tag"
        );
        assert!(escaped.contains("\\x3C/script>"));
    }

    #[test]
    fn escape_string_masks_html_comment_open() {
        // `<!--` opens an HTML comment that legacy parsers honor inside a
        // <script>. Masking `<` neutralizes it.
        let escaped = escape_string("before <!-- after");
        assert!(!escaped.contains("<!--"), "unmasked `<!--` in escaped JS");
        assert!(escaped.contains("\\x3C!-- after"));
    }

    #[test]
    fn escape_string_masks_line_separators() {
        // U+2028 / U+2029 break out of a string literal on legacy parsers and
        // must become their \uXXXX escapes.
        let escaped = escape_string("line\u{2028}para\u{2029}end");
        assert!(!escaped.contains('\u{2028}'), "raw U+2028 in escaped JS");
        assert!(!escaped.contains('\u{2029}'), "raw U+2029 in escaped JS");
        assert_eq!(escaped, "line\\u2028para\\u2029end");
    }

    #[test]
    fn test_scope_expr_codegen() {
        let expr = crate::Expr::scope_path(["product", "price"]);
        assert_eq!(expr_to_js(&expr), "$scope[\"product\"][\"price\"]");

        // Scope composes with Prop/Index
        let expr = crate::Expr::scope("cart").prop("items").index(0);
        assert_eq!(expr_to_js(&expr), "(($scope[\"cart\"]).items)[0]");
    }

    #[test]
    fn test_format_number_codegen() {
        let expr = crate::Expr::scope("total").div(100).format_number(2);
        assert_eq!(expr_to_js(&expr), "(($scope[\"total\"] / 100)).toFixed(2)");
    }
}
