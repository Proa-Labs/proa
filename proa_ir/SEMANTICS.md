# proa_ir server evaluation semantics

The same `Expr` IR runs in two worlds: compiled to JavaScript for the
client (`codegen/js.rs` legacy emitter, `codegen/form.rs` split emitter)
and interpreted on the server by `eval` (`src/eval/`). An expression must
not evaluate differently in the two worlds, so the server evaluator is
pinned to the JS emitters' semantics. This document records every
semantics decision and every deliberate divergence.

Where the two JS emitters disagree with each other, the richer
`codegen/form.rs` emitter is the reference (it implements far more of the
expression grammar; `codegen/js.rs` falls back to `/* TODO */` comments
for the rest).

## Number model

- `Value::Int` and `Value::Float` are one observable "JS number" type.
  `Int` exists so authoring and serialization stay exact for integer
  data; equality, ordering, and stringification treat `Int(3)` and
  `Float(3.0)` identically (`3 === 3.0` in JS).
- All arithmetic (`Add`/`Sub`/`Mul`/`Div`/`Mod`/`Neg`, math ops) is
  performed in `f64` and produces `Float`. This reproduces JS behavior
  including precision loss above 2^53: `9007199254740992 + 1` evaluates
  to `9007199254740992`.
- `Div` by zero yields `Infinity`/`-Infinity`/`NaN` like JS, never an
  error. `Mod` is the JS remainder (sign of the dividend): `-5 % 3` is
  `-2`.
- Operands coerce via JS `ToNumber`: `null` is 0, booleans are 0/1,
  strings parse per `StringToNumber` (empty/whitespace is 0, `0x`/`0o`/
  `0b` prefixes supported, otherwise the whole trimmed string must be a
  decimal literal or the result is `NaN`), arrays coerce through their
  joined string form (`[5] - 1` is `4`, `[1,2] * 1` is `NaN`), objects
  are `NaN`.
- Number-to-string matches JS `String(n)`: shortest round-trip digits,
  `"NaN"`, `"Infinity"`, `-0` prints `"0"`, integral floats print with no
  decimal point (`String(3.0)` is `"3"`), exponent form outside
  `[1e-6, 1e21)` (`"1e+21"`, `"1e-7"`).

Divergences (documented, not observable for realistic page data):

- `Int` literals beyond 2^53 keep full i64 precision when stringified
  directly (`ToString(Int(2^53 + 1))` prints the exact integer; a JS
  number literal could not represent it). Arithmetic collapses them to
  f64 exactly like JS. Do not author integer literals above 2^53.
- Exponent-form mantissas rely on Rust's shortest-round-trip formatting,
  which matches V8 for the range page data lives in.

## `Add` and `Concat`

- `Add` is the full JS `+`: after `ToPrimitive` (arrays/objects become
  their joined-string form), if either side is a string the result is
  string concatenation, otherwise numeric addition. `true + true` is `2`,
  `"a" + null` is `"anull"`, `[1,2] + "!"` is `"1,2!"`.
- `Concat { parts }` is exactly the emitted `+` chain, left-folded. It is
  NOT a string join: `Concat[1, 2, "x"]` is `"3x"` because the emitter
  produces `(1 + 2 + "x")`. Zero parts yield `""`; a single part passes
  through unchanged (matching `FormCodegen`). Wrap parts in `ToString` if
  string behavior is needed regardless of arity.

## Truthiness: strict boolean

The server evaluator is strict-boolean:
`And`/`Or`/`Not` operands, `Ternary` conditions, and combinator
predicates (`Filter`/`Find`/`FindIndex`/`Some`/`Every`) must evaluate to
`Value::Bool` or evaluation fails with `TypeMismatch`.

This is a deliberate, documented divergence from JS truthiness: the
emitted `&&`/`||`/`!`/`?:` accept any value and coerce. When operands are
booleans (which the server dialect requires and the dialect validator
plus authoring layer enforce), the two worlds agree exactly, including
short-circuit behavior. `And`/`Or` short-circuit, so a non-boolean or
even client-only right side is never evaluated when the left side
decides.

Note the JS `&&`/`||` return the deciding *operand*; with boolean
operands that value equals the boolean the server produces.

## Comparison

- `Eq`/`Ne` are JS strict equality (`===`/`!==`), as emitted: no type
  coercion (`1 == "1"` is false), numbers compare by value across
  `Int`/`Float`, `NaN !== NaN`.
- Divergence: `Eq`/`Ne` on arrays or objects is an
  `EvalError::CompositeComparison` instead of a result. JS compares
  composites by reference identity, which serialized server values do not
  have; failing loudly beats silently answering `false` where the client
  might answer `true` for the same-reference case.
- `Lt`/`Le`/`Gt`/`Ge` follow the JS abstract relational algorithm: if
  both primitives are strings, lexicographic comparison; otherwise both
  sides coerce with `ToNumber`, and any `NaN` makes every comparison
  false (`"abc" < 5` and `"abc" >= 5` are both false). `"10" < "9"` is
  true (string compare); `"5" < 10` is true (numeric compare).
