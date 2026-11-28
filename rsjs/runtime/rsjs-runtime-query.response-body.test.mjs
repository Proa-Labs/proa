// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import assert from 'node:assert/strict';
import { once } from 'node:events';
import { createServer } from 'node:http';
import test from 'node:test';
import { register_query, register_mutation } from './rsjs-runtime-query.js';

test('the request deadline covers a response stalled after its headers', { timeout: 8_000 }, async (t) => {
    const requests = new Set();
    const server = createServer((req, res) => {
        requests.add(req.method);
        res.writeHead(200);
        res.write('partial');
    });
    server.listen(0, '127.0.0.1');
    await once(server, 'listening');
    t.after(() => {
        server.closeAllConnections();
        return new Promise((resolve) => server.close(resolve));
    });
    const url = `http://127.0.0.1:${server.address().port}/stall`;
    const request = { url, response: 'empty', timeout_ms: 500 };
    const query = register_query({ island: 'read-body', query_id: 0, key: ['stall-q'], fetcher: request });
    const mutation = register_mutation({
        island: 'read-body', mutation_id: 0, key: ['stall-m'], runner: request, invalidates: [],
    });
    await Promise.all([
        assert.rejects(query.refetch(), { name: 'AbortError' }),
        assert.rejects(mutation.run(1), { name: 'AbortError' }),
    ]);
    assert.deepEqual(requests, new Set(['GET', 'POST']));
    assert.equal(query.status(), 'error');
    assert.equal(mutation.status(), 'error');
});

test('empty responses drain chunks and report progress without retaining the payload', async (t) => {
    const response = new Response(new ReadableStream({
        start(controller) {
            controller.enqueue(Uint8Array.of(1, 2));
            controller.enqueue(Uint8Array.of(3));
            controller.close();
        },
    }), { headers: { 'Content-Length': '3' } });
    t.mock.method(response, 'arrayBuffer', () => assert.fail('buffered a streamed response'));
    t.mock.method(globalThis, 'fetch', async () => response);
    const query = register_query({
        island: 'read-body', query_id: 1, key: ['discard-chunks'],
        fetcher: { url: '/discard', response: 'empty' }, options: { download_progress: true },
    });
    assert.equal(await query.refetch(), null);
    assert.equal(query.download_loaded(), 3);
    assert.equal(response.body.locked, false);
});
