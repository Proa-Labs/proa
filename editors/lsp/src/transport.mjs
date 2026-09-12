// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// LSP framing counts UTF-8 bytes, independently of UTF-16 document positions.
const maxBody = 64 * 1024 * 1024;
const maxHeader = 8192;

export function encode(message) {
  const body = Buffer.from(JSON.stringify(message), 'utf8');
  return Buffer.concat([Buffer.from(`Content-Length: ${body.length}\r\n\r\n`), body]);
}

export class Decoder {
  buffer = Buffer.alloc(0);
  length = null;
  constructor(onMessage) { this.onMessage = onMessage; }

  push(chunk) {
    this.buffer = Buffer.concat([this.buffer, chunk]);
    for (;;) {
      if (this.length === null) {
        const end = this.buffer.indexOf('\r\n\r\n');
        if (end < 0) {
          if (this.buffer.length > maxHeader) throw new Error('LSP header is too large.');
          return;
        }
        if (end > maxHeader) throw new Error('LSP header is too large.');
        const lengths = this.buffer.subarray(0, end).toString('ascii').split('\r\n')
          .filter(line => /^content-length:/i.test(line));
        if (lengths.length !== 1 || !/^content-length:\s*\d+\s*$/i.test(lengths[0])) {
          throw new Error('Expected one valid LSP Content-Length header.');
        }
        this.length = Number(lengths[0].split(':')[1].trim());
        if (!Number.isSafeInteger(this.length) || this.length > maxBody) throw new Error('LSP message is too large.');
        this.buffer = this.buffer.subarray(end + 4);
      }
      if (this.buffer.length < this.length) return;
      const message = JSON.parse(this.buffer.subarray(0, this.length).toString('utf8'));
      this.buffer = this.buffer.subarray(this.length);
      this.length = null;
      if (!message || typeof message !== 'object' || Array.isArray(message) || message.jsonrpc !== '2.0') {
        throw new Error('Expected an LSP JSON-RPC 2.0 object.');
      }
      this.onMessage(message);
    }
  }

  end() {
    if (this.buffer.length || this.length !== null) throw new Error('Incomplete LSP message at end of stream.');
  }
}
