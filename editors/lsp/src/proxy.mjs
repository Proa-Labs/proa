// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { spawn } from 'node:child_process';
import { Decoder, encode } from './transport.mjs';
import { Session } from './session.mjs';

export function runProxy({ command, args = [], lintCommand, lintEnabled = true, input = process.stdin, output = process.stdout, log = process.stderr }) {
  return new Promise(resolve => {
    const child = spawn(command, args, { stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true });
    let finished = false, stopping = false, failure = false, timer;
    const write = (stream, message) => {
      if (!finished && !stream.destroyed && !stream.writableEnded) stream.write(encode(message));
    };
    const session = new Session({
      toClient: message => write(output, message),
      toServer: message => write(child.stdin, message),
      lint: lintEnabled ? { command: lintCommand, onError: error => log.write(`proa-lsp: ${error.message}\n`) } : undefined,
    });
    const stop = () => {
      if (stopping || finished) return;
      stopping = true;
      session.dispose();
      child.stdin.end();
      timer = setTimeout(() => child.kill('SIGKILL'), 5000);
      timer.unref();
    };
    const fail = error => {
      if (finished) return;
      failure = true;
      log.write(`proa-lsp: ${error.message}\n`);
      stop();
      child.kill();
    };
    const clientDecoder = new Decoder(message => {
      session.fromClient(message);
      if (message.method === 'exit') stop();
    });
    const serverDecoder = new Decoder(message => session.fromServer(message));
    const clientData = chunk => { try { clientDecoder.push(chunk); } catch (error) { fail(error); } };
    const serverData = chunk => { try { serverDecoder.push(chunk); } catch (error) { fail(error); } };
    const clientEnd = () => {
      try { clientDecoder.end(); stop(); } catch (error) { fail(error); }
    };
    const signal = () => { stop(); child.kill(); };
    const finish = code => {
      if (finished) return;
      finished = true;
      clearTimeout(timer);
      session.dispose();
      input.off('data', clientData).off('end', clientEnd).off('error', fail);
      input.pause();
      output.off('error', fail);
      process.off('SIGINT', signal).off('SIGTERM', signal);
      resolve(failure ? 1 : (code ?? 1));
    };

    input.on('data', clientData).once('end', clientEnd).on('error', fail);
    output.on('error', fail);
    child.stdout.on('data', serverData);
    child.stdout.once('end', () => { try { serverDecoder.end(); } catch (error) { fail(error); } });
    child.stderr.on('data', chunk => log.write(chunk));
    child.stdin.on('error', error => { if (!stopping) fail(error); });
    child.once('error', error => {
      failure = true;
      log.write(`proa-lsp: Could not start ${command}: ${error.message}\nSet --rust-analyzer to a working rust-analyzer executable.\n`);
      finish(1);
    });
    child.once('close', code => {
      if (!stopping) log.write(`proa-lsp: rust-analyzer exited (${code ?? 'signal'}). Restart the editor's LSP client.\n`);
      finish(code);
    });
    process.on('SIGINT', signal).on('SIGTERM', signal);
  });
}
