// A minimal LSP client for frappe-lsp over stdio.
//
// It speaks only what the pi extension needs: requests, notifications, and
// answers to the few requests the server sends on its own initiative. Node's
// standard library only, so the package has no dependencies to install.

import { spawn, type ChildProcess } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

type Pending = {
	resolve: (value: unknown) => void;
	reject: (error: Error) => void;
	timer: NodeJS.Timeout;
};

export type Diagnostic = {
	range: { start: { line: number; character: number } };
	code?: string;
	message: string;
	severity?: number;
};

/** The bench above `from`: the directory holding `sites/apps.txt`. */
export function findBench(from: string): string | undefined {
	let dir = resolve(from);
	for (;;) {
		if (existsSync(join(dir, "sites", "apps.txt"))) return dir;
		const parent = dirname(dir);
		if (parent === dir) return undefined;
		dir = parent;
	}
}

/** The language a file is opened as, or undefined for one the server does not read. */
export function languageOf(path: string): string | undefined {
	if (path.endsWith(".py")) return "python";
	if (path.endsWith(".js")) return "javascript";
	if (path.endsWith(".json")) return "json";
	if (path.endsWith(".html")) return "html";
	if (path.endsWith("patches.txt")) return "plaintext";
	return undefined;
}

export class FrappeLsp {
	private process: ChildProcess | undefined;
	private buffer = Buffer.alloc(0);
	private nextId = 1;
	private pending = new Map<number, Pending>();
	private ready: Promise<void> | undefined;
	readonly binary: string;
	readonly bench: string;

	constructor(binary: string, bench: string) {
		this.binary = binary;
		this.bench = bench;
	}

	/** Start the server and wait for its index; safe to call repeatedly. */
	start(): Promise<void> {
		this.ready ??= this.boot().catch((error) => {
			this.ready = undefined;
			throw error;
		});
		return this.ready;
	}

	private async boot(): Promise<void> {
		const child = spawn(this.binary, ["--log-level", "warn"], {
			cwd: this.bench,
			stdio: ["pipe", "pipe", "pipe"],
		});
		this.process = child;
		child.stdout!.on("data", (chunk: Buffer) => this.receive(chunk));
		// The tail of stderr says why a start failed, such as a download the
		// launcher could not make.
		let log = "";
		child.stderr!.on("data", (chunk: Buffer) => {
			log = (log + chunk.toString()).slice(-2_000);
		});
		const gone = (why: string) => {
			for (const [, waiting] of this.pending) {
				clearTimeout(waiting.timer);
				waiting.reject(new Error(why));
			}
			this.pending.clear();
			this.process = undefined;
			this.ready = undefined;
		};
		child.on("error", (error) => gone(`frappe-lsp did not start: ${error.message}`));
		child.on("exit", (code) => {
			const why = log.trim().split("\n").pop();
			gone(`frappe-lsp exited (${code})${why ? `: ${why}` : ""}`);
		});

		await this.request("initialize", {
			processId: process.pid,
			clientInfo: { name: "pi" },
			rootUri: pathToFileURL(this.bench).href,
			capabilities: {
				general: { positionEncodings: ["utf-16"] },
				workspace: { configuration: true, symbol: {} },
				textDocument: { diagnostic: {}, hover: { contentFormat: ["markdown", "plaintext"] } },
			},
		});
		this.notify("initialized", {});

		// The index builds in the background; nothing is worth asking before it.
		const deadline = Date.now() + 120_000;
		for (;;) {
			const status = (await this.request("frappe-lsp/indexStatus", null)) as { confidence?: string };
			if (status?.confidence && status.confidence !== "partial") return;
			if (Date.now() > deadline) throw new Error("frappe-lsp: the index did not build within two minutes");
			await new Promise((done) => setTimeout(done, 200));
		}
	}

	request(method: string, params: unknown, timeoutMs = 60_000): Promise<unknown> {
		const child = this.process;
		if (!child?.stdin) return Promise.reject(new Error("frappe-lsp is not running"));
		const id = this.nextId++;
		return new Promise((resolve, reject) => {
			const timer = setTimeout(() => {
				this.pending.delete(id);
				reject(new Error(`frappe-lsp: ${method} timed out`));
			}, timeoutMs);
			this.pending.set(id, { resolve, reject, timer });
			this.send({ jsonrpc: "2.0", id, method, params });
		});
	}

	notify(method: string, params: unknown): void {
		this.send({ jsonrpc: "2.0", method, params });
	}

	/** Diagnostics for a file as it is on disk. */
	async check(path: string): Promise<Diagnostic[]> {
		const languageId = languageOf(path);
		if (!languageId) return [];
		const uri = pathToFileURL(path).href;
		this.notify("textDocument/didOpen", {
			textDocument: { uri, languageId, version: 1, text: readFileSync(path, "utf8") },
		});
		try {
			const report = (await this.request("textDocument/diagnostic", { textDocument: { uri } }, 15_000)) as {
				items?: Diagnostic[];
			};
			return report?.items ?? [];
		} finally {
			this.notify("textDocument/didClose", { textDocument: { uri } });
		}
	}

	stop(): void {
		const child = this.process;
		if (!child) return;
		this.request("shutdown", null, 2_000)
			.catch(() => undefined)
			.finally(() => {
				this.notify("exit", null);
				setTimeout(() => child.kill(), 1_000).unref();
			});
	}

	private send(message: unknown): void {
		const body = Buffer.from(JSON.stringify(message), "utf8");
		this.process?.stdin?.write(`Content-Length: ${body.length}\r\n\r\n`);
		this.process?.stdin?.write(body);
	}

	private receive(chunk: Buffer): void {
		this.buffer = Buffer.concat([this.buffer, chunk]);
		for (;;) {
			const end = this.buffer.indexOf("\r\n\r\n");
			if (end < 0) return;
			const length = Number(/Content-Length: *(\d+)/i.exec(this.buffer.subarray(0, end).toString())?.[1]);
			if (!Number.isFinite(length) || this.buffer.length < end + 4 + length) return;
			const body = this.buffer.subarray(end + 4, end + 4 + length).toString("utf8");
			this.buffer = this.buffer.subarray(end + 4 + length);
			// This runs in a stream handler inside pi: a throw here would end the
			// agent's session over one bad frame. Drop the frame instead; a
			// request it answered times out on its own.
			let message: unknown;
			try {
				message = JSON.parse(body);
			} catch {
				continue;
			}
			this.dispatch(message as Parameters<FrappeLsp["dispatch"]>[0]);
		}
	}

	private dispatch(message: { id?: number; method?: string; params?: any; result?: unknown; error?: any }): void {
		if (message.method !== undefined) {
			// A request from the server: answer as an editor with no settings would.
			if (message.id !== undefined) {
				const items = message.params?.items;
				const result = message.method === "workspace/configuration" ? (items ?? []).map(() => null) : null;
				this.send({ jsonrpc: "2.0", id: message.id, result });
			}
			return;
		}
		const waiting = message.id === undefined ? undefined : this.pending.get(message.id);
		if (!waiting) return;
		this.pending.delete(message.id!);
		clearTimeout(waiting.timer);
		if (message.error) waiting.reject(new Error(message.error.message ?? "frappe-lsp error"));
		else waiting.resolve(message.result);
	}
}
