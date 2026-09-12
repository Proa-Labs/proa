// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Per-form code generation
//!
//! Generates the tiny per-form configuration that imports from the runtime.

use crate::expr::Expr;
use crate::form::FormIR;
use crate::stmt::{Block, Stmt};
use crate::types::ToastLevel;
use crate::validate::{InputFormat, ValidationRule};
use crate::value::Value;

use super::runtime::runtime_url;

/// Configuration for form code generation
#[derive(Debug, Clone)]
pub struct FormCodegenConfig {
    /// CDN base URL for the runtime (None = use default)
    pub cdn_url: Option<String>,
    /// Whether to minify the output
    pub minify: bool,
    /// Whether to use inline script tags
    pub inline: bool,
}

impl Default for FormCodegenConfig {
    fn default() -> Self {
        Self {
            cdn_url: None,
            minify: false,
            inline: true,
        }
    }
}

/// Generate JavaScript for a form
pub struct FormCodegen {
    config: FormCodegenConfig,
    output: String,
    indent: usize,
}

impl FormCodegen {
    pub fn new(config: FormCodegenConfig) -> Self {
        Self {
            config,
            output: String::new(),
            indent: 0,
        }
    }

    /// Generate a complete script tag for the form
    pub fn generate_script(form: &FormIR, config: FormCodegenConfig) -> String {
        let mut gen = Self::new(config);
        gen.emit_script(form);
        gen.output
    }

    /// Generate just the JavaScript (no script tag, but includes import)
    pub fn generate_js(form: &FormIR, config: FormCodegenConfig) -> String {
        let mut gen = Self::new(config.clone());
        let runtime = runtime_url(config.cdn_url.as_deref());
        gen.emit_import(&runtime);
        gen.newline();
        gen.emit_form_js(form);
        gen.output
    }

    fn emit_script(&mut self, form: &FormIR) {
        let runtime = runtime_url(self.config.cdn_url.as_deref());

        self.write("<script type=\"module\">");
        self.newline();
        self.emit_import(&runtime);
        self.newline();
        self.emit_form_js(form);
        self.write("</script>");
    }

    fn emit_import(&mut self, runtime_url: &str) {
        self.write(&format!(
            "import {{ createForm, debounce, toast, detectCardType, getCardInfo }} from '{}';",
            runtime_url
        ));
    }

