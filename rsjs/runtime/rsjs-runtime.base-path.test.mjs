// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const load = async (file, prefix) => {
    const source = (await readFile(new URL(file, import.meta.url), 'utf8'))
        .replace('const PROA_BASE_PATH = "";', `const PROA_BASE_PATH = ${JSON.stringify(prefix)};`);
    return import(`data:text/javascript;base64,${Buffer.from(source).toString('base64')}`);
};
globalThis.document = { querySelectorAll: () => [], getElementById: () => null };
const requests = [];
globalThis.fetch = async (url, init) => {
    requests.push({ url, init });
    return { ok: true, status: 200, text: async () => '{"saved":true}' };
};
for (const prefix of ['', '/apps/demo', '/user/alice%40example/proxy/3000']) {
    const core = await load('./rsjs-runtime.js', prefix);
    const { app_url, rsjs, register_query, register_mutation } = await load('./rsjs-runtime-query.js', prefix);
    for (const path of ['/api/items', '//cdn.test/items', 'https://api.test/items', './items', '?page=1', '#item']) {
        const expected = path.startsWith('/') && !path.startsWith('//') ? prefix + path : path;
        assert.equal(app_url(path), expected);
        assert.equal(core.app_url(path), expected);
        assert.equal(app_url(app_url(path)), expected);
    }
    for (const [index, fetcher] of [
        '/api/items',
        { url: '/api/items', method: 'GET' },
        JSON.stringify({ url: '/api/items', method: 'GET' }),
        { url: `${prefix}/api/items`, method: 'GET' },
        'https://api.test/items',
        '//cdn.test/items',
        () => ({ url: '/api/items' }),
    ].entries()) {
        const key = ['base-path', prefix, index];
        const query = rsjs.query(key);
        register_query({ island: `mounted-${index}`, query_id: index, key, fetcher, options: {} });
        await query.refetch();
        const expected = index < 4 || index === 6 ? `${prefix}/api/items` : fetcher;
        assert.equal(requests.at(-1).url, expected);
    }
    const mutation = register_mutation({ island: 'mounted-mutation', mutation_id: 0,
        runner: { url: '/api/save', method: 'POST' }, options: {}, invalidates: [] });
    assert.deepEqual(await mutation.run({ value: 42 }), { saved: true });
    assert.equal(requests.at(-1).url, `${prefix}/api/save`);
    assert.equal(requests.at(-1).init.method, 'POST');
    assert.equal(requests.at(-1).init.body, '{"value":42}');
}
console.log('runtime base-path tests passed');
