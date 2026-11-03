// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { fromMarkdown } from 'mdast-util-from-markdown';
import { gfmTableFromMarkdown } from 'mdast-util-gfm-table';
import { gfmTable } from 'micromark-extension-gfm-table';
import { parser as rustParser } from '@lezer/rust';

const rustExpressions = rustParser.configure({ strict: true });

// Brace balance alone cannot distinguish Rust from a quoted Markdown example.
// Require one complete expression, without parser error recovery, before
// repairing a code span that appears to close inside that expression.
function isRustExpression(text) {
  const prefix = 'fn __proa() { let _ = (';
  try {
    const tree = rustExpressions.parse(prefix + rustValidationSource(text) + '\n); }');
    const expression = tree.topNode.getChild('FunctionItem')?.getChild('Block')
      ?.getChild('LetDeclaration')?.getChild('ParenthesizedExpression');
    return expression?.from === prefix.length - 1
      && expression.to === prefix.length + text.length + 2;
  } catch (error) {
    if (error instanceof SyntaxError) return false;
    throw error;
  }
}

const tableDelimiter = /^\|?[ \t\v\f]*:?-+:?[ \t\v\f]*(?:\|[ \t\v\f]*:?-+:?[ \t\v\f]*)*\|?[ \t\v\f]*$/;

function tableCellCount(row) {
  const cells = row.trim().split(/(?<!\\)\|/);
  if (cells[0] === '') cells.shift();
  if (cells.at(-1) === '') cells.pop();
  return cells.length;
}

function markdownParse(text) {
  const extension = gfmTable();
  const tokenize = extension.flow.null.tokenize;
  extension.flow.null.tokenize = function (effects, ok, nok) {
    const current = /^[^\r\n]*/.exec(text.slice(this.now().offset))[0];
    // A single pipe has no cells and ends a table in cmark's grammar.
    if (current.trim() === '|') return nok;
    if (this.interrupt) {
      const chunks = [];
      for (let i = this.events.length - 1; i >= 0; i--) {
        const [kind, token] = this.events[i];
        if (token.type === 'content') break;
        if (kind === 'enter' && token.type === 'chunkContent') chunks.push(token);
      }
      const lines = chunks.reverse().flatMap(token => this.sliceSerialize(token)
        .split(/\r\n?|\n/).map((value, n) => ({ value, line: token.start.line + n })));
      // cmark does not retry table detection in a paragraph after a valid
      // delimiter failed to match its header's cell count. micromark does.
      if (lines.length) {
        const candidates = [...lines.slice(1), { value: current, line: this.now().line }];
        if (candidates.some(({ value, line }) => !this.parser.lazy[line] && tableDelimiter.test(value))) return nok;
      }
    }
    return tokenize.call(this, effects, ok, nok);
  };
  // GFM treats a pipe preceded by a backslash as cell content even when more
  // backslashes precede it. Neutralize the earlier slashes without moving spans.
  const view = text.replace(/\\+(?=\|)/g, run => 'x'.repeat(run.length - 1) + '\\');
  return fromMarkdown(view, { extensions: [extension], mdastExtensions: [gfmTableFromMarkdown()] });
}

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