    fn emit_form_js(&mut self, form: &FormIR) {
        self.write(&format!("createForm('{}', {{", form.id));
        self.newline();
        self.indent += 1;

        // Signals
        if !form.signals.is_empty() {
            self.write_indent();
            self.write("signals: {");
            for (i, signal) in form.signals.iter().enumerate() {
                if i > 0 {
                    self.write(", ");
                }
                self.write(&format!("{}: ", signal.id.0));
                self.emit_value(&signal.initial);
            }
            self.write("},");
            self.newline();
        }

        // Fields
        if !form.fields.is_empty() {
            self.write_indent();
            self.write("fields: {");
            for (i, field) in form.fields.iter().enumerate() {
                if i > 0 {
                    self.write(", ");
                }
                self.write(&format!("{}: ", field.id.0));
                if let Some(ref initial) = field.initial {
                    self.emit_value(initial);
                } else {
                    self.write("''");
                }
            }
            self.write("},");
            self.newline();
        }

        // Rules (sync validation)
        let fields_with_rules: Vec<_> =
            form.fields.iter().filter(|f| !f.rules.is_empty()).collect();
        if !fields_with_rules.is_empty() {
            self.write_indent();
            self.write("rules: {");
            self.newline();
            self.indent += 1;

            for (i, field) in fields_with_rules.iter().enumerate() {
                self.write_indent();
                self.write(&format!("{}: [", field.id.0));
                self.newline();
                self.indent += 1;

                for (j, rule) in field.rules.iter().enumerate() {
                    if !matches!(rule, ValidationRule::Async { .. }) {
                        self.write_indent();
                        self.emit_validation_rule(rule);
                        if j < field.rules.len() - 1 {
                            self.write(",");
                        }
                        self.newline();
                    }
                }

                self.indent -= 1;
                self.write_indent();
                self.write("]");
                if i < fields_with_rules.len() - 1 {
                    self.write(",");
                }
                self.newline();
            }

            self.indent -= 1;
            self.write_indent();
            self.write("},");
            self.newline();
        }

        // Async rules
        let fields_with_async: Vec<_> = form
            .fields
            .iter()
            .filter(|f| {
                f.rules
                    .iter()
                    .any(|r| matches!(r, ValidationRule::Async { .. }))
            })
            .collect();
        if !fields_with_async.is_empty() {
            self.write_indent();
            self.write("asyncRules: {");
            self.newline();
            self.indent += 1;

            for (i, field) in fields_with_async.iter().enumerate() {
                self.write_indent();
                self.write(&format!("{}: [", field.id.0));

                for rule in &field.rules {
                    if let ValidationRule::Async {
                        endpoint,
                        debounce_ms,
                        message,
                        ..
                    } = rule
                    {
                        self.write(&format!(
                            "{{ endpoint: '{}', debounce: {}, message: '{}' }}",
                            endpoint,
                            debounce_ms,
                            escape_string(message)
                        ));
                    }
                }

                self.write("]");
                if i < fields_with_async.len() - 1 {
                    self.write(",");
                }
                self.newline();
            }

            self.indent -= 1;
            self.write_indent();
            self.write("},");
            self.newline();
        }

        // Input formats
        let fields_with_formats: Vec<_> =
            form.fields.iter().filter(|f| f.format.is_some()).collect();
        if !fields_with_formats.is_empty() {
            self.write_indent();
            self.write("formats: {");
            for (i, field) in fields_with_formats.iter().enumerate() {
                if i > 0 {
                    self.write(", ");
                }
                let format_name = match field.format.as_ref().unwrap() {
                    InputFormat::Phone => "phone",
                    InputFormat::CreditCard => "credit_card",
                    InputFormat::Date { .. } => "expiry", // Use expiry for dates
                    InputFormat::Ssn => "ssn",
                    InputFormat::ZipCode => "zip_code",
                    InputFormat::Cvc => "cvc",
                    InputFormat::Name => "name",
                    InputFormat::Currency { .. } => "currency",
                    InputFormat::Custom { .. } => "custom",
                };
                self.write(&format!("{}: '{}'", field.id.0, format_name));
            }
            self.write("},");
            self.newline();
        }

        // Effects
        if !form.effects.is_empty() {
            self.write_indent();
            self.write("effects: [");
            self.newline();
            self.indent += 1;

            for (i, effect) in form.effects.iter().enumerate() {
                self.write_indent();
                self.emit_effect(effect);
                if i < form.effects.len() - 1 {
                    self.write(",");
                }
                self.newline();
            }

            self.indent -= 1;
            self.write_indent();
            self.write("],");
            self.newline();
        }

        // onSubmit handler
        if let Some(ref body) = form.options.on_submit {
            self.write_indent();
            self.write(
                "onSubmit: async (form, { set, setField, setError, redirect, toast, fetch }) => {",
            );
            self.newline();
            self.indent += 1;
            self.emit_block(body);
            self.indent -= 1;
            self.write_indent();
            self.write("},");
            self.newline();
        }

        self.indent -= 1;
        self.write_indent();
        self.write("});");
        self.newline();
    }

    fn emit_effect(&mut self, effect: &crate::trigger::Effect) {
        use crate::trigger::Trigger;

        self.write("{ trigger: '");
        match &effect.trigger {
            Trigger::Mount => self.write("mount"),
            Trigger::Unmount => self.write("unmount"),
            Trigger::Input { field } => {
                self.write(&format!("input', field: '{}", field.0));
            }
            Trigger::Change { field } => {
                self.write(&format!("change', field: '{}", field.0));
            }
            Trigger::Blur { field } => {
                self.write(&format!("blur', field: '{}", field.0));
            }
            Trigger::Focus { field } => {
                self.write(&format!("focus', field: '{}", field.0));
            }
            Trigger::Click { selector } => {
                self.write(&format!("click', selector: '{}", selector.0));
            }
            Trigger::Submit => self.write("submit"),
            Trigger::Interval { ms } => {
                self.write(&format!("interval', ms: {}", ms));
            }
            _ => self.write("unknown"),
        }
        self.write("', handler: (form) => {");
        self.newline();
        self.indent += 1;
        self.emit_block(&effect.body);
        self.indent -= 1;
        self.write_indent();
        self.write("} }");
    }

