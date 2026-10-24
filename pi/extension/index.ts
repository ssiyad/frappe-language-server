// frappe-lsp for pi: Frappe's structure as tools, and its checks after edits.
//
// Frappe connects its parts with strings — DocType names, fieldnames, hook
// paths — so an agent editing Frappe code cannot see when one is wrong. This
// runs frappe-lsp beside the session, gives the agent the schema and the
// references as tools, and reports frappe-lsp's findings on every file the
// agent edits inside a bench.

import { execFile } from "node:child_process";
import { statSync } from "node:fs";
import { dirname, isAbsolute, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { Type } from "@earendil-works/pi-ai";
import { defineTool, type ExtensionAPI } from "@earendil-works/pi-coding-agent";

import { type Diagnostic, FrappeLsp, findBench, languageOf } from "./client.ts";

/**
 * The launcher in this package. It downloads the release the package pins on
 * first use, so pi needs no frappe-lsp of its own; $FRAPPE_LSP overrides it.
 */
const LAUNCHER = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "bin", "frappe-lsp");

/** Fieldtypes that lay a form out and hold no value. */
const LAYOUT = new Set(["Section Break", "Column Break", "Tab Break", "HTML", "Button", "Fold", "Heading"]);

/** Findings a directory check lists before it only counts the rest. */
const SHOWN_FINDINGS = 100;

/** The parameter that names the bench when the session does not run in one. */
const BENCH_PATH = Type.Optional(
	Type.String({ description: "Any path inside the bench. Needed only when the working directory is not in one" }),
);

