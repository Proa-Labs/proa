// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { spawn } from 'node:child_process';
import { existsSync, readdirSync, realpathSync, watch } from 'node:fs';
import { dirname, join, resolve, sep } from 'node:path';
import { createInterface } from 'node:readline';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { applyChanges } from './projection.mjs';

const ignored = path => path.split(/[\\/]/).some(part =>
  ['target', 'node_modules', '.git', '.worktrees', 'dist'].includes(part));

function packageRoot(path) {
  for (let dir = dirname(path); ; dir = dirname(dir)) {
    if (existsSync(join(dir, 'Cargo.toml'))) return dir;
    if (dirname(dir) === dir) return dirname(path);
  }
}

function canonicalURI(uri) {
  const path = fileURLToPath(uri);
  for (let ancestor = path; ; ancestor = dirname(ancestor)) {
    try { return pathToFileURL(realpathSync(ancestor) + path.slice(ancestor.length)).href; }
    catch { if (dirname(ancestor) === ancestor) return uri; }
  }
}

// The Rust diagnostic columns count Unicode scalars, whereas LSP counts UTF-16
// code units. Use retained snapshot lines, never a potentially newer disk file.
function position(label, line, column) {
  const text = label.source?.find(item => item.number === line)?.text ?? '';
  return { line: Math.max(0, line - 1), character: [...text].slice(0, Math.max(0, column - 1)).join('').length };
}

function location(label, uris) {
  const r = label.range;
  const uri = pathToFileURL(resolve(r.path)).href;
  return { uri: uris.get(uri) ?? uri, range: {
    start: position(label, r.line, r.column),
    end: position(label, r.end_line, r.end_column),
  } };
}

export function diagnosticGroups(diagnostics, uris = new Map()) {
  const groups = new Map();
  for (const diagnostic of diagnostics) {
    const primary = diagnostic.labels?.find(label => label.primary);
    if (!primary) continue;
    const { uri, range } = location(primary, uris);
    const message = [diagnostic.message, ...(diagnostic.notes ?? []), diagnostic.help].filter(Boolean).join('\n\n');
    const item = { range, severity: { error: 1, warning: 2, info: 3 }[diagnostic.severity],
      code: diagnostic.code, source: 'proa', message };
    const relatedInformation = (diagnostic.labels ?? []).filter(label => label !== primary)
      .map(label => ({ location: location(label, uris), message: label.message }));
    if (relatedInformation.length) item.relatedInformation = relatedInformation;
    if (!groups.has(uri)) groups.set(uri, []);
    groups.get(uri).push(item);
  }
  return groups;
}

export class LintWorker {
  constructor(command = process.env.PROA_LINT || 'proa', { timeout = 30000, spawnProcess = spawn } = {}) {
    this.command = command;
    this.timeout = timeout;
    this.spawnProcess = spawnProcess;
    this.sequence = 0;
    this.pending = new Map();
  }

  start() {
    const child = this.spawnProcess(this.command, ['lint', '--stdio'], { stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true });
    this.child = child;
    let stderr = '';
    child.stderr.setEncoding('utf8');
    child.stderr.on('data', text => { stderr = (stderr + text).slice(-4096); });
    const lines = createInterface({ input: child.stdout });
    const fail = error => {
      if (this.child !== child) return;
      this.child = undefined;
      for (const pending of this.pending.values()) { clearTimeout(pending.timer); pending.reject(error); }
      this.pending.clear();
      if (child.exitCode === null) child.kill();
    };
    child.once('error', error => fail(new Error(`Could not start ${this.command} lint --stdio: ${error.message}. Install an updated Proa CLI or configure its executable path.`)));
    child.once('close', code => {
      lines.close();
      fail(new Error(`Proa lint worker exited (${code ?? 'signal'}). ${stderr}`));
    });
    child.stdin.on('error', fail);
    lines.on('line', line => {
      try {
        const response = JSON.parse(line);
        const pending = this.pending.get(response.id);
        if (!pending) return;
        clearTimeout(pending.timer);
        this.pending.delete(response.id);
        if (response.error) pending.reject(Object.assign(new Error(response.error), { fatal: false }));
        else pending.resolve(response.diagnostics);
      } catch (error) { fail(new Error(`Invalid Proa lint response: ${error.message}`)); child.kill(); }
    });
  }