    fn emit_validation_rule(&mut self, rule: &ValidationRule) {
        match rule {
            ValidationRule::Required { message } => {
                self.write(&format!(
                    "(v) => !!v?.trim() || '{}'",
                    escape_string(message)
                ));
            }
            ValidationRule::MinLength { min, message } => {
                self.write(&format!(
                    "(v) => v.length >= {} || '{}'",
                    min,
                    escape_string(message)
                ));
            }
            ValidationRule::MaxLength { max, message } => {
                self.write(&format!(
                    "(v) => v.length <= {} || '{}'",
                    max,
                    escape_string(message)
                ));
            }
            ValidationRule::Email { message } => {
                // Skip validation if empty (let required handle empty case)
                self.write(&format!(
                    "(v) => !v || /^[^\\s@]+@[^\\s@]+\\.[^\\s@]+$/.test(v) || '{}'",
                    escape_string(message)
                ));
            }
            ValidationRule::Pattern { pattern, message } => {
                // Skip validation if empty (let required handle empty case)
                self.write(&format!(
                    "(v) => !v || /{}/u.test(v) || '{}'",
                    pattern,
                    escape_string(message)
                ));
            }
            ValidationRule::Matches { other, message } => {
                self.write(&format!(
                    "(v, f) => v === f.{} || '{}'",
                    other.0,
                    escape_string(message)
                ));
            }
            ValidationRule::Min { min, message } => {
                self.write(&format!(
                    "(v) => parseFloat(v) >= {} || '{}'",
                    min,
                    escape_string(message)
                ));
            }
            ValidationRule::Max { max, message } => {
                self.write(&format!(
                    "(v) => parseFloat(v) <= {} || '{}'",
                    max,
                    escape_string(message)
                ));
            }
            ValidationRule::Custom { validate, message } => {
                self.write("(v, f, s) => (");
                self.emit_expr(validate);
                self.write(&format!(") || '{}'", escape_string(message)));
            }
            ValidationRule::All { rules } => {
                self.write("(v, f, s) => [");
                for (i, r) in rules.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.emit_validation_rule(r);
                }
                self.write("].every(r => r(v, f, s) === true) || 'Invalid'");
            }
            _ => {
                self.write("() => true");
            }
        }
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
                self.write(&format!("set('{}', ", id.0));
                self.emit_expr(value);
                self.write(");");
                self.newline();
            }
            Stmt::SetField { id, value } => {
                self.write_indent();
                self.write(&format!("setField('{}', ", id.0));
                self.emit_expr(value);
                self.write(");");
                self.newline();
            }
            Stmt::SetFieldError { id, error } => {
                self.write_indent();
                self.write(&format!("setError('{}', ", id.0));
                self.emit_expr(error);
                self.write(");");
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
            Stmt::Redirect { url } => {
                self.write_indent();
                self.write("redirect(");
                self.emit_expr(url);
                self.write(");");
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
                self.write("toast(");
                self.emit_expr(message);
                self.write(&format!(", '{}'", level_str));
                if let Some(ms) = duration_ms {
                    self.write(&format!(", {}", ms));
                }
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
                self.write("const res = await fetch(");
                self.emit_expr(url);

                if options.method != crate::types::Method::Get || options.body.is_some() {
                    self.write(", { ");
                    self.write(&format!("method: '{:?}'", options.method).to_uppercase());
                    if let Some(ref body) = options.body {
                        self.write(", body: ");
                        self.emit_expr(body);
                    }
                    self.write(" }");
                }
                self.write(");");
                self.newline();

                self.emit_block(on_success);

                self.indent -= 1;
                self.write_indent();
                self.write("} catch (err) {");
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
            Stmt::Log { value, .. } => {
                self.write_indent();
                self.write("console.log(");
                self.emit_expr(value);
                self.write(");");
                self.newline();
            }
            Stmt::ResetForm => {
                self.write_indent();
                self.write("form.reset();");
                self.newline();
            }
            Stmt::Show { selector } => {
                self.write_indent();
                self.write(&format!(
                    "document.querySelector('{}')?.removeAttribute('hidden');",
                    selector.0
                ));
                self.newline();
            }
            Stmt::Hide { selector } => {
                self.write_indent();
                self.write(&format!(
                    "document.querySelector('{}')?.setAttribute('hidden', '');",
                    selector.0
                ));
                self.newline();
            }
            Stmt::AddClass { selector, class } => {
                self.write_indent();
                self.write(&format!(
                    "document.querySelector('{}')?.classList.add(",
                    selector.0
                ));
                self.emit_expr(class);
                self.write(");");
                self.newline();
            }
            Stmt::RemoveClass { selector, class } => {
                self.write_indent();
                self.write(&format!(
                    "document.querySelector('{}')?.classList.remove(",
                    selector.0
                ));
                self.emit_expr(class);
                self.write(");");
                self.newline();
            }
            Stmt::ToggleClass { selector, class } => {
                self.write_indent();
                self.write(&format!(
                    "document.querySelector('{}')?.classList.toggle(",
                    selector.0
                ));
                self.emit_expr(class);
                self.write(");");
                self.newline();
            }
            Stmt::SetAttr {
                selector,
                attr,
                value,
            } => {
                self.write_indent();
                self.write(&format!(
                    "document.querySelector('{}')?.setAttribute('{}', ",
                    selector.0, attr
                ));
                self.emit_expr(value);
                self.write(");");
                self.newline();
            }
            Stmt::RemoveAttr { selector, attr } => {
                self.write_indent();
                self.write(&format!(
                    "document.querySelector('{}')?.removeAttribute('{}');",
                    selector.0, attr
                ));
                self.newline();
            }
            Stmt::SetText { selector, value } => {
                self.write_indent();
                self.write(&format!(
                    "{{ const el = document.querySelector('{}'); if (el) el.textContent = ",
                    selector.0
                ));
                self.emit_expr(value);
                self.write("; }");
                self.newline();
            }
            Stmt::SetHtml { selector, value } => {
                self.write_indent();
                self.write(&format!(
                    "{{ const el = document.querySelector('{}'); if (el) el.innerHTML = ",
                    selector.0
                ));
                self.emit_expr(value);
                self.write("; }");
                self.newline();
            }
            Stmt::Focus { selector } => {
                self.write_indent();
                self.write(&format!(
                    "document.querySelector('{}')?.focus();",
                    selector.0
                ));
                self.newline();
            }
            Stmt::Blur { selector } => {
                self.write_indent();
                self.write(&format!(
                    "document.querySelector('{}')?.blur();",
                    selector.0
                ));
                self.newline();
            }
            Stmt::ScrollTo { selector, behavior } => {
                let behavior_str = match behavior {
                    crate::stmt::ScrollBehavior::Smooth => "smooth",
                    crate::stmt::ScrollBehavior::Instant => "instant",
                    crate::stmt::ScrollBehavior::Auto => "auto",
                };
                self.write_indent();
                self.write(&format!(
                    "document.querySelector('{}')?.scrollIntoView({{ behavior: '{}' }});",
                    selector.0, behavior_str
                ));
                self.newline();
            }
            Stmt::DispatchEvent {
                selector,
                event,
                detail,
            } => {
                self.write_indent();
                if let Some(ref detail_expr) = detail {
                    self.write(&format!(
                        "document.querySelector('{}')?.dispatchEvent(new CustomEvent('{}', {{ detail: ",
                        selector.0, event
                    ));
                    self.emit_expr(detail_expr);
                    self.write(" }));");
                } else {
                    self.write(&format!(
                        "document.querySelector('{}')?.dispatchEvent(new Event('{}'));",
                        selector.0, event
                    ));
                }
                self.newline();
            }
            Stmt::For {
                var, iter, body, ..
            } => {
                self.write_indent();
                self.write(&format!("for (const {} of ", var.0));
                self.emit_expr(iter);
                self.write(") {");
                self.newline();
                self.indent += 1;
                self.emit_block(body);
                self.indent -= 1;
                self.write_indent();
                self.write("}");
                self.newline();
            }
            Stmt::CopyToClipboard { value } => {
                self.write_indent();
                self.write("navigator.clipboard.writeText(");
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
            Stmt::RemoveLocalStorage { key } => {
                self.write_indent();
                self.write(&format!("localStorage.removeItem('{}');", key));
                self.newline();
            }
            Stmt::SetSessionStorage { key, value } => {
                self.write_indent();
                self.write(&format!(
                    "sessionStorage.setItem('{}', JSON.stringify(",
                    key
                ));
                self.emit_expr(value);
                self.write("));");
                self.newline();
            }
            Stmt::RemoveSessionStorage { key } => {
                self.write_indent();
                self.write(&format!("sessionStorage.removeItem('{}');", key));
                self.newline();
            }
            #[allow(unreachable_patterns)]
            _ => {
                self.write_indent();
                self.write(&format!("/* TODO: {:?} */", stmt));
                self.newline();
            }
        }
    }

    fn emit_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Lit { value } => self.emit_value(value),
            Expr::Signal { id } => self.write(&format!("form.signals.{}", id.0)),
            Expr::Field { id } => self.write(&format!("form.fields.{}", id.0)),
            Expr::Var { id } => self.write(&id.0),
            // Server data scope. Client runtimes that mirror server data
            // must define `$scope`; see SEMANTICS.md.
            Expr::Scope { path } => {
                self.write("$scope");
                for segment in path {
                    self.write(&format!("['{}']", escape_string(segment)));
                }
            }
            Expr::Response => self.write("res"),
            Expr::Error => self.write("err"),

            Expr::Prop { expr, prop } => {
                self.emit_expr(expr);
                self.write(&format!(".{}", prop));
            }
            Expr::Index { expr, index } => {
                self.emit_expr(expr);
                self.write("[");
                self.emit_expr(index);
                self.write("]");
            }

            // Arithmetic
            Expr::Add { left, right } => self.emit_binary(left, "+", right),
            Expr::Sub { left, right } => self.emit_binary(left, "-", right),
            Expr::Mul { left, right } => self.emit_binary(left, "*", right),
            Expr::Div { left, right } => self.emit_binary(left, "/", right),

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

            // Ternary
            Expr::Ternary { cond, then, else_ } => {
                self.write("(");
                self.emit_expr(cond);
                self.write(" ? ");
                self.emit_expr(then);
                self.write(" : ");
                self.emit_expr(else_);
                self.write(")");
            }

            // String
            Expr::Len { expr } => {
                self.emit_expr(expr);
                self.write(".length");
            }
            Expr::Trim { expr } => {
                self.emit_expr(expr);
                self.write(".trim()");
            }

            // Form helpers
            Expr::FormValid => self.write("form.isValid()"),
            Expr::FormDirty => self.write("form.isDirty()"),
            Expr::FormData => self.write("form.getData()"),
            Expr::FieldHasError { id } => {
                self.write(&format!("(form.errors.{} !== null)", id.0));
            }
            Expr::FieldError { id } => {
                self.write(&format!("form.errors.{}", id.0));
            }
            Expr::FieldTouched { id } => {
                self.write(&format!("form.touched.{}", id.0));
            }

            // String operations
            Expr::Concat { parts } => {
                if parts.is_empty() {
                    self.write("''");
                } else if parts.len() == 1 {
                    self.emit_expr(&parts[0]);
                } else {
                    self.write("(");
                    for (i, part) in parts.iter().enumerate() {
                        if i > 0 {
                            self.write(" + ");
                        }
                        self.emit_expr(part);
                    }
                    self.write(")");
                }
            }
            Expr::Matches { string, pattern } => {
                self.write(&format!("/{}/u.test(", pattern));
                self.emit_expr(string);
                self.write(")");
            }
            Expr::Contains { haystack, needle } => {
                self.emit_expr(haystack);
                self.write(".includes(");
                self.emit_expr(needle);
                self.write(")");
            }
            Expr::StartsWith { string, prefix } => {
                self.emit_expr(string);
                self.write(".startsWith(");
                self.emit_expr(prefix);
                self.write(")");
            }
            Expr::EndsWith { string, suffix } => {
                self.emit_expr(string);
                self.write(".endsWith(");
                self.emit_expr(suffix);
                self.write(")");
            }
            Expr::Upper { expr } => {
                self.emit_expr(expr);
                self.write(".toUpperCase()");
            }
            Expr::Lower { expr } => {
                self.emit_expr(expr);
                self.write(".toLowerCase()");
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
            Expr::First { array } => {
                self.emit_expr(array);
                self.write("[0]");
            }
            Expr::Last { array } => {
                self.emit_expr(array);
                self.write(".at(-1)");
            }
            Expr::Includes { array, value } => {
                self.emit_expr(array);
                self.write(".includes(");
                self.emit_expr(value);
                self.write(")");
            }
            Expr::Slice { array, start, end } => {
                self.emit_expr(array);
                self.write(".slice(");
                self.emit_expr(start);
                if let Some(ref e) = end {
                    self.write(", ");
                    self.emit_expr(e);
                }
                self.write(")");
            }
            Expr::Join { array, delimiter } => {
                self.emit_expr(array);
                self.write(".join(");
                self.emit_expr(delimiter);
                self.write(")");
            }
            Expr::Split { string, delimiter } => {
                self.emit_expr(string);
                self.write(".split(");
                self.emit_expr(delimiter);
                self.write(")");
            }
            Expr::Flat { array, depth } => {
                self.emit_expr(array);
                if let Some(d) = depth {
                    self.write(&format!(".flat({})", d));
                } else {
                    self.write(".flat()");
                }
            }

            // Object construction
            Expr::Object { entries } => {
                self.write("{ ");
                for (i, (key, val)) in entries.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.write(&format!("{}: ", key));
                    self.emit_expr(val);
                }
                self.write(" }");
            }

            // DOM references
            Expr::Target => self.write("e.target"),
            Expr::Event => self.write("e"),

            // Type coercion
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

            // Math operations
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
            Expr::Clamp { value, min, max } => {
                self.write("Math.min(Math.max(");
                self.emit_expr(value);
                self.write(", ");
                self.emit_expr(min);
                self.write("), ");
                self.emit_expr(max);
                self.write(")");
            }
            Expr::Neg { expr } => {
                self.write("-(");
                self.emit_expr(expr);
                self.write(")");
            }
            Expr::Mod { left, right } => self.emit_binary(left, "%", right),

            // Number formatting (must agree with the server evaluator's
            // FormatNumber implementation; see SEMANTICS.md)
            Expr::FormatNumber { value, decimals } => {
                self.write("(");
                self.emit_expr(value);
                self.write(&format!(").toFixed({})", decimals));
            }

            // Misc
            Expr::Coalesce { left, right } => {
                self.write("(");
                self.emit_expr(left);
                self.write(" ?? ");
                self.emit_expr(right);
                self.write(")");
            }

            // Date/time
            Expr::Now => self.write("Date.now()"),
            Expr::FormatDate { date, format } => {
                // Simple implementation - could be extended with a proper date formatter
                self.write("new Date(");
                self.emit_expr(date);
                self.write(&format!(
                    ").toLocaleDateString('en-US', {{ /* {} */ }})",
                    format
                ));
            }
            Expr::ParseDate { string, .. } => {
                self.write("new Date(");
                self.emit_expr(string);
                self.write(").getTime()");
            }

            // String operations
            Expr::Replace { string, from, to } => {
                self.emit_expr(string);
                self.write(".replace(");
                self.emit_expr(from);
                self.write(", ");
                self.emit_expr(to);
                self.write(")");
            }
            Expr::Substring { string, start, end } => {
                self.emit_expr(string);
                self.write(".substring(");
                self.emit_expr(start);
                if let Some(ref e) = end {
                    self.write(", ");
                    self.emit_expr(e);
                }
                self.write(")");
            }

            // Array higher-order functions
            Expr::Map { array, var, body } => {
                self.emit_expr(array);
                self.write(&format!(".map({} => ", var.0));
                self.emit_expr(body);
                self.write(")");
            }
            Expr::Filter {
                array,
                var,
                predicate,
            } => {
                self.emit_expr(array);
                self.write(&format!(".filter({} => ", var.0));
                self.emit_expr(predicate);
                self.write(")");
            }
            Expr::Find {
                array,
                var,
                predicate,
            } => {
                self.emit_expr(array);
                self.write(&format!(".find({} => ", var.0));
                self.emit_expr(predicate);
                self.write(")");
            }
            Expr::FindIndex {
                array,
                var,
                predicate,
            } => {
                self.emit_expr(array);
                self.write(&format!(".findIndex({} => ", var.0));
                self.emit_expr(predicate);
                self.write(")");
            }
            Expr::Some {
                array,
                var,
                predicate,
            } => {
                self.emit_expr(array);
                self.write(&format!(".some({} => ", var.0));
                self.emit_expr(predicate);
                self.write(")");
            }
            Expr::Every {
                array,
                var,
                predicate,
            } => {
                self.emit_expr(array);
                self.write(&format!(".every({} => ", var.0));
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
                self.emit_expr(array);
                self.write(&format!(".reduce(({}, {}) => ", acc_var.0, item_var.0));
                self.emit_expr(body);
                self.write(", ");
                self.emit_expr(initial);
                self.write(")");
            }

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
            Expr::MatchesPattern { expr, pattern } => {
                self.write(&format!("/{}/u.test(", pattern));
                self.emit_expr(expr);
                self.write(")");
            }

            // Form field state
            Expr::FieldDirty { id } => {
                self.write(&format!("form.dirty.{}", id.0));
            }

            // Payment helpers
            Expr::CardType { expr } => {
                self.write("detectCardType(");
                self.emit_expr(expr);
                self.write(")");
            }

            #[allow(unreachable_patterns)]
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
            Value::String(s) => self.write(&format!("'{}'", escape_string(s))),
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
                self.write("{ ");
                for (i, (key, val)) in entries.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.write(&format!("{}: ", key));
                    self.emit_value(val);
                }
                self.write(" }");
            }
        }
    }

    fn write(&mut self, s: &str) {
        self.output.push_str(s);
    }

    fn write_indent(&mut self) {
        if !self.config.minify {
            for _ in 0..self.indent {
                self.output.push_str("  ");
            }
        }
    }

    fn newline(&mut self) {
        if !self.config.minify {
            self.output.push('\n');
        }
    }
}