- `Coalesce` (`??`) fires only on `Null` (the IR has no separate
  `undefined`; missing properties and out-of-range indexes already
  resolve to `Null`).
- `Includes`/`Contains` on arrays use SameValueZero like
  `Array.prototype.includes`: `NaN` matches `NaN`. Composite elements
  never match (JS reference inequality for distinct references; the
  same-reference case is not representable server-side).

## Strings

- `Len` and all index math are UTF-16 code units, matching JS
  `.length`: `"👍"` has length 2. Divergence: slicing that would split a
  surrogate pair produces U+FFFD replacement characters, because Rust
  strings cannot hold lone surrogates. `Split` with an empty delimiter
  splits by chars, not code units (differs only for non-BMP input).
- `Trim` trims the JS whitespace set. Divergence: JS also strips U+FEFF
  and does not strip U+0085; the evaluator handles both correctly, but
  Rust's `char::is_whitespace` alone would not.
- `Substring` is JS `String.prototype.substring` (as emitted by
  `FormCodegen`): NaN and negative arguments clamp to 0, arguments clamp
  to length, and start/end swap when reversed. `Slice` is
  `Array/String.prototype.slice`: negative indexes count from the end.
- `Replace` is the emitted `.replace(from, to)` with a string pattern:
  first occurrence only. Divergence: JS interprets `$$`, `$&`, `` $` ``,
  `$'` in the replacement string; the evaluator inserts the replacement
  literally. Avoid `$` sequences in replacements (or accept the
  divergence: money strings like `"$5"` are inserted literally, which is
  almost always what authors mean).
- `StartsWith`/`EndsWith`/`Contains` require a string (or array for
  `Contains`) base like the emitted method calls; the needle coerces via
  `ToString`. String ops on `null` or numbers are `TypeMismatch` errors
  where JS would throw a TypeError; `Len` on numbers/booleans is `Null`
  where JS yields `undefined`.
- `Lower`/`Upper` use Rust's Unicode case mapping; identical to JS for
  ASCII and virtually all practical input (neither is locale-sensitive).
- `Matches` compiles the pattern with the Rust `regex` crate and tests
  the `ToString` of the operand, like the emitted `/pattern/u.test(x)`.
  Divergences: no backreferences or lookaround (pattern fails to compile,
  surfacing `InvalidRegex` instead of matching), and the pattern is
  always Unicode-mode. Invalid patterns are runtime `EvalError`s
  server-side but JS syntax errors client-side; validate patterns at
  authoring time.

## Property and index access: null propagation

Server dialect decision (RFC): `Prop`, `Index`, and `Scope` are
null-propagating like optional chaining. `Prop` on `Null`, a missing
object key, a missing scope path segment, an out-of-range or non-integral
or negative index: all produce `Null` rather than an error. Divergence:
the emitted JS is plain `.prop`/`[i]` access, which throws on
null/undefined bases. Runtime-template interpreters prefer rendering an empty
slot over crashing the page; the dialect validator plus typed view models make
the client path safe.

Details pinned to JS:

- Duplicate object keys: last entry wins (JS object literal semantics).
- `Index` keys go through JS `ToPropertyKey` stringification:
  `arr[1.0]` hits index 1, `arr["1"]` hits, `arr["01"]` and `arr[1.5]`
  and `arr[-1]` miss (canonical numeric strings only).
- `"length"` works on arrays and strings via `Prop` and `Index`.
- String indexing yields single UTF-16 unit strings like JS.

## Arrays

- `Map`/`Filter`/`Find`/`FindIndex`/`Some`/`Every`/`Reduce` require an
  array base (as in JS, where calling `.map` on anything else throws) and
  bind their variable per element; inner bindings shadow outer ones.
  `Reduce` always has an explicit initial value (the IR has no
  initial-less form).
- Empty arrays: `Every` is true, `Some` is false, `Find` is `Null`,
  `FindIndex` is `-1`, `Reduce` returns the initial value, `First`/`Last`
  are `Null`, `Join` is `""`.
- `Flat` defaults to depth 1 (`.flat()`), like the emitter.
- `Join` stringifies elements with `ToString` except `Null`, which
  becomes the empty string (JS `Array.prototype.join`).

## Math

- `Round` is JS `Math.round`: half-up toward positive infinity
  (`Math.round(-2.5)` is `-2`), implemented against the exact midpoint to
  avoid the `floor(x + 0.5)` floating-point trap
  (`Math.round(0.49999999999999994)` is `0`).
- `Min`/`Max` match `Math.min`/`Math.max`: empty argument lists give
  `Infinity`/`-Infinity`, `NaN` propagates (unlike `f64::min/max`), and
  `-0`/`+0` order correctly.