function rustValidationSource(source) {
  // Lezer 1.0.3 recognizes raw byte strings but not raw C strings. For syntax
  // validation only, give each raw C-string token the equivalent byte prefix.
  // Skip other literals/comments so their contents remain opaque.
  let text = source;
  for (let at = 0; at < source.length;) {
    const end = comment(source, at);
    if (end !== at) { at = end; continue; }
    const cString = source.startsWith('cr', at) && (at === 0 || !ident.test(source[at - 1]));
    const raw = rawString(source, cString ? at + 1 : at);
    if (raw) {
      if (cString) text = text.slice(0, at) + 'b' + text.slice(at + 1);
      at = raw.after;
    } else if (source[at] === '"' || source[at] === "'") {
      at = Math.max(at + 1, quoted(source, at));
    } else at++;
  }
  return text;
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

// Apply the compiler's CRLF normalization and exact common-prefix dedenting,
// retaining a map from normalized UTF-16 offsets to the authored raw literal.
function templateSource(source, literal) {
  const chars = [], offsets = [];
  for (let i = literal.start; i < literal.end; i++) {
    if (source[i] === '\r' && source[i + 1] === '\n') continue;
    chars.push(source[i]);
    offsets.push(i);
  }
  const text = chars.join('');
  if (!text.includes('\n') || !text.replace(/\p{White_Space}/gu, '')) return { text, offsets };
  // Raw literals without an opening newline retain the first line and remove
  // the existing continuation margin, matching MappedLiteral in proa_dsl.
  if (!text.startsWith('\n')) {
    const lines = text.split('\n');
    const widths = lines.slice(1).filter(line => line.replace(/[ \t\r]/g, '')).map(line => /^[ \t]*/.exec(line)[0].length);
    const margin = widths.reduce((minimum, width) => Math.min(minimum, width), widths[0] ?? 0);
    if (!margin) return { text, offsets };
    const normalized = [], mapped = [];
    let cursor = 0;
    for (let n = 0; n < lines.length; n++) {
      const line = lines[n];
      const skip = n === 0 ? 0 : line.replace(/[ \t\r]/g, '') ? margin : line.length;
      for (let i = cursor + skip; i < cursor + line.length; i++) { normalized.push(chars[i]); mapped.push(offsets[i]); }
      cursor += line.length;
      if (n + 1 < lines.length) { normalized.push('\n'); mapped.push(offsets[cursor++]); }
    }
    return { text: normalized.join(''), offsets: mapped };
  }
  const lines = text.slice(1).split('\n');
  let common;
  for (const line of lines) {
    if (!line.replace(/\p{White_Space}/gu, '')) continue;
    const prefix = /^\p{White_Space}*/u.exec(line)[0];
    if (common === undefined) common = prefix;
    else {
      let end = 0;
      while (end < common.length && common[end] === prefix[end]) end++;
      common = common.slice(0, end);
    }
  }
  common ??= '';
  const normalized = [], mapped = [];
  let cursor = 1;
  for (let n = 0; n < lines.length; n++) {
    const line = lines[n];
    let margin = 0;
    while (margin < common.length && line[margin] === common[margin]) margin++;
    for (let i = cursor + margin; i < cursor + line.length; i++) {
      normalized.push(chars[i]);
      mapped.push(offsets[i]);
    }
    cursor += line.length;
    if (n + 1 < lines.length) {
      normalized.push('\n');
      mapped.push(offsets[cursor++]);
    }
  }
  return { text: normalized.join(''), offsets: mapped };
}

function markdownTree(text) {
  let tree = markdownParse(text);
  const breaks = new Set(), adjustments = [], pending = [tree];
  while (pending.length) {
    const node = pending.pop();
    let paragraph = node.children?.[0];
    while (paragraph && ['list', 'listItem'].includes(paragraph.type)) paragraph = paragraph.children[0];
    if (node.type === 'blockquote' && paragraph?.type === 'paragraph') {
      const first = text.slice(paragraph.position.start.offset, paragraph.position.end.offset).split('\n')[0];
      const line = text.lastIndexOf('\n', node.position.start.offset - 1) + 1;
      // micromark 4 carries paragraph-interruption restrictions into a new
      // blockquote, treating an empty list marker as paragraph text. A virtual
      // blank line resets that state at an existing block boundary. It never
      // changes emitted text, and offsets below map back to the real source.
      if (line > 0 && /^(?:[-+*]|\d{1,9}[.)])(?:[ \t]+(?:[-+*]|\d{1,9}[.)]))*[ \t]*$/.test(first)) breaks.add(line);
    }
    if (node.type === 'paragraph' || (node.type === 'heading' && text[node.position.start.offset] !== '#')) {
      // An indented continuation of an existing paragraph may become a table
      // header. micromark skips table detection at four columns of indentation.
      // Removing only excess continuation whitespace preserves the container.
      const baseline = node.position.start.column - 1;
      let start = text.indexOf('\n', node.position.start.offset) + 1;
      while (start > 0 && start < node.position.end.offset) {
        const end = text.indexOf('\n', start);
        const line = text.slice(start, end < 0 ? text.length : end);
        const prefix = /^[> \t]*/.exec(line)[0];
        let keep = 0, column = 0;
        while (keep < prefix.length && column < baseline) {
          column += prefix[keep++] === '\t' ? 4 - column % 4 : 1;
        }
        const following = end >= 0 ? /^[^\r\n]*/.exec(text.slice(end + 1))[0].replace(/^[> \t]*/, '') : '';
        if (end < node.position.end.offset && tableDelimiter.test(following)
            && /[|:]/.test(following) && /^[ \t]+$/.test(prefix.slice(keep))
            && tableCellCount(line.slice(prefix.length)) === tableCellCount(following)) {
          // A leading pipe keeps a header that starts with a fence/list marker
          // inline without introducing another cell.
          adjustments.push([start + keep, start + prefix.length, line[prefix.length] === '|' ? '' : '|']);
          if (tableDelimiter.test(line.slice(prefix.length))) {
            // This is a header, not a delimiter for the preceding paragraph.
            // Its punctuation contains no code; change one marker in the view
            // so removing indentation cannot turn it into that delimiter.
            const marker = start + line.indexOf('-', prefix.length);
            adjustments.push([marker, marker + 1, 'x']);
          }
          break;
        }
        start = end + 1;
      }
    }
    if (node.children) for (const child of node.children) pending.push(child);
  }
  const edits = [...breaks].map(at => [at, at, '\n']).concat(adjustments).sort((a, b) => a[0] - b[0]);
  if (!edits.length) return { tree, original: offset => offset };
  const chunks = [], boundaries = [0];
  let start = 0;
  for (const [at, end, replacement] of edits) {
    chunks.push(text.slice(start, at), replacement);
    for (let i = start; i < at; i++) boundaries.push(i + 1);
    for (let i = 0; i < replacement.length; i++) boundaries.push(at);
    boundaries[boundaries.length - 1] = end;
    start = end;
  }
  chunks.push(text.slice(start));
  for (let i = start; i < text.length; i++) boundaries.push(i + 1);
  const normalized = chunks.join('');
  tree = markdownParse(normalized);
  return { tree, original: offset => boundaries[offset] };
}