fn escape_string(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::{block, form};

    #[test]
    fn test_form_codegen_script() {
        let form_ir = form("signup")
            .signal("loading", false)
            .field("email", |f| {
                f.required("Email is required").email("Invalid email")
            })
            .field("password", |f| {
                f.required("Password is required")
                    .min_length(8, "Min 8 chars")
            })
            .on_submit(block().set("loading", true).build())
            .build();

        let script = FormCodegen::generate_script(&form_ir, FormCodegenConfig::default());

        assert!(script.contains("<script type=\"module\">"));
        assert!(script.contains("import { createForm"));
        assert!(script.contains("createForm('signup'"));
        assert!(script.contains("signals: {loading: false}"));
        assert!(script.contains("fields: {email: ''"));
        assert!(script.contains("rules: {"));
        assert!(script.contains("</script>"));
    }

    #[test]
    fn test_form_codegen_js_only() {
        let form_ir = form("login")
            .field("username", |f| f.required("Required"))
            .build();

        let js = FormCodegen::generate_js(&form_ir, FormCodegenConfig::default());

        assert!(!js.contains("<script"));
        assert!(js.contains("createForm('login'"));
    }

    #[test]
    fn test_validation_rules() {
        let form_ir = form("test")
            .field("email", |f| f.email("Bad email"))
            .field("password", |f| {
                f.min_length(8, "Too short").matches("confirm", "No match")
            })
            .build();

        let js = FormCodegen::generate_js(&form_ir, FormCodegenConfig::default());

        assert!(js.contains("/^[^\\s@]+@[^\\s@]+\\.[^\\s@]+$/.test(v)"));
        assert!(js.contains("v.length >= 8"));
        assert!(js.contains("v === f.confirm"));
    }
}