- `Clamp` is the emitted `Math.min(Math.max(v, min), max)` composition
  (the `FormCodegen` form; the legacy `js.rs` emitter composes as
  `Math.max(min, Math.min(max, v))`, which differs only in the
  pathological `min > max` case).

## FormatNumber (toFixed)

`FormatNumber { value, decimals }` is JS `Number.prototype.toFixed`,
emitted as `(value).toFixed(d)` in both emitters and implemented
digit-exactly on the server:

- The operand coerces via `ToNumber` first (the node evaluates its
  argument; feed it numbers).
- Rounding picks the closer scaled integer and resolves exact ties away
  from zero on the magnitude, per spec. This is NOT Rust's
  `format!("{:.2}")` (ties-to-even on the binary value):
  `(0.125).toFixed(2)` is `"0.13"` in both worlds, where Rust formatting
  would say `"0.12"`. The implementation expands the double's exact
  decimal digits and rounds on the digit string, so classic traps match
  V8: `(1.005).toFixed(2)` is `"1.00"`, `(0.995).toFixed(2)` is
  `"0.99"`, `(0.1 + 0.2).toFixed(2)` is `"0.30"`.
- `NaN` gives `"NaN"`; `|x| >= 1e21` falls back to `ToString`
  (`"1e+21"`); `-0` gets no sign but negative values rounding to zero
  keep theirs (`"-0.00"`).
- `decimals` above 100 (the JS RangeError bound) is
  `EvalError::InvalidDecimals`.
- Note: the exact-expansion path formats up to ~1100 digits per call. It
  is correct-first; if money formatting shows up in profiles, add a fast
  path for small-magnitude values later.

The canonical money shape is
`Concat["$", FormatNumber(Div(cents, 100), 2)]`.

## Dates

- `FormatDate` treats its operand as epoch milliseconds and formats in
  UTC with tokens `YYYY YY MM M DD D HH H mm m ss s SSS`. Non-finite or
  out-of-range (beyond +/-8.64e15 ms) input yields `"Invalid Date"`.
- Divergence: both JS emitters currently emit a placeholder for
  `FormatDate` (`FormCodegen` calls `toLocaleDateString('en-US')` and
  ignores the format string; the legacy emitter emits a TODO comment).
  The server implementation defined here is the reference; the client
  emitters must be brought in line when client-side date formatting is
  needed. Formatting is UTC by design so server renders are
  deterministic; locale/timezone-aware formatting belongs in registry
  components.
- `Now` is impure and `ParseDate` is unimplemented; both are
  `EvalError::Unsupported` server-side and rejected by the dialect
  validator.

## Server dialect and the validator

`dialect::validate_server_dialect(expr)` rejects, with the variant name
and a path into the expression tree:

- Client-only nodes: `Signal`, `Field`, `Event`, `Target`, `Response`,
  `Error`, `FieldHasError`, `FieldError`, `FieldTouched`, `FieldDirty`,
  `FormValid`, `FormDirty`, `FormData`, the validation helpers
  (`IsEmail`, `IsUrl`, `MatchesPattern`), and the payment helper
  (`CardType`).
- `Now` (impure) and `ParseDate` (unsupported).

Everything else in the grammar is evaluable. The evaluator enforces the
same boundary at runtime (`ClientOnly`/`Unsupported` errors), so an
unvalidated expression still cannot reach client-only state.

`Stmt` is not reachable from `Expr` (expressions are pure and contain no
statements), so no statement-level validation is needed for expression
slots. A future `Stmt`-level server dialect is a separate concern for a
runtime-template program layer.

## Budgets

`eval_with_budget` threads an `EvalBudget` through evaluation:

- `max_depth` (default 64) bounds expression nesting; exceeding it is
  `EvalError::DepthLimit`, not a stack overflow. The evaluator is split
  into small per-category functions specifically so debug-build stack
  frames stay small enough that the depth limit, not the OS stack,
  is the binding constraint.
- `max_iterations` (default 10 000) bounds total combinator element
  visits (`Map`/`Filter`/`Find`/`FindIndex`/`Some`/`Every`/`Reduce`/
  `Flat`) across the whole evaluation, so nested loops multiply against
  one shared budget.

Evaluation never panics on malformed input; every failure is a typed
`EvalError`. Keep serde's recursion limit (default 128) in front of
deserialization: recursive `Drop` on extremely deep `Expr` trees is a
stack hazard independent of evaluation.

## Serialization

- Identifiers, prop keys, and string literals are `Cow<'static, str>`
  (`IrStr`): compile-time construction borrows (const-friendly),
  deserialized programs own.
- `Value::Object` serializes as a JSON object with insertion order
  preserved; duplicate keys round-trip and resolve last-wins on access.
- `Value::Float(NaN/Infinity)` does not survive JSON serialization
  (serde_json writes `null`); non-finite numbers arise during evaluation
  but should not be stored in programs.
