#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Verify the explicit hot-static fixture's HTTP updates and full-build fallbacks."""
import argparse
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import threading
import time
import urllib.request


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fixture', type=Path, required=True)
    parser.add_argument('--cli', type=Path, required=True)
    args = parser.parse_args()
    root = args.fixture.resolve()
    source = root / 'src/page_0.rs'
    original_source = source.read_text()
    # Constant inline text attributes remain dynamic boundaries in the
    # manifest. Exercise their escaping and the following static slots.
    original = original_source.replace(
        'class="original"', 'class="original" title={text!("fixed \\\" &")}', 1)
    assert original != original_source, 'fixture needs class="original"'
    original = original.replace('</h1>',
        '</h1><aside>{self.title}"GAP_LEFT_A"{self.value}""{self.title}</aside>', 1)
    config_path = root / 'proa.config.json'
    original_config = config_path.read_bytes() if config_path.exists() else None
    config = json.loads(original_config) if original_config else {}
    config.setdefault('tasks', {})['probe_hot_generation'] = {'command': [sys.executable, '-c',
        'from pathlib import Path; p=Path("src/page_0.rs"); s=p.read_text(); '
        'n=s.replace("HOOK_PENDING_A", "EDIT_MARKER_A").replace("HOOK_PENDING_B", "HOOK_PUBLISHED"); '
        'p.write_text(n) if n != s else None']}
    config.setdefault('dev', {}).setdefault('before', []).append('probe_hot_generation')
    config_path.write_text(json.dumps(config, indent=2) + '\n')
    source.write_text(original.replace('EDIT_MARKER_A', 'HOOK_PENDING_A'))
    environment = dict(os.environ, CARGO_TARGET_DIR=str(root / 'target'))
    for key in ['RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER']:
        environment.pop(key, None)
    command = [str(args.cli.resolve()), 'dev', '--root', str(root), '--hot-static', '--no-assets', '--port', '39149',
               '--debounce-ms', '100', '--', 'cargo', 'run', '--offline', '--profile', 'nodebug']
    events = []
    process = subprocess.Popen(command, cwd=root, env=environment, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                               text=True, start_new_session=True)
    def collect(stream):
        for line in stream:
            events.append({'time': time.perf_counter(), 'line': line.rstrip()})
    for stream in [process.stdout, process.stderr]:
        threading.Thread(target=collect, args=(stream,), daemon=True).start()
    def response():
        try:
            with urllib.request.urlopen('http://127.0.0.1:39149/page/0', timeout=.3) as request:
                return request.read().decode()
        except OSError:
            return ''
    def await_condition(condition, timeout=180):
        deadline = time.perf_counter() + timeout
        while time.perf_counter() < deadline:
            if condition():
                return
            if process.poll() is not None:
                raise RuntimeError('dev watcher exited')
            time.sleep(.02)
        raise RuntimeError('acceptance condition timed out')
    checks = []
    try:
        await_condition(lambda: 'EDIT_MARKER_A' in response())
        assert 'dynamic &lt;input&gt;' in response()
        checks.append({'case': 'initial manifest follows generated Rust from before hook', 'passed': True})
        html = response()
        assert 'class="original" title="fixed &quot; &amp;"' in html
        assert html.count('<article data-card>') == 8
        assert '<h1>EDIT_MARKER_A</h1>' in html and '</article></main>' in html
        checks.append({'case': 'constant text attribute preserves escaped HTML and static slots', 'passed': True})
        for label, source_text, expected in [
            ('text and escaping', original.replace('EDIT_MARKER_A', 'EDIT_MARKER_B <&'), 'EDIT_MARKER_B &lt;&amp;'),
            ('static attribute', original.replace('EDIT_MARKER_A', 'EDIT_MARKER_C').replace('class="original"', 'class="updated"'), 'class="updated"'),
        ]:
            start = time.perf_counter()
            source.write_text(source_text)
            await_condition(lambda: expected in response())
            html = response()
            assert 'title="fixed &quot; &amp;"' in html
            assert html.count('<article data-card>') == 8
            assert '</article></main>' in html
            elapsed = time.perf_counter() - start
            relevant = [event for event in events if event['time'] >= start]
            assert not any('running linked RSJS Cargo dev build:' in event['line'] for event in relevant)
            assert not any('starting final linked server' in event['line'] for event in relevant)
            assert any('published without Cargo or restart' in event['line'] for event in relevant)
            checks.append({'case': label, 'seconds': elapsed, 'cargo': False, 'restart': False})
        # A changed Rust expression requires compilation and changes the number.
        start = time.perf_counter()
        dynamic = original.replace('EDIT_MARKER_A', 'EDIT_MARKER_D').replace('self.value + 0', 'self.value + 100')
        source.write_text(dynamic)
        await_condition(lambda: 'EDIT_MARKER_D' in response() and '<span>100</span>' in response())
        assert any(event['time'] >= start and 'running linked RSJS Cargo dev build:' in event['line'] for event in events)
        checks.append({'case': 'dynamic expression fallback', 'seconds': time.perf_counter()-start, 'cargo': True})
        # A compilation failure must retain the previously validated process
        # and its independent static manifest, including its latest output.
        start = time.perf_counter()
        source.write_text(dynamic.replace('self.value + 100', 'self.does_not_exist').replace('EDIT_MARKER_D', 'INVALID_BUILD'))
        await_condition(lambda: any(event['time'] >= start and 'keeping the last valid server' in event['line'] for event in events))
        assert 'EDIT_MARKER_D' in response() and 'INVALID_BUILD' not in response()
        checks.append({'case': 'failed build retains prior HTTP output', 'passed': True})
        start = time.perf_counter()
        source.write_text(dynamic)
        await_condition(lambda: any(event['time'] >= start and ('linked dev rebuild succeeded;' in event['line'] or 'published without Cargo or restart' in event['line']) for event in events))
        assert '<span>100</span>' in response()
        checks.append({'case': 'repair after failure', 'passed': True})
        source.write_text(original.replace('EDIT_MARKER_A', 'HOOK_PENDING_B').replace('self.value + 0', 'self.value + 101'))
        await_condition(lambda: 'HOOK_PUBLISHED' in response() and '<span>101</span>' in response())
        checks.append({'case': 'fallback manifest follows generated Rust from before hook', 'passed': True})
        moved = original.replace('"GAP_LEFT_A"', '""').replace(
            '{self.value}""', '{self.value}"GAP_RIGHT_B"')
        for label, source_text, expected in [
            ('same-count static slot movement fallback', moved,
             '<aside>dynamic &lt;input&gt;0GAP_RIGHT_Bdynamic &lt;input&gt;</aside>'),
            ('static slot deletion fallback', moved.replace('"GAP_RIGHT_B"', '""'),
             '<aside>dynamic &lt;input&gt;0dynamic &lt;input&gt;</aside>'),
        ]:
            start = time.perf_counter()
            source.write_text(source_text)
            await_condition(lambda: expected in response())
            assert any(event['time'] >= start and 'running linked RSJS Cargo dev build:' in event['line'] for event in events)
            checks.append({'case': label, 'passed': True, 'cargo': True})
        print(json.dumps(checks, indent=2))
    finally:
        if process.poll() is None:
            os.killpg(process.pid, signal.SIGTERM)
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait()
        source.write_text(original_source)
        if original_config is None:
            config_path.unlink(missing_ok=True)
        else:
            config_path.write_bytes(original_config)
        output = root / 'target/probe-logs/hot-acceptance.json'
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(json.dumps({'command':command, 'checks':checks, 'events':events},indent=2)+'\n')


if __name__ == '__main__':
    main()