export default function (pi: ExtensionAPI) {
	const servers = new Map<string, FrappeLsp>();

	/** The started server for the bench holding `path`. */
	async function serverFor(path: string): Promise<FrappeLsp> {
		const bench = findBench(path);
		if (!bench) throw new Error(`${path} is not inside a Frappe bench (no sites/apps.txt above it)`);
		let server = servers.get(bench);
		if (!server) {
			server = new FrappeLsp(LAUNCHER, bench);
			servers.set(bench, server);
		}
		await server.start();
		return server;
	}

	/**
	 * The server for `path`, else for the working directory. A session that
	 * runs outside every bench but has one server running asks that one.
	 */
	async function serverAt(cwd: string, path?: string): Promise<FrappeLsp> {
		if (path) return serverFor(isAbsolute(path) ? path : resolve(cwd, path));
		if (findBench(cwd) || servers.size !== 1) {
			return serverFor(cwd).catch((error: Error) => {
				throw new Error(`${error.message}. Pass \`path\`: any path inside the bench.`);
			});
		}
		const [only] = servers.values();
		await only.start();
		return only;
	}

	/** `frappe-lsp check --json` over a directory, through the launcher. */
	function checkDirectory(bench: string, directory: string): Promise<{ checked: number; findings: any[] }> {
		return new Promise((done, fail) => {
			const args = ["check", "--json", directory];
			// It exits 1 when it finds something; only output that is not JSON is a failure.
			execFile(LAUNCHER, args, { cwd: bench, maxBuffer: 64 * 1024 * 1024 }, (error, stdout, stderr) => {
				try {
					done(JSON.parse(stdout));
				} catch {
					fail(new Error(`frappe-lsp check failed: ${stderr.trim().split("\n").pop() ?? error?.message}`));
				}
			});
		});
	}

	const text = (value: string, structured?: unknown) => ({
		content: [{ type: "text" as const, text: value }],
		details: undefined,
		...(structured === undefined ? {} : { structuredContent: structured as never }),
	});

	const shown = (bench: string, path: string) => (path.startsWith(bench) ? relative(bench, path) : path);

	function findings(bench: string, path: string, diagnostics: Diagnostic[]): string {
		const lines = diagnostics.map((diagnostic) => {
			const { line, character } = diagnostic.range.start;
			const code = (diagnostic.code ?? "").replace(/^frappe-lsp\//, "");
			return `  ${line + 1}:${character + 1} ${code}: ${diagnostic.message}`;
		});
		const count = diagnostics.length === 1 ? "1 problem" : `${diagnostics.length} problems`;
		return `frappe-lsp: ${count} in ${shown(bench, path)}\n${lines.join("\n")}`;
	}

	pi.registerTool(
		defineTool({
			name: "frappe_doctype",
			label: "Frappe DocType",
			description:
				"The schema of a Frappe DocType from the bench's index: every field with its type, label, " +
				"link target and whether it is required; the controller class and file; the DocType JSON; " +
				"child-table parents; and the site's Client/Server Scripts when a live site is configured. " +
				"Use it before writing code that names fields, instead of reading the JSON by hand.",
			parameters: Type.Object({
				doctype: Type.String({ description: "The DocType name, exactly, e.g. \"Sales Invoice\"" }),
				path: BENCH_PATH,
			}),
			annotations: { readOnlyHint: true, openWorldHint: false },
			async execute(_id, params, _signal, _update, ctx) {
				const server = await serverAt(ctx.cwd, params.path);
				const found = (await server.request("frappe-lsp/describe", { doctype: params.doctype })) as any;
				if (!found.found) {
					const similar = found.similar?.length ? ` Did you mean ${found.similar.join(", ")}?` : "";
					return text(`No DocType named "${params.doctype}" in this bench.${similar}`, found);
				}
				const flags = [
					found.isChild && "child table",
					found.isSingle && "single",
					found.isSubmittable && "submittable",
					found.isVirtual && "virtual",
					found.isTree && "tree",
					found.createdThroughTheUi && "created through the UI",
				].filter(Boolean);
				const out = [`${found.name} — ${found.module ?? "?"} · ${found.app}${flags.length ? ` (${flags.join(", ")})` : ""}`];
				if (found.controller) out.push(`controller: ${shown(server.bench, found.controller.path)} (${found.controller.class})`);
				if (found.path) out.push(`json: ${shown(server.bench, found.path)}`);
				if (found.parents?.length) out.push(`child of: ${found.parents.join(", ")}`);
				const fields = found.fields.filter((field: any) => !LAYOUT.has(field.fieldtype));
				out.push(`fields (${fields.length}):`);
				for (const field of fields) {
					const target = field.options && /Link|Table|Select|Dynamic/.test(field.fieldtype)
						? ` → ${String(field.options).split("\n").filter(Boolean).join(" | ")}`
						: "";
					const marks = [field.reqd && "reqd", field.layer !== "schema" && field.layer].filter(Boolean);
					out.push(`  ${field.fieldname}  ${field.fieldtype}${target}${marks.length ? `  [${marks.join(", ")}]` : ""}${field.label ? `  "${field.label}"` : ""}`);
				}
				for (const script of found.scripts ?? []) {
					out.push(`script: ${script.kind} ${script.name}${script.trigger ? ` (${script.trigger})` : ""} → ${script.path}`);
				}
				return text(out.join("\n"), found);
			},
		}),
	);

	pi.registerTool(
		defineTool({
			name: "frappe_references",
			label: "Frappe references",
			description:
				"Every place in the bench that names a DocType, or one of its fields: Python and JavaScript " +
				"calls, DocType JSON (Link options, fetch_from), hooks.py, fixtures and templates. Finds uses " +
				"grep cannot tell apart, such as `customer` on Sales Invoice rather than on Sales Order.",
			parameters: Type.Object({
				doctype: Type.String({ description: "The DocType name" }),
				fieldname: Type.Optional(Type.String({ description: "A field of that DocType; omit for the DocType itself" })),
				path: BENCH_PATH,
			}),
			annotations: { readOnlyHint: true, openWorldHint: false },
			async execute(_id, params, _signal, _update, ctx) {
				const server = await serverAt(ctx.cwd, params.path);
				const asked = { doctype: params.doctype, fieldname: params.fieldname };
				let found = (await server.request("frappe-lsp/references", asked)) as any;
				// The table is built just after the index; give it the time it takes.
				for (let tries = 0; !found.ready && tries < 150; tries++) {
					await new Promise((done) => setTimeout(done, 200));
					found = (await server.request("frappe-lsp/references", asked)) as any;
				}
				if (!found.ready) return text("frappe-lsp is still building its reference table; try again shortly.");
				const what = params.fieldname ? `${params.doctype}.${params.fieldname}` : params.doctype;
				const covers = found.covers ? `\n${found.covers}` : "";
				if (found.total === 0) return text(`Nothing in this bench names ${what}.${covers}`, found);
				const lines = found.sites.map((site: any) => `${shown(server.bench, site.path)}:${site.line}  ${site.text}`);
				const more = found.total > found.sites.length ? `\n(${found.total - found.sites.length} more not shown)` : "";
				return text(`${found.total} references to ${what}:\n${lines.join("\n")}${more}${covers}`, found);
			},
		}),
	);

	pi.registerTool(
		defineTool({
			name: "frappe_find",
			label: "Frappe find",
			description:
				"Find DocTypes, whitelisted methods, reports, pages and web forms in the bench by name " +
				"(fuzzy), with the file each is defined in.",
			parameters: Type.Object({
				query: Type.String({ description: "Part of the name, e.g. \"sales inv\"" }),
				path: BENCH_PATH,
			}),
			annotations: { readOnlyHint: true, openWorldHint: false },
			async execute(_id, params, _signal, _update, ctx) {
				const server = await serverAt(ctx.cwd, params.path);
				const symbols = ((await server.request("workspace/symbol", { query: params.query })) as any[]) ?? [];
				if (symbols.length === 0) return text(`Nothing in this bench matches "${params.query}".`, symbols);
				const lines = symbols.slice(0, 50).map((symbol) => {
					const uri = symbol.location?.uri as string | undefined;
					const path = uri?.startsWith("file:") ? shown(server.bench, fileURLToPath(uri)) : uri ?? "";
					const line = symbol.location?.range ? `:${symbol.location.range.start.line + 1}` : "";
					return `${symbol.name}${symbol.containerName ? ` (${symbol.containerName})` : ""}  ${path}${line}`;
				});
				return text(lines.join("\n"), symbols);
			},
		}),
	);

	pi.registerTool(
		defineTool({
			name: "frappe_check",
			label: "Frappe check",
			description:
				"Run frappe-lsp's checks on a file or a whole directory in the bench: unknown DocTypes and " +
				"fields, handlers that never fire, hooks and patches that resolve nowhere, translation mistakes. " +
				"Files the agent edits are checked automatically; use this to review or audit other code, such " +
				"as an app's api/ folder or the files a diff touches.",
			parameters: Type.Object({
				path: Type.String({ description: "A file or directory, absolute or relative to the working directory" }),
			}),
			annotations: { readOnlyHint: true, openWorldHint: false },
			async execute(_id, params, _signal, _update, ctx) {
				const path = isAbsolute(params.path) ? params.path : resolve(ctx.cwd, params.path);
				if (statSync(path, { throwIfNoEntry: false })?.isDirectory()) return checkTree(path);
				if (!languageOf(path)) return text(`frappe-lsp does not read ${params.path}.`);
				const server = await serverFor(dirname(path));
				const diagnostics = await server.check(path);
				return text(
					diagnostics.length ? findings(server.bench, path, diagnostics) : `frappe-lsp: no problems in ${shown(server.bench, path)}`,
					diagnostics,
				);
			},
		}),
	);

	async function checkTree(directory: string) {
		const bench = findBench(directory);
		if (!bench) throw new Error(`${directory} is not inside a Frappe bench (no sites/apps.txt above it)`);
		const report = await checkDirectory(bench, directory);
		const where = shown(bench, directory);
		if (report.findings.length === 0) return text(`frappe-lsp: no problems in ${report.checked} files under ${where}`, report);
		const lines = report.findings.slice(0, SHOWN_FINDINGS).map((finding) => {
			const code = String(finding.code ?? "").replace(/^frappe-lsp\//, "");
			return `  ${finding.path}:${finding.line}:${finding.column} ${code}: ${finding.message}`;
		});
		const rest = report.findings.length - lines.length;
		const more = rest > 0 ? `\n  (${rest} more; check a narrower directory to see them)` : "";
		const count = `${report.findings.length} problem${report.findings.length === 1 ? "" : "s"}`;
		return text(`frappe-lsp: ${count} in ${report.checked} files under ${where}\n${lines.join("\n")}${more}`, report);
	}

	// Start the server as the session opens in a bench, so the index is warm by
	// the first question rather than built during it.
	pi.on("session_start", async (_event, ctx) => {
		if (findBench(ctx.cwd)) serverFor(ctx.cwd).catch(() => undefined);
	});

	// Every file the agent edits in a bench is checked. Quiet when clean: a
	// line per edit saying nothing is wrong would be noise the model pays for.
	pi.on("tool_result", async (event, ctx) => {
		if ((event.toolName !== "edit" && event.toolName !== "write") || event.isError) return;
		const written = event.input.path;
		if (typeof written !== "string") return;
		const path = isAbsolute(written) ? written : resolve(ctx.cwd, written);
		if (!languageOf(path) || !findBench(path)) return;
		try {
			const server = await serverFor(dirname(path));
			const diagnostics = await server.check(path);
			if (diagnostics.length === 0) return;
			return {
				content: [...event.content, { type: "text" as const, text: findings(server.bench, path, diagnostics) }],
				structuredContent: event.structuredContent,
			};
		} catch {
			// A check that cannot run says nothing; the edit itself succeeded.
			return;
		}
	});

	pi.on("session_shutdown", async () => {
		for (const server of servers.values()) server.stop();
		servers.clear();
	});
}
