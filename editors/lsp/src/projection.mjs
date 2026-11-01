// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Editor-only projection. Never write this text to a source file.
// Offsets are JavaScript/LSP UTF-16 code units; retain every line break.
const ident = /[\p{ID_Continue}]/u;

function rawString(source, at) {
  const opening = /^r(#+)?"/.exec(source.slice(at));
  if (!opening) return null;
  const start = at + opening[0].length;
  const closing = '"' + (opening[1] ?? '');
  const end = source.indexOf(closing, start);
  return end < 0 ? null : { start, end, after: end + closing.length };
}

function quoted(source, at) {
  const quote = source[at];
  // An apostrophe may begin a lifetime instead of a character literal.
  if (quote === "'") {
    const character = /^'(?:\\(?:u\{[\da-fA-F_]+\}|x[\da-fA-F]{2}|.)|[^'\\\r\n])'/u.exec(source.slice(at));
    return character ? at + character[0].length : at;
  }
  for (let i = at + 1; i < source.length; i++) {
    if (source[i] === '\\') i++;
    else if (source[i] === quote) return i + 1;
  }
  return source.length;
}

function comment(source, at) {
  if (source.startsWith('//', at)) {
    const end = source.indexOf('\n', at + 2);
    return end < 0 ? source.length : end;
  }
  if (!source.startsWith('/*', at)) return at;
  let depth = 1;
  for (let i = at + 2; i < source.length;) {
    if (source.startsWith('/*', i)) { depth++; i += 2; }
    else if (source.startsWith('*/', i)) {
      i += 2;
      if (--depth === 0) return i;
    } else i++;
  }
  return source.length;
}

function trivia(source, at) {
  while (at < source.length) {
    if (/\s/u.test(source[at])) at++;
    else {
      const end = comment(source, at);
      if (end === at) break;
      at = end;
    }
  }
  return at;
}

function expressionEnd(source, at, limit) {
  let depth = 1;
  for (let i = at + 1; i < limit;) {
    const end = comment(source, i);
    if (end !== i) { i = end; continue; }
    const raw = source[i] === 'r' ? rawString(source, i) : null;
    if (raw) { i = raw.after; continue; }
    if (source[i] === '"' || source[i] === "'") {
      const end = quoted(source, i);
      if (end !== i) { i = end; continue; }
    }
    if (source[i] === '{') depth++;
    if (source[i] === '}' && --depth === 0) return i + 1;
    i++;
  }
  return null;
}

// The compiler removes common source indentation before recognizing fences.
function indentation(source, literal) {
  const lines = source.slice(literal.start, literal.end).split('\n').slice(1);
  const widths = lines.filter(line => line.trim()).map(line => /^[ \t]*/.exec(line)[0].length);
  return widths.length ? Math.min(...widths) : 0;
}