function codeRanges(text) {
  const { tree, original } = markdownTree(text);
  const ranges = [], pending = [tree];
  while (pending.length) {
    const node = pending.pop();
    if (node.type === 'inlineCode' || node.type === 'code') {
      ranges.push({ start: original(node.position.start.offset), end: original(node.position.end.offset), block: node.type === 'code' });
    } else if (node.children) {
      for (let i = node.children.length - 1; i >= 0; i--) pending.push(node.children[i]);
    }
  }
  return ranges;
}

// Preserve UTF-8 and UTF-16 widths while making a Rust slot opaque to Markdown.
function maskSlot(text, start, end) {
  const mask = [...text.slice(start, end)].map(ch => {
    const point = ch.codePointAt(0);
    return point < 128 ? 'x' : point < 2048 ? 'é' : point < 65536 ? '中' : '🦀';
  }).join('');
  return text.slice(0, start) + mask + text.slice(end);
}

function expressions(source, literal, statement = false) {
  const { text, offsets } = templateSource(source, literal);
  const ranges = [], copies = [], replacements = [];
  let masked = text, code = codeRanges(text), nextCode = 0;
  for (let i = 0; i < text.length;) {
    while (code[nextCode]?.end <= i) nextCode++;
    if (code[nextCode]?.start <= i) {
      const range = code[nextCode];
      let crossing;
      if (!range.block && /["'/]/.test(masked.slice(range.start, range.end))) {
        for (let at = range.start; at < range.end;) {
          if (masked.startsWith('{{', at)) { at += 2; continue; }
          if (masked[at] === '{') {
            const end = expressionEnd(masked, at, masked.length);
            if (end === null) break;
            if (end > range.end && isRustExpression(masked.slice(at + 1, end - 1))) {
              crossing = [at, end]; break;
            }
            at = end;
          } else at++;
        }
      }
      if (crossing) {
        masked = maskSlot(masked, ...crossing);
        code = codeRanges(masked).filter(range => range.end > i);
        nextCode = 0;
        continue;
      }
      i = range.end;
      continue;
    }
    if (text.startsWith('{{', i) || text.startsWith('}}', i)) { i += 2; continue; }
    if (text[i] === '{') {
      const end = expressionEnd(text, i, text.length);
      if (end === null) return null;
      const authored = { start: offsets[i], end: offsets[end - 1] + 1 };
      ranges.push(authored);
      copies.push(authored);
      if (statement) replacements.push([authored.start, ' '], [authored.end - 1, ';']);
      // Rust slots are opaque to Markdown, including their physical newlines.
      // Rescan after masking so Rust strings cannot open a fence outside a slot.
      if (/[`~\n\r\t<>|\[\]\\"']/.test(masked.slice(i, end))) {
        masked = maskSlot(masked, i, end);
        code = codeRanges(masked).filter(range => range.start >= end);
        nextCode = 0;
      }
      i = end;
    } else if (text[i] === '}') return null;
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
    if (incomplete) parsed = expressions(source, literal, true);
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