  analyze(overlays) {
    if (!this.child) this.start();
    const id = ++this.sequence;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(Object.assign(new Error('Proa live lint timed out; the next edit will restart the worker.'), { fatal: false }));
        this.dispose();
      }, this.timeout);
      this.pending.set(id, { resolve, reject, timer });
      this.child.stdin.write(JSON.stringify({ id, overlays }) + '\n');
    });
  }

  dispose() {
    const child = this.child;
    this.child = undefined;
    for (const pending of this.pending.values()) { clearTimeout(pending.timer); pending.reject(new Error('Proa lint stopped.')); }
    this.pending.clear();
    if (child) {
      child.stdin.end();
      child.kill();
      const timer = setTimeout(() => child.kill('SIGKILL'), 1000);
      timer.unref();
      child.once('close', () => clearTimeout(timer));
    }
  }
}

// Install shallow watches only in source-discovery directories. A recursive
// root watch still allocates OS watches for target/node_modules even if their
// events are ignored, so exclusions must happen before traversal/registration.
export class SourceWatcher {
  constructor(root, { changed, onError }) {
    this.root = root;
    this.changed = changed;
    this.onError = onError;
    this.watchers = new Map();
    this.sync();
  }

  sync() {
    if (this.disposed) return;
    const directories = new Set();
    const visit = directory => {
      let entries;
      try { entries = readdirSync(directory, { withFileTypes: true }); }
      catch (error) { if (error.code !== 'ENOENT') this.onError(error); return; }
      directories.add(directory);
      if (!this.watchers.has(directory)) {
        try {
          const watcher = watch(directory, (event, name) => {
            const path = name?.toString();
            if (path && ignored(path)) return;
            if (event === 'rename' || !path) {
              clearTimeout(this.timer);
              this.timer = setTimeout(() => this.sync(), 50);
            }
            if (event === 'rename' || !path || /\.rs$/.test(path) || path === 'Cargo.toml') this.changed();
          });
          watcher.on('error', this.onError);
          this.watchers.set(directory, watcher);
        } catch (error) { if (error.code !== 'ENOENT') this.onError(error); }
      }
      for (const entry of entries) {
        if (entry.isDirectory() && !entry.name.startsWith('.') && !ignored(entry.name)) visit(join(directory, entry.name));
      }
    };
    visit(this.root);
    for (const [directory, watcher] of this.watchers) {
      if (!directories.has(directory)) { watcher.close(); this.watchers.delete(directory); }
    }
  }

  dispose() {
    this.disposed = true;
    clearTimeout(this.timer);
    for (const watcher of this.watchers.values()) watcher.close();
    this.watchers.clear();
  }
}

// Shared by VS Code and the standalone bridge. Only one request is in flight;
// edits invalidate its entire snapshot, including diagnostics in other files.
export class LiveLint {
  constructor({ publish, onError = () => {}, command, delay = 250, worker, watchFiles = true }) {
    this.publish = publish;
    this.onError = onError;
    this.worker = worker ?? new LintWorker(command);
    this.delay = delay;
    this.watchFiles = watchFiles;
    this.documents = new Map();
    this.watchers = new Map();
    this.published = new Set();
    this.generation = 0;
  }

  fromClient({ method, params }) {
    if (this.disposed) return;
    const uri = params?.textDocument?.uri;
    if (method === 'textDocument/didOpen') {
      if (!uri?.startsWith('file:') || !uri.endsWith('.rs') || ignored(fileURLToPath(uri))) return;
      this.documents.set(uri, { source: params.textDocument.text, version: params.textDocument.version });
      this.refreshWatchers();
    } else if (method === 'textDocument/didChange') {
      const previous = this.documents.get(uri);
      if (!previous) return;
      this.documents.set(uri, { source: applyChanges(previous.source, params.contentChanges), version: params.textDocument.version });
    } else if (method === 'textDocument/didClose') {
      if (!this.documents.delete(uri)) return;
      this.refreshWatchers();
    } else if (method === 'textDocument/didSave' || method === 'workspace/didChangeWatchedFiles'
      || method === 'workspace/didCreateFiles' || method === 'workspace/didRenameFiles' || method === 'workspace/didDeleteFiles') {
      // Disk is reread for every snapshot; all still-open buffers take priority.
    } else return;
    this.changed();
  }