function expressions(source, literal, indent = indentation(source, literal), statement = false) {
  const ranges = [], copies = [], replacements = [];
  const keep = (start, end) => copies.push({ start, end });
  let fence = null;
  for (let i = literal.start; i < literal.end;) {
    if (i === literal.start || source[i - 1] === '\n') {
      const lineEnd = Math.min(source.indexOf('\n', i) < 0 ? literal.end : source.indexOf('\n', i), literal.end);
      const line = source.slice(i, lineEnd);
      const leading = /^[ \t]*/.exec(line)[0].length;
      const marker = /^ {0,3}(`{3,}|~{3,})(.*)$/.exec(line.slice(Math.min(indent, leading)));
      if (fence) {
        if (marker && marker[1][0] === fence[0] && marker[1].length >= fence.length && !marker[2].trim()) fence = null;
        i = lineEnd + 1;
        continue;
      }
      if (marker && (marker[1][0] !== '`' || !marker[2].includes('`'))) { fence = marker[1]; i = lineEnd + 1; continue; }
    }
    if (source[i] === '`') {
      let end = i + 1;
      while (source[end] === '`') end++;
      const ticks = source.slice(i, end);
      let close = source.indexOf(ticks, end);
      while (close >= 0 && close < literal.end && (source[close - 1] === '`' || source[close + ticks.length] === '`')) close = source.indexOf(ticks, close + ticks.length);
      i = close >= 0 && close < literal.end ? close + ticks.length : end;
      continue;
    }
    if (source.startsWith('{{', i) || source.startsWith('}}', i)) { i += 2; continue; }
    if (source[i] === '{') {
      // A slot is one Rust expression, control flow included. Markdown
      // re-enters a branch only through a nested template literal, which is
      // an ordinary Rust string token to this scanner.
      const end = expressionEnd(source, i, literal.end);
      if (end === null) return null;
      ranges.push({ start: i, end });
      keep(i, end);
      // In block mode each slot becomes a statement, retaining its scope and
      // exact source coordinates. The compiler still checks the original
      // template's render bounds.
      if (statement) replacements.push([i, ' '], [end - 1, ';']);
      i = end;
    } else if (source[i] === '}') return null;
    else i++;
  }
  return { ranges, copies, replacements };
}

// An unfinished field access ends a slot or a control-flow body inside it:
// `{a.}` or `{if ready { a. }}`. Strings, characters, and comments are
// skipped so a `.` inside them is content, and `..` is a range.
function unfinishedAccess(source, start, end) {
  for (let i = start; i < end;) {
    const skipped = comment(source, i);
    if (skipped !== i) { i = skipped; continue; }
    const raw = source[i] === 'r' ? rawString(source, i) : null;
    if (raw) { i = raw.after; continue; }
    if (source[i] === '"' || source[i] === "'") {
      const quoteEnd = quoted(source, i);
      if (quoteEnd !== i) { i = quoteEnd; continue; }
    }
    if (source[i] !== '.') { i++; continue; }
    let next = i + 1;
    while (next < end && source[next] === '.') next++;
    if (next === i + 1) {
      const after = trivia(source, next);
      if (after >= end || source[after] === '}') return true;
    }
    i = next;
  }
  return false;
}

export function project(source) {
  const templates = [];
  let output = null;
  for (let i = 0; i < source.length;) {
    const end = comment(source, i);
    if (end !== i) { i = end; continue; }
    const raw = source[i] === 'r' ? rawString(source, i) : null;
    if (raw) { i = raw.after; continue; }
    if (source[i] === '"' || source[i] === "'") {
      const end = quoted(source, i);
      if (end !== i) { i = end; continue; }
    }
    if (!ident.test(source[i])) { i++; continue; }
    const wordStart = i++;
    while (i < source.length && ident.test(source[i])) i++;
    if (!['md', 'md_sync'].includes(source.slice(wordStart, i))) continue;
    let cursor = trivia(source, i);
    if (source[cursor] !== '!') continue;
    cursor = trivia(source, cursor + 1);
    const closing = { '{': '}', '(': ')', '[': ']' }[source[cursor]];
    if (!closing) continue;
    const literalStart = trivia(source, cursor + 1);
    const literal = rawString(source, literalStart);
    // Leave cooked strings, named arguments, and token composition alone.
    if (!literal || source[trivia(source, literal.after)] !== closing) continue;
    let parsed = expressions(source, literal);
    i = literal.after;
    if (!parsed?.ranges.length) continue;
    // A proc macro cannot expand an unfinished field access. While typing,
    // expose an ordinary Rust block instead, so rust-analyzer can recover the
    // expression and complete its fields without needing a macro expansion.
    // Complete templates retain the macro and its actual render result type.
    const incomplete = parsed.ranges.some(range => unfinishedAccess(source, range.start + 1, range.end - 1));
    if (incomplete) parsed = expressions(source, literal, indentation(source, literal), true);
    output ??= source.split('');
    if (incomplete) {
      let pathStart = wordStart;
      // Include qualified paths such as ::proa_macros::md_sync!.
      for (;;) {
        let end = pathStart;
        while (end > 0 && /\s/u.test(source[end - 1])) end--;
        if (source.slice(end - 2, end) !== '::') break;
        pathStart = end - 2;
        let segmentEnd = pathStart;
        while (segmentEnd > 0 && /\s/u.test(source[segmentEnd - 1])) segmentEnd--;
        let segmentStart = segmentEnd;
        while (segmentStart > 0 && ident.test(source[segmentStart - 1])) segmentStart--;
        const segment = source.slice(segmentStart, segmentEnd);
        // A leading global path can follow a keyword: return ::proa::md!.
        if (!segment || /^(?:as|async|await|break|const|continue|dyn|else|enum|extern|false|fn|for|if|impl|in|let|loop|match|mod|move|mut|pub|ref|return|static|struct|trait|true|type|unsafe|use|where|while|yield)$/.test(segment)) break;
        pathStart = segmentStart;
      }
      for (let n = pathStart; n < cursor; n++) {
        if (source[n] !== '\r' && source[n] !== '\n') output[n] = ' ';
      }
      output[cursor] = '{';
      output[trivia(source, literal.after)] = '}';
    }
    for (let n = literalStart; n < literal.after; n++) {
      if (source[n] !== '\r' && source[n] !== '\n') output[n] = ' ';
    }
    for (const range of parsed.copies) {
      for (let n = range.start; n < range.end; n++) output[n] = source[n];
    }
    for (const [offset, value] of parsed.replacements) output[offset] = value;
    templates.push({ start: literalStart, end: literal.after, expressions: parsed.ranges });
  }
  return { source, text: output ? output.join('') : source, templates };
}

export function offsetAt(source, position) {
  let offset = 0;
  for (let line = 0; line < position.line; line++) {
    const next = source.indexOf('\n', offset);
    if (next < 0) return source.length;
    offset = next + 1;
  }
  return Math.min(source.length, offset + position.character);
}

export function applyChanges(source, changes) {
  for (const change of changes) {
    source = change.range
      ? source.slice(0, offsetAt(source, change.range.start)) + change.text + source.slice(offsetAt(source, change.range.end))
      : change.text;
  }
  return source;
}

export function safeRange(document, range) {
  const start = offsetAt(document.source, range.start);
  const end = offsetAt(document.source, range.end);
  return document.templates.every(template => {
    // Insertions at the outer literal boundaries are ordinary Rust edits.
    if (end <= template.start || start >= template.end) return true;
    return template.expressions.some(expr => start > expr.start && end < expr.end);
  });
}
