// frappe-lsp for pi: Frappe's structure as tools, and its checks after edits.
//
// Frappe connects its parts with strings — DocType names, fieldnames, hook
// paths — so an agent editing Frappe code cannot see when one is wrong. This
// runs frappe-lsp beside the session, gives the agent the schema and the
// references as tools, and reports frappe-lsp's findings on every file the
// agent edits inside a bench.

import { existsSync } from "node:fs";
import { homedir } from "node:os";
import { delimiter, dirname, isAbsolute, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { Type } from "@earendil-works/pi-ai";
import { defineTool, type ExtensionAPI } from "@earendil-works/pi-coding-agent";

import { type Diagnostic, FrappeLsp, findBench, languageOf } from "./client.ts";

/** frappe-lsp: $FRAPPE_LSP, else on PATH, else where :FrappeLspInstall puts it. */
function binary(): string | undefined {
	if (process.env.FRAPPE_LSP) return process.env.FRAPPE_LSP;
	for (const dir of (process.env.PATH ?? "").split(delimiter)) {
		if (dir && existsSync(join(dir, "frappe-lsp"))) return join(dir, "frappe-lsp");
	}
	const data = process.env.XDG_DATA_HOME ?? join(homedir(), ".local", "share");
	const installed = join(data, "nvim", "frappe-lsp", "frappe-lsp");
	return existsSync(installed) ? installed : undefined;
}

const INSTALL_HINT =
	"frappe-lsp is not installed: download a release from " +
	"https://github.com/ssiyad/frappe-language-server/releases and put it on PATH, " +
	"or set FRAPPE_LSP to its path.";

/** Fieldtypes that lay a form out and hold no value. */
const LAYOUT = new Set(["Section Break", "Column Break", "Tab Break", "HTML", "Button", "Fold", "Heading"]);

export default function (pi: ExtensionAPI) {
	const servers = new Map<string, FrappeLsp>();

	/** The started server for the bench holding `path`. */
	async function serverFor(path: string): Promise<FrappeLsp> {
		const bench = findBench(path);
		if (!bench) throw new Error(`${path} is not inside a Frappe bench (no sites/apps.txt above it)`);
		let server = servers.get(bench);
		if (!server) {
			const command = binary();
			if (!command) throw new Error(INSTALL_HINT);
			server = new FrappeLsp(command, bench);
			servers.set(bench, server);
		}
		await server.start();
		return server;
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
			}),
			annotations: { readOnlyHint: true, openWorldHint: false },
			async execute(_id, params, _signal, _update, ctx) {
				const server = await serverFor(ctx.cwd);
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
			}),
			annotations: { readOnlyHint: true, openWorldHint: false },
			async execute(_id, params, _signal, _update, ctx) {
				const server = await serverFor(ctx.cwd);
				let found = (await server.request("frappe-lsp/references", params)) as any;
				// The table is built just after the index; give it the time it takes.
				for (let tries = 0; !found.ready && tries < 150; tries++) {
					await new Promise((done) => setTimeout(done, 200));
					found = (await server.request("frappe-lsp/references", params)) as any;
				}
				if (!found.ready) return text("frappe-lsp is still building its reference table; try again shortly.");
				const what = params.fieldname ? `${params.doctype}.${params.fieldname}` : params.doctype;
				if (found.total === 0) return text(`Nothing in this bench names ${what}.`, found);
				const lines = found.sites.map((site: any) => `${shown(server.bench, site.path)}:${site.line}  ${site.text}`);
				const more = found.total > found.sites.length ? `\n(${found.total - found.sites.length} more not shown)` : "";
				return text(`${found.total} references to ${what}:\n${lines.join("\n")}${more}`, found);
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
			parameters: Type.Object({ query: Type.String({ description: "Part of the name, e.g. \"sales inv\"" }) }),
			annotations: { readOnlyHint: true, openWorldHint: false },
			async execute(_id, params, _signal, _update, ctx) {
				const server = await serverFor(ctx.cwd);
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
				"Run frappe-lsp's checks on a file in the bench: unknown DocTypes and fields, handlers that " +
				"never fire, hooks and patches that resolve nowhere, translation mistakes. Files the agent edits " +
				"are checked automatically; use this for other files.",
			parameters: Type.Object({ path: Type.String({ description: "The file, absolute or relative to the working directory" }) }),
			annotations: { readOnlyHint: true, openWorldHint: false },
			async execute(_id, params, _signal, _update, ctx) {
				const path = isAbsolute(params.path) ? params.path : resolve(ctx.cwd, params.path);
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

	// Start the server as the session opens in a bench, so the index is warm by
	// the first question rather than built during it.
	pi.on("session_start", async (_event, ctx) => {
		if (findBench(ctx.cwd) && binary()) serverFor(ctx.cwd).catch(() => undefined);
	});

	// Every file the agent edits in a bench is checked. Quiet when clean: a
	// line per edit saying nothing is wrong would be noise the model pays for.
	pi.on("tool_result", async (event, ctx) => {
		if ((event.toolName !== "edit" && event.toolName !== "write") || event.isError) return;
		const written = event.input.path;
		if (typeof written !== "string") return;
		const path = isAbsolute(written) ? written : resolve(ctx.cwd, written);
		if (!languageOf(path) || !findBench(path) || !binary()) return;
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
