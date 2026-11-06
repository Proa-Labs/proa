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
    for (const [index, [fetcher, expected]] of [
        ['/api/items', '/api/items'],
        [{ url: '/api/items', method: 'GET' }, '/api/items'],
        [JSON.stringify({ url: '/api/items', method: 'GET' }), '/api/items'],
        [{ url: '/api/items', apply_base_path: true }, `${prefix}/api/items`],
        [JSON.stringify({ url: '/api/items', apply_base_path: true }), `${prefix}/api/items`],
        [{ url: `${prefix}/api/items`, apply_base_path: true }, `${prefix}/api/items`],
        ['https://api.test/items', 'https://api.test/items'],
        ['//cdn.test/items', '//cdn.test/items'],
        [() => ({ url: '/api/items' }), '/api/items'],
        [() => ({ url: '/api/items', apply_base_path: true }), `${prefix}/api/items`],
    ].entries()) {
        const key = ['base-path', prefix, index];
        const query = rsjs.query(key);
        register_query({ island: `mounted-${index}`, query_id: index, key, fetcher, options: {} });
        await query.refetch();
        assert.equal(requests.at(-1).url, expected);
    }
    for (const [index, [runner, expected]] of [
        [{ url: '/api/save', method: 'POST' }, '/api/save'],
        [{ url: '/api/save', method: 'POST', apply_base_path: true }, `${prefix}/api/save`],
        [JSON.stringify({ url: '/api/save', apply_base_path: true }), `${prefix}/api/save`],
    ].entries()) {
        const mutation = register_mutation({ island: 'mounted-mutation', mutation_id: index,
            key: ['mounted-mutation', prefix, index], runner, options: {}, invalidates: [] });
        assert.deepEqual(await mutation.run({ value: 42 }), { saved: true });
        assert.equal(requests.at(-1).url, expected);
        assert.equal(requests.at(-1).init.method, 'POST');
        assert.equal(requests.at(-1).init.body, '{"value":42}');
    }
}
console.log('runtime base-path tests passed');
