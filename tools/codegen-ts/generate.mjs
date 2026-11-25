import { mkdir, readdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";
import { compile } from "json-schema-to-typescript";

const BANNER = [
  "/* eslint-disable */",
  "/**",
  " * AUTO-GENERATED FILE — DO NOT EDIT BY HAND.",
  " * Source: contracts/v2/schemas/*.schema.json (exported from Rust via schemars).",
  " * Regenerate with: `just codegen-contracts`.",
  " */",
].join("\n");

export function compareCodePoint(a, b) {
  return a < b ? -1 : a > b ? 1 : 0;
}

export function canonicalize(value) {
  if (Array.isArray(value)) {
    return value.map(canonicalize);
  }
  if (value !== null && typeof value === "object") {
    const out = {};
    for (const key of Object.keys(value).sort(compareCodePoint)) {
      out[key] = canonicalize(value[key]);
    }
    return out;
  }
  return value;
}

export function stripDocMetadata(schema) {
  if (Array.isArray(schema)) {
    return schema.map(stripDocMetadata);
  }
  if (schema !== null && typeof schema === "object") {
    const out = {};
    for (const key of Object.keys(schema)) {
      const isDocString =
        (key === "title" || key === "description") && typeof schema[key] === "string";
      if (!isDocString) {
        out[key] = stripDocMetadata(schema[key]);
      }
    }
    return out;
  }
  return schema;
}

export function cleanRefSiblings(schema) {
  if (Array.isArray(schema)) {
    return schema.map(cleanRefSiblings);
  }
  if (schema !== null && typeof schema === "object") {
    if (typeof schema.$ref === "string") {
      const out = { $ref: schema.$ref };
      if (typeof schema.description === "string") {
        out.description = schema.description;
      }
      return out;
    }
    const out = {};
    for (const key of Object.keys(schema).sort(compareCodePoint)) {
      out[key] = cleanRefSiblings(schema[key]);
    }
    return out;
  }
  return schema;
}

export function mergeDeterministic(existing, incoming, name, originExisting, originIncoming, pathTrail = "") {
  if (typeof existing !== typeof incoming) {
    throw new Error(
      `Conflicting JSON Schema definition for '${name}' at '${pathTrail || name}': type mismatch (${typeof existing} vs ${typeof incoming}) between ${originExisting} and ${originIncoming}.`,
    );
  }

  if (Array.isArray(existing)) {
    if (!Array.isArray(incoming) || existing.length !== incoming.length) {
      throw new Error(
        `Conflicting JSON Schema definition for '${name}' at '${pathTrail || name}': array mismatch between ${originExisting} and ${originIncoming}.`,
      );
    }
    return existing.map((item, idx) =>
      mergeDeterministic(item, incoming[idx], name, originExisting, originIncoming, `${pathTrail}[${idx}]`),
    );
  }

  if (existing !== null && typeof existing === "object") {
    const strippedExisting = JSON.stringify(canonicalize(stripDocMetadata(existing)));
    const strippedIncoming = JSON.stringify(canonicalize(stripDocMetadata(incoming)));
    if (strippedExisting !== strippedIncoming) {
      throw new Error(
        `Conflicting JSON Schema definition for '${name}' at '${pathTrail || name}': structural mismatch between ${originExisting} and ${originIncoming}.`,
      );
    }

    const merged = {};
    const allKeys = new Set([...Object.keys(existing), ...Object.keys(incoming)]);
    for (const key of Array.from(allKeys).sort(compareCodePoint)) {
      const isTitleMeta =
        key === "title" &&
        (typeof existing.title === "string" || typeof incoming.title === "string");
      const isDescMeta =
        key === "description" &&
        (typeof existing.description === "string" || typeof incoming.description === "string");
      if (isTitleMeta) {
        if (typeof existing.title === "string" && typeof incoming.title === "string") {
          if (existing.title !== incoming.title) {
            throw new Error(
              `Conflicting JSON Schema definition for '${name}' at '${pathTrail || name}': conflicting title ('${existing.title}' vs '${incoming.title}') between ${originExisting} and ${originIncoming}.`,
            );
          }
          merged.title = existing.title;
        } else {
          merged.title = existing.title || incoming.title;
        }
      } else if (isDescMeta) {
        if (typeof existing.description === "string" && typeof incoming.description === "string") {
          if (existing.description !== incoming.description) {
            // Never pick "the longest one" silently: two different doc comments for the
            // same shared type mean the Rust sources disagree about the contract.
            throw new Error(
              `Conflicting JSON Schema definition for '${name}' at '${pathTrail || name}': conflicting description between ${originExisting} and ${originIncoming}.\n` +
                `  ${originExisting}: ${JSON.stringify(existing.description)}\n` +
                `  ${originIncoming}: ${JSON.stringify(incoming.description)}\n` +
                `Align the Rust doc comments so every occurrence of '${name}' carries the same description, or drop the doc comment from all but one occurrence.`,
            );
          }
          merged.description = existing.description;
        } else {
          merged.description = existing.description || incoming.description;
        }
      } else if (Object.hasOwn(existing, key) && Object.hasOwn(incoming, key)) {
        merged[key] = mergeDeterministic(
          existing[key],
          incoming[key],
          name,
          originExisting,
          originIncoming,
          pathTrail ? `${pathTrail}.${key}` : key,
        );
      } else if (Object.hasOwn(existing, key)) {
        merged[key] = canonicalize(existing[key]);
      } else {
        merged[key] = canonicalize(incoming[key]);
      }
    }
    return canonicalize(merged);
  }

  if (existing !== incoming) {
    throw new Error(
      `Conflicting JSON Schema definition for '${name}' at '${pathTrail || name}': literal mismatch (${existing} vs ${incoming}) between ${originExisting} and ${originIncoming}.`,
    );
  }

  return existing;
}

export function registerDef(defs, origins, name, schema, origin) {
  const normalized = canonicalize(cleanRefSiblings(schema));
  if (Object.hasOwn(defs, name)) {
    const existing = defs[name];
    const originExisting = origins[name] || "earlier schema";
    defs[name] = mergeDeterministic(existing, normalized, name, originExisting, origin);
    return;
  }
  defs[name] = normalized;
  origins[name] = origin;
}

function escapeRegExp(text) {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function sample(text, limit = 120) {
  const flat = text.replace(/\s+/g, " ").trim();
  return flat.length > limit ? `${flat.slice(0, limit - 1)}…` : flat;
}

/**
 * Names declared at the top level of the compiled `.d.ts`
 * (`export interface X`, `export type X = …`, `export const X`, `export enum X`).
 */
export function collectEmittedTypeNames(compiledTs) {
  const names = new Set();
  for (const match of compiledTs.matchAll(
    /^export\s+(?:interface|type|const|enum|namespace)\s+([A-Za-z_$][\w$]*)/gm,
  )) {
    names.add(match[1]);
  }
  return names;
}

function collectRefTargets(node, out) {
  if (Array.isArray(node)) {
    for (const item of node) {
      collectRefTargets(item, out);
    }
    return;
  }
  if (node === null || typeof node !== "object") {
    return;
  }
  if (typeof node.$ref === "string") {
    const match = /^#\/\$defs\/(.+)$/.exec(node.$ref);
    if (match) {
      out.add(match[1].replace(/~1/g, "/").replace(/~0/g, "~"));
    }
  }
  for (const [key, value] of Object.entries(node)) {
    if (key === "$defs") {
      // A definition body is walked when (and only when) something references it.
      continue;
    }
    collectRefTargets(value, out);
  }
}

/**
 * Definition names reachable from the bundle root through `$ref` chains.
 * `json-schema-to-typescript` runs with `unreachableDefinitions: false`, so anything
 * outside this set is dropped from the emitted `.d.ts` without a warning.
 */
export function collectReachableDefinitions(bundleSchema) {
  const defs = bundleSchema.$defs ?? {};
  const reachable = new Set();
  const pending = [];
  const roots = new Set();
  collectRefTargets(bundleSchema.properties ?? {}, roots);
  pending.push(...roots);

  while (pending.length > 0) {
    const name = pending.pop();
    if (reachable.has(name) || !Object.hasOwn(defs, name)) {
      continue;
    }
    reachable.add(name);
    const found = new Set();
    collectRefTargets(defs[name], found);
    for (const next of found) {
      if (!reachable.has(next)) {
        pending.push(next);
      }
    }
  }
  return reachable;
}

/**
 * (a) Every `$defs` entry of the input schemas must either appear in the emitted types or be
 * explicitly referenced — otherwise the codegen silently discards contract types.
 */
export function assertDefinitionCoverage({ compiledTs, bundleSchema, defOrigins = {} }) {
  const defs = bundleSchema.$defs ?? {};
  const emitted = collectEmittedTypeNames(compiledTs);
  const reachable = collectReachableDefinitions(bundleSchema);

  const orphans = [];
  const missing = [];
  for (const name of Object.keys(defs).sort(compareCodePoint)) {
    if (emitted.has(name)) {
      continue;
    }
    const origin = defOrigins[name] ?? "unknown schema";
    if (reachable.has(name)) {
      missing.push(
        `  - '${name}' is reachable from the bundle root but was not emitted (defined in ${origin})`,
      );
    } else {
      orphans.push(
        `  - '${name}' is emitted nowhere and referenced nowhere (defined in ${origin})`,
      );
    }
  }

  if (orphans.length === 0 && missing.length === 0) {
    // `emittedTypeCount` also counts the bundle root interface (`AnkiWorkflowContractsV2`).
    return { emittedTypeCount: emitted.size, reachableDefinitionCount: reachable.size };
  }

  throw new Error(
    [
      "JSON Schema -> TypeScript codegen lost contract definitions:",
      ...orphans,
      ...missing,
      "json-schema-to-typescript runs with `unreachableDefinitions: false`, so an unreferenced",
      "definition disappears from the .d.ts without any warning. Fix the schema source: delete the",
      "stale `$defs` entry, or add an explicit `$ref` to it so it is part of the contract.",
    ].join("\n"),
  );
}

/**
 * (b) A duplicated `title` is only harmless when the bodies are byte-identical.
 * Two different definitions claiming the same `title` make `json-schema-to-typescript`
 * invent suffixed types (`CompileMode1`, `QualityMode1`), so it is an error here.
 */
export function assertUniqueTitles({ defs, defOrigins = {} }) {
  const byTitle = new Map();
  for (const name of Object.keys(defs).sort(compareCodePoint)) {
    const title = defs[name]?.title;
    if (typeof title !== "string") {
      continue;
    }
    if (!byTitle.has(title)) {
      byTitle.set(title, []);
    }
    byTitle.get(title).push({
      name,
      origin: defOrigins[name] ?? "unknown schema",
      body: JSON.stringify(canonicalize(stripDocMetadata(defs[name]))),
    });
  }

  const conflicts = [];
  for (const [title, group] of byTitle) {
    if (new Set(group.map((item) => item.name)).size < 2) {
      continue;
    }
    const bodies = new Set(group.map((item) => item.body));
    if (bodies.size === 1) {
      continue;
    }
    conflicts.push(
      `  - title '${title}' is claimed by ${group
        .map((item) => `'${item.name}' (${item.origin})`)
        .join(" and ")} with ${bodies.size} different bodies`,
    );
  }

  if (conflicts.length > 0) {
    throw new Error(
      [
        "Duplicate JSON Schema `title` with different bodies:",
        ...conflicts,
        "Identical bodies may share a title; different bodies must not, because the TypeScript",
        "codegen then emits `Foo`/`Foo1` and consumers silently bind the wrong shape.",
        "Give each definition a distinct title (or drop the redundant one) at the Rust source.",
      ].join("\n"),
    );
  }
}

/**
 * (c) A schema that forbids every extra property and declares none is `Record<string, never>`
 * in TypeScript, not `{}` (which accepts any non-null value).
 */
export function isKeylessObjectSchema(schema) {
  const body = canonicalize(stripDocMetadata(schema));
  const keys = Object.keys(body);
  return (
    keys.length === 2 &&
    body.type === "object" &&
    body.additionalProperties === false
  );
}

export function applyKeylessObjectTypes(compiledTs, defs) {
  let output = compiledTs;
  const rewritten = [];
  for (const name of Object.keys(defs).sort(compareCodePoint)) {
    if (!isKeylessObjectSchema(defs[name])) {
      continue;
    }
    const expected = `export interface ${name} {}`;
    const pattern = new RegExp(`^export interface ${escapeRegExp(name)} \\{\\}$`, "m");
    if (!pattern.test(output)) {
      throw new Error(
        [
          `Expected \`${expected}\` for the keyless object schema '${name}', but the compiled .d.ts has:`,
          `  ${sample(findDeclaration(output, name) ?? "<no declaration>")}`,
          "The schema declares `type: object` + `additionalProperties: false` with no properties,",
          "so the emitted type must be rewritten to `Record<string, never>`.",
        ].join("\n"),
      );
    }
    output = output.replace(pattern, `export type ${name} = Record<string, never>;`);
    rewritten.push(name);
  }
  return { compiledTs: output, rewritten };
}

function findDeclaration(compiledTs, name) {
  const pattern = new RegExp(`^export\\s+(?:interface|type)\\s+${escapeRegExp(name)}\\b.*$`, "m");
  return pattern.exec(compiledTs)?.[0];
}

export async function generateContracts(schemasDir, tsOutDir, options = {}) {
  const { checkOnly = false } = options;
  const entries = (await readdir(schemasDir))
    .filter((name) => name.endsWith(".schema.json"))
    .sort(compareCodePoint);

  if (entries.length === 0) {
    throw new Error(`No *.schema.json files found in ${schemasDir}. Run export-schemas first.`);
  }

  const mergedDefs = {};
  const defOrigins = {};
  const rootProperties = {};
  const rootRequired = [];

  for (const filename of entries) {
    const fullPath = path.join(schemasDir, filename);
    const rawText = await readFile(fullPath, "utf8");
    const parsed = JSON.parse(rawText);

    if (parsed.$defs && typeof parsed.$defs === "object") {
      for (const defName of Object.keys(parsed.$defs).sort(compareCodePoint)) {
        registerDef(
          mergedDefs,
          defOrigins,
          defName,
          parsed.$defs[defName],
          `${filename}#/$defs/${defName}`,
        );
      }
    }

    const rootName = parsed.title;
    if (typeof rootName !== "string" || rootName.length === 0) {
      throw new Error(`Schema ${filename} is missing a non-empty string 'title'.`);
    }

    const rootSchema = { ...parsed };
    delete rootSchema.$schema;
    delete rootSchema.$defs;

    registerDef(mergedDefs, defOrigins, rootName, rootSchema, filename);
    rootProperties[rootName] = { $ref: `#/$defs/${rootName}` };
    rootRequired.push(rootName);
  }

  const sortedDefs = {};
  for (const key of Object.keys(mergedDefs).sort(compareCodePoint)) {
    sortedDefs[key] = mergedDefs[key];
  }

  const sortedProperties = {};
  for (const key of Object.keys(rootProperties).sort(compareCodePoint)) {
    sortedProperties[key] = rootProperties[key];
  }

  const bundleSchema = {
    $schema: "https://json-schema.org/draft/2020-12/schema",
    title: "AnkiWorkflowContractsV2",
    description:
      "Aggregate index of all v2 JSON Schema contracts exported by anki-workflow-mcp.",
    type: "object",
    additionalProperties: false,
    properties: sortedProperties,
    required: rootRequired.sort(compareCodePoint),
    $defs: sortedDefs,
  };

  const compiled = await compile(bundleSchema, "AnkiWorkflowContractsV2", {
    additionalProperties: false,
    bannerComment: BANNER,
    declareExternallyReferenced: true,
    enableConstEnums: false,
    ignoreMinAndMaxItems: false,
    strictIndexSignatures: true,
    unknownAny: true,
    unreachableDefinitions: false,
    style: {
      endOfLine: "lf",
      printWidth: 100,
      semi: true,
      singleQuote: false,
      tabWidth: 2,
      trailingComma: "all",
    },
  });

  const normalizedCompiled = `${compiled.replace(/\r\n/g, "\n").trimEnd()}\n`;

  // Post-compile assertions: the three silent failure modes (orphan `$defs`, duplicated
  // `title`, conflicting `description`) must fail loudly instead of shrinking the contract.
  assertUniqueTitles({ defs: sortedDefs, defOrigins });
  const coverage = assertDefinitionCoverage({
    compiledTs: normalizedCompiled,
    bundleSchema,
    defOrigins,
  });
  const keyless = applyKeylessObjectTypes(normalizedCompiled, sortedDefs);

  const normalizedTs = keyless.compiledTs;
  const tsOutFile = path.join(tsOutDir, "index.d.ts");
  if (checkOnly) {
    let existingTs;
    try {
      existingTs = await readFile(tsOutFile, "utf8");
    } catch {
      throw new Error(`Contract check failed: ${tsOutFile} does not exist. Run generate first.`);
    }
    if (existingTs !== normalizedTs) {
      throw new Error(`Contract check failed: ${tsOutFile} is out of date with ${schemasDir}.`);
    }
  } else {
    await mkdir(tsOutDir, { recursive: true });
    await writeFile(tsOutFile, normalizedTs, "utf8");
  }

  return {
    tsOutFile,
    schemaCount: entries.length,
    defsCount: Object.keys(sortedDefs).length,
    emittedTypeCount: coverage.emittedTypeCount,
    reachableDefinitionCount: coverage.reachableDefinitionCount,
    keylessObjects: keyless.rewritten,
  };
}

async function main() {
  const args = process.argv.slice(2);
  if (args.some((arg) => arg !== "--check")) {
    throw new Error(`Unknown arguments: ${args.join(" ")}. Supported: [--check]`);
  }
  const checkOnly = args.includes("--check");
  const __dirname = path.dirname(fileURLToPath(import.meta.url));
  const repoRoot = path.resolve(__dirname, "..", "..");
  const schemasDir = path.join(repoRoot, "contracts", "v2", "schemas");
  const tsOutDir = path.join(repoRoot, "contracts", "v2", "ts");

  const result = await generateContracts(schemasDir, tsOutDir, { checkOnly });
  const verb = checkOnly ? "verified" : "generated";
  process.stdout.write(
    `${verb} ${path.relative(repoRoot, result.tsOutFile)} from ${result.schemaCount} schemas (${result.defsCount} types)\n`,
  );
}

const isMain =
  process.argv[1] &&
  fileURLToPath(import.meta.url) === path.resolve(process.argv[1]);

if (isMain) {
  main().catch((err) => {
    process.stderr.write(
      `codegen-ts failed: ${err instanceof Error ? err.message : String(err)}\n`,
    );
    process.exit(1);
  });
}