  refreshWatchers() {
    if (!this.watchFiles) return;
    const roots = new Set([...this.documents.keys()].map(uri => packageRoot(fileURLToPath(uri))));
    for (const [root, watcher] of this.watchers) {
      if (!roots.has(root)) { watcher.dispose(); this.watchers.delete(root); }
    }
    for (const root of roots) {
      if (this.watchers.has(root)) continue;
      this.watchers.set(root, new SourceWatcher(root, {
        changed: () => this.changed(),
        onError: error => this.onError(new Error(`Proa file watcher: ${error.message}`)),
      }));
    }
  }

  changed() {
    if (this.disposed) return;
    this.generation++;
    clearTimeout(this.timer);
    if (!this.documents.size) {
      for (const uri of this.published) this.publish({ uri, diagnostics: [] });
      this.published.clear();
      return;
    }
    if (!this.failed) this.timer = setTimeout(() => void this.flush(), this.delay);
  }

  async flush() {
    if (this.disposed || this.failed || this.running || !this.documents.size) return;
    clearTimeout(this.timer);
    const generation = this.generation;
    const versions = new Map([...this.documents].map(([uri, doc]) => [uri, doc.version]));
    const uris = new Map([...this.documents.keys()].map(uri => [canonicalURI(uri), uri]));
    const roots = [...this.documents.keys()].map(uri => {
      const root = packageRoot(fileURLToPath(uri));
      return { root, canonical: fileURLToPath(canonicalURI(pathToFileURL(root).href)) };
    }).sort((a, b) => b.canonical.length - a.canonical.length);
    const overlays = [...this.documents].map(([uri, doc]) => ({ path: fileURLToPath(uri), source: doc.source }));
    this.running = true;
    try {
      const diagnostics = await this.worker.analyze(overlays);
      if (this.disposed || generation !== this.generation) return;
      // Counterparts can be closed. Preserve the workspace's authored path
      // through symlinked package roots for those URIs too, not only buffers.
      for (const diagnostic of diagnostics) for (const label of diagnostic.labels ?? []) {
        const path = resolve(label.range.path);
        const uri = pathToFileURL(path).href;
        if (uris.has(uri)) continue;
        const alias = roots.find(({ canonical }) => path.startsWith(canonical.endsWith(sep) ? canonical : canonical + sep));
        if (alias) uris.set(uri, pathToFileURL(alias.root + path.slice(alias.canonical.length)).href);
      }
      const groups = diagnosticGroups(diagnostics, uris);
      for (const uri of new Set([...this.published, ...groups.keys()])) {
        this.publish({ uri, ...(versions.has(uri) ? { version: versions.get(uri) } : {}), diagnostics: groups.get(uri) ?? [] });
      }
      this.published = new Set(groups.keys());
    } catch (error) {
      if (!this.disposed) {
        // A file disappearing during discovery can invalidate this snapshot;
        // retry on the next edit. Missing/old executables stop until restart.
        this.failed = error.fatal !== false;
        for (const uri of this.published) this.publish({ uri, diagnostics: [] });
        this.published.clear();
        this.onError(error);
      }
    } finally {
      this.running = false;
      if (!this.disposed && !this.failed && generation !== this.generation && this.documents.size) {
        clearTimeout(this.timer);
        this.timer = setTimeout(() => void this.flush(), this.delay);
      }
    }
  }

  dispose() {
    this.disposed = true;
    clearTimeout(this.timer);
    for (const watcher of this.watchers.values()) watcher.dispose();
    this.watchers.clear();
    this.worker.dispose();
    for (const uri of this.published) this.publish({ uri, diagnostics: [] });
    this.published.clear();
    this.documents.clear();
  }
}
