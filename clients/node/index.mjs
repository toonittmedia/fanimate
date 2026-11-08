// Drive Fanimate from Node (or TypeScript, with Node 22.6 or later).
//
// Two ways in, one protocol (JSON-RPC 2.0, one request per line):
//
//   import { Host } from 'fanimate';
//   const host = await Host.start({ save: 'title.fnm' });   // starts `fanimate-cli script`
//   const title = await host.add_text_layer({ content: 'Hello', x: 100, y: 80 });
//   await host.save();
//   await host.close();
//
//   import { plugin } from 'fanimate';
//   const api = plugin();   // inside a plugin Fanimate started: talks over our own stdin and stdout
//   await api.add_layer({ kind: 'vector', name: 'Art' });
//   api.close();
//
// Every Fanimate call is a method with one object of parameters. A call that fails
// rejects with an Error whose `code` is one of the API's stable strings (not_found,
// invalid_argument, playing, io_error, unauthorized, permission_denied). Nothing
// here depends on a package: it is the standard library only.

import { spawn } from 'node:child_process';
import readline from 'node:readline';

export const version = '0.1.0';

export class FanimateError extends Error {
  constructor(code, message, rpcCode) {
    super(`${code}: ${message}`);
    this.name = 'FanimateError';
    this.code = code;
    this.detail = message;
    this.rpcCode = rpcCode;
  }
}

/** The fanimate-cli to run: the one given, $FANIMATE_CLI, or the one on the path. */
export function findCli(cli) {
  return cli || process.env.FANIMATE_CLI || 'fanimate-cli';
}

class Connection {
  #input;
  #write;
  #lines;
  #next = 0;
  #queue = Promise.resolve();
  #closed = false;

  constructor(input, write) {
    this.#input = input;
    this.#write = write;
    this.#lines = readline.createInterface({ input })[Symbol.asyncIterator]();
  }

  /** Run one method and resolve with its result. Calls are sent one at a time, in order. */
  call(method, params = {}) {
    const run = async () => {
      if (this.#closed) throw new FanimateError('io_error', 'the connection is closed');
      const id = ++this.#next;
      this.#write(JSON.stringify({ jsonrpc: '2.0', id, method, params }) + '\n');
      const { value, done } = await this.#lines.next();
      if (done || value === undefined) throw new FanimateError('io_error', 'Fanimate ended without answering');
      let answer;
      try {
        answer = JSON.parse(value);
      } catch (e) {
        throw new FanimateError('io_error', `Fanimate sent something that is not JSON: ${e.message}`);
      }
      if (answer.id !== id) {
        throw new FanimateError('invalid_argument', `the answer was for request ${answer.id}, not ${id}`);
      }
      if (answer.error) {
        const data = answer.error.data || {};
        throw new FanimateError(data.code || 'invalid_argument', answer.error.message || '', answer.error.code);
      }
      return answer.result;
    };
    const result = this.#queue.then(run, run);
    this.#queue = result.catch(() => {});
    return result;
  }

  /** Send a request that has no answer. */
  notify(method, params = {}) {
    this.#write(JSON.stringify({ jsonrpc: '2.0', method, params }) + '\n');
  }

  close() {
    this.#closed = true;
    this.#lines.return?.();
    this.#input.destroy?.();
  }
}

// host.add_layer({ ... }) is host.call('add_layer', { ... })
function withMethods(target) {
  return new Proxy(target, {
    get(t, name) {
      if (name in t || typeof name === 'symbol' || name === 'then') {
        const value = Reflect.get(t, name);
        // a method runs on the target itself: private fields cannot be reached through the proxy
        return typeof value === 'function' ? value.bind(t) : value;
      }
      return (params = {}) => t.call(name, params);
    },
  });
}

export class Host {
  #connection;
  #process;

  constructor(connection, child) {
    this.#connection = connection;
    this.#process = child;
    return withMethods(this);
  }

  /**
   * Start `fanimate-cli script`. `project` opens a file; `save` is where save() writes;
   * `permissions` (['read', 'edit']) or `plugin` (a plugin.json) limit what it will do.
   */
  static async start({ project, save, cli, permissions, plugin } = {}) {
    const args = ['script'];
    if (project) args.push(project);
    if (save) args.push('--save', save);
    if (permissions) args.push('--permissions', permissions.join(','));
    if (plugin) args.push('--plugin', plugin);
    const child = spawn(findCli(cli), args, { stdio: ['pipe', 'pipe', 'inherit'] });
    await new Promise((resolve, reject) => {
      child.once('spawn', resolve);
      child.once('error', (e) =>
        reject(new FanimateError('not_found', `could not start ${findCli(cli)}: ${e.message}`)),
      );
    });
    child.stdin.on('error', () => {});
    return new Host(new Connection(child.stdout, (text) => child.stdin.write(text)), child);
  }

  call(method, params = {}) {
    return this.#connection.call(method, params);
  }

  notify(method, params = {}) {
    this.#connection.notify(method, params);
  }

  version() {
    return this.call('api.version');
  }

  methods() {
    return this.call('api.methods');
  }

  /** End the host and wait for it to exit. */
  async close() {
    this.#connection.close();
    this.#process.stdin.end();
    await new Promise((resolve) => {
      if (this.#process.exitCode !== null) return resolve();
      this.#process.once('exit', resolve);
      setTimeout(() => {
        this.#process.kill();
        resolve();
      }, 5000).unref();
    });
  }
}

/**
 * The API for a plugin: Fanimate started this program and answers over its standard
 * input and output. Anything written to standard output that is not a request breaks
 * the conversation, so print to standard error (console.error).
 */
export function plugin() {
  const connection = new Connection(process.stdin, (text) => process.stdout.write(text));
  const api = {
    call: (method, params) => connection.call(method, params),
    notify: (method, params) => connection.notify(method, params),
    close: () => connection.close(),
  };
  return withMethods(api);
}
