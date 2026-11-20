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
          if (existing.description === incoming.description) {
            merged.description = existing.description;
          } else {
            // Deterministic description selection: prefer longer description, break tie with code point
            if (existing.description.length > incoming.description.length) {
              merged.description = existing.description;
            } else if (incoming.description.length > existing.description.length) {
              merged.description = incoming.description;
            } else {
              merged.description =
                compareCodePoint(existing.description, incoming.description) <= 0
                  ? existing.description
                  : incoming.description;
            }
          }
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

  const normalizedTs = `${compiled.replace(/\r\n/g, "\n").trimEnd()}\n`;
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
