// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! JavaScript expression emission for evaluator parity tests and tooling.

use crate::{Expr, Value};

#[derive(Default)]
struct ExpressionWriter {
    output: String,
}

impl ExpressionWriter {
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
}

/// Render a single [`Expr`] as a standalone JavaScript expression string.
///
/// The generated code references the runtime names this codegen uses for
/// client roots (`$s`, `$f`, `$scope`, `e`, ...), so it is self-contained
/// only for the server dialect (literals, `Scope`, `Var`, and pure ops).
/// Used for evaluator/emitter parity testing and tooling.
pub fn expr_to_js(expr: &Expr) -> String {
    let mut codegen = ExpressionWriter::default();
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
