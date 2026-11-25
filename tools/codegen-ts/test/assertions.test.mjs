// Guards against the silent failure modes of the JSON Schema -> TypeScript codegen
// (B12/CT-03) and the `Record<string, never>` rendering of keyless argument objects (CT-07).
import test from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import os from "node:os";
import {
  applyKeylessObjectTypes,
  assertDefinitionCoverage,
  assertUniqueTitles,
  canonicalize,
  collectEmittedTypeNames,
  collectReachableDefinitions,
  generateContracts,
  isKeylessObjectSchema,
  mergeDeterministic,
} from "../generate.mjs";

const DRAFT = "https://json-schema.org/draft/2020-12/schema";

async function withSchemas(schemas, fn) {
  const tempDir = await mkdtemp(path.join(os.tmpdir(), "codegen-assert-"));
  const schemasDir = path.join(tempDir, "schemas");
  const tsOutDir = path.join(tempDir, "ts");
  try {
    await mkdir(schemasDir, { recursive: true });
    for (const [filename, schema] of Object.entries(schemas)) {
      await writeFile(
        path.join(schemasDir, `${filename}.schema.json`),
        `${JSON.stringify(schema, null, 2)}\n`,
      );
    }
    return await fn({ tempDir, schemasDir, tsOutDir });
  } finally {
    await rm(tempDir, { recursive: true, force: true });
  }
}

test("mergeDeterministic rejects conflicting descriptions instead of picking the longest", () => {
  const short = {
    title: "Diagnostic",
    description: "Short",
    type: "object",
    properties: { code: { type: "string" } },
  };
  const long = {
    title: "Diagnostic",
    description: "Detailed diagnostic message with full context.",
    type: "object",
    properties: { code: { type: "string" } },
  };

  assert.throws(
    () => mergeDeterministic(short, long, "Diagnostic", "fileA", "fileB"),
    (err) => {
      assert.match(err.message, /conflicting description between fileA and fileB/);
      assert.match(err.message, /"Short"/);
      assert.match(err.message, /"Detailed diagnostic message with full context\."/);
      return true;
    },
  );
  // Order must not matter: the conflict is symmetric.
  assert.throws(
    () => mergeDeterministic(long, short, "Diagnostic", "fileB", "fileA"),
    /conflicting description/,
  );
});

test("mergeDeterministic accepts identical descriptions from different schemas", () => {
  const def1 = { title: "Severity", description: "Severity of a diagnostic.", type: "string" };
  const def2 = { title: "Severity", description: "Severity of a diagnostic.", type: "string" };

  const merged = mergeDeterministic(def1, def2, "Severity", "fileA", "fileB");
  assert.strictEqual(merged.description, "Severity of a diagnostic.");
  assert.strictEqual(merged.title, "Severity");
});

test("assertUniqueTitles rejects a duplicated title with different bodies", () => {
  const defs = {
    A: { title: "Shared", type: "object", properties: { x: { type: "string" } } },
    B: { title: "Shared", type: "object", properties: { y: { type: "string" } } },
  };

  assert.throws(
    () => assertUniqueTitles({ defs, defOrigins: { A: "a.schema.json", B: "b.schema.json" } }),
    (err) => {
      assert.match(err.message, /Duplicate JSON Schema `title` with different bodies/);
      assert.match(err.message, /title 'Shared' is claimed by 'A' \(a\.schema\.json\) and 'B'/);
      return true;
    },
  );
});

test("assertUniqueTitles accepts a duplicated title when the bodies are identical", () => {
  const body = { title: "Shared", type: "object", properties: { x: { type: "string" } } };
  const defs = { A: body, B: { ...body } };

  assert.doesNotThrow(() => assertUniqueTitles({ defs, defOrigins: {} }));
});

test("collectReachableDefinitions only follows $ref chains from the bundle root", () => {
  const bundle = {
    properties: { Root: { $ref: "#/$defs/Root" } },
    $defs: {
      Root: { type: "object", properties: { leaf: { $ref: "#/$defs/Leaf" } } },
      Leaf: { type: "string" },
      Orphan: { type: "number" },
    },
  };

  const reachable = collectReachableDefinitions(bundle);
  assert.deepStrictEqual([...reachable].sort(), ["Leaf", "Root"]);
});

test("collectEmittedTypeNames reads interface and type declarations", () => {
  const ts = [
    "export interface Foo {",
    "  a: string;",
    "}",
    "export type Bar = string;",
    "export type Baz<T> = T;",
  ].join("\n");

  assert.deepStrictEqual([...collectEmittedTypeNames(ts)].sort(), ["Bar", "Baz", "Foo"]);
});

test("assertDefinitionCoverage throws when a $defs entry is dropped as unreachable", () => {
  const bundleSchema = {
    properties: { Root: { $ref: "#/$defs/Root" } },
    $defs: {
      Root: { type: "object", properties: { a: { type: "string" } } },
      Orphan: { type: "string" },
    },
  };
  const compiledTs = `${["export interface Root {", "  a?: string;", "}"].join("\n")}\n`;

  assert.throws(
    () => assertDefinitionCoverage({ compiledTs, bundleSchema, defOrigins: { Orphan: "x.json#/$defs/Orphan" } }),
    (err) => {
      assert.match(err.message, /lost contract definitions/);
      assert.match(err.message, /'Orphan' is emitted nowhere and referenced nowhere/);
      assert.match(err.message, /defined in x\.json#\/\$defs\/Orphan/);
      assert.match(err.message, /unreachableDefinitions: false/);
      return true;
    },
  );
});

test("assertDefinitionCoverage reports a reachable definition missing from the output", () => {
  const bundleSchema = {
    properties: { Root: { $ref: "#/$defs/Root" } },
    $defs: {
      Root: { type: "object", properties: { leaf: { $ref: "#/$defs/Leaf" } } },
      Leaf: { type: "string" },
    },
  };

  assert.throws(
    () => assertDefinitionCoverage({ compiledTs: "export interface Root {}\n", bundleSchema }),
    /'Leaf' is reachable from the bundle root but was not emitted/,
  );
});

test("generateContracts fails loudly on an orphan $defs entry", async () => {
  await withSchemas(
    {
      root: {
        $schema: DRAFT,
        title: "Root",
        type: "object",
        properties: { a: { type: "string" } },
        $defs: { Orphan: { title: "Orphan", type: "string" } },
      },
    },
    async ({ schemasDir, tsOutDir }) => {
      await assert.rejects(
        () => generateContracts(schemasDir, tsOutDir),
        /'Orphan' is emitted nowhere and referenced nowhere \(defined in root\.schema\.json#\/\$defs\/Orphan\)/,
      );
    },
  );
});

test("generateContracts keeps a $defs entry that is explicitly referenced", async () => {
  await withSchemas(
    {
      root: {
        $schema: DRAFT,
        title: "Root",
        type: "object",
        properties: { a: { $ref: "#/$defs/Referenced" } },
        $defs: { Referenced: { title: "Referenced", type: "string" } },
      },
    },
    async ({ schemasDir, tsOutDir }) => {
      const result = await generateContracts(schemasDir, tsOutDir);
      const emitted = await readFile(result.tsOutFile, "utf8");
      assert.match(emitted, /export type Referenced = string;/);
      // Root + the single referenced definition: nothing was silently dropped.
      assert.strictEqual(result.reachableDefinitionCount, 2);
      assert.strictEqual(result.emittedTypeCount, 3);
    },
  );
});

test("isKeylessObjectSchema only matches object schemas that forbid every key", () => {
  assert.strictEqual(
    isKeylessObjectSchema({ title: "EmptyArgs", description: "doc", type: "object", additionalProperties: false }),
    true,
  );
  assert.strictEqual(isKeylessObjectSchema({ type: "object", additionalProperties: true }), false);
  assert.strictEqual(
    isKeylessObjectSchema({ type: "object", additionalProperties: false, properties: {} }),
    false,
  );
  assert.strictEqual(isKeylessObjectSchema({ type: "object" }), false);
});

test("applyKeylessObjectTypes rewrites the empty interface into Record<string, never>", () => {
  const compiledTs = ["export interface EmptyArgs {}", "export type Other = string;"].join("\n");
  const { compiledTs: out, rewritten } = applyKeylessObjectTypes(compiledTs, {
    EmptyArgs: { type: "object", additionalProperties: false },
    Other: { type: "string" },
  });

  assert.deepStrictEqual(rewritten, ["EmptyArgs"]);
  assert.match(out, /^export type EmptyArgs = Record<string, never>;$/m);
  assert.ok(!out.includes("export interface EmptyArgs"), "must not keep the empty interface");
  assert.match(out, /^export type Other = string;$/m);
});

test("applyKeylessObjectTypes fails when the compiled shape is not an empty interface", () => {
  assert.throws(
    () =>
      applyKeylessObjectTypes("export interface EmptyArgs {\n  extra?: string;\n}\n", {
        EmptyArgs: { type: "object", additionalProperties: false },
      }),
    /Expected `export interface EmptyArgs \{\}` for the keyless object schema 'EmptyArgs'/,
  );
});

test("generateContracts emits Record<string, never> for a keyless args object", async () => {
  await withSchemas(
    {
      empty_args: {
        $schema: DRAFT,
        title: "EmptyArgs",
        type: "object",
        additionalProperties: false,
      },
      ping: {
        $schema: DRAFT,
        title: "PingArgs",
        type: "object",
        additionalProperties: false,
        properties: { name: { type: "string" } },
        required: ["name"],
      },
    },
    async ({ schemasDir, tsOutDir }) => {
      const result = await generateContracts(schemasDir, tsOutDir);
      const emitted = await readFile(result.tsOutFile, "utf8");

      assert.match(emitted, /^export type EmptyArgs = Record<string, never>;$/m);
      assert.ok(!emitted.includes("export interface EmptyArgs {}"));
      assert.deepStrictEqual(result.keylessObjects, ["EmptyArgs"]);
      // The aggregate index keeps referencing the type by name, so it stays usable.
      assert.match(emitted, /^ {2}EmptyArgs: EmptyArgs;$/m);

      // Determinism must survive the rewrite.
      const again = await generateContracts(schemasDir, tsOutDir);
      assert.strictEqual(await readFile(again.tsOutFile, "utf8"), emitted);
    },
  );
});

test("generateContracts rejects schemas whose shared def carries two descriptions", async () => {
  const shared = {
    type: "object",
    properties: { code: { type: "string" } },
    required: ["code"],
  };
  await withSchemas(
    {
      first: {
        $schema: DRAFT,
        title: "First",
        type: "object",
        properties: { def: { $ref: "#/$defs/Shared" } },
        $defs: { Shared: { ...shared, description: "Short" } },
      },
      second: {
        $schema: DRAFT,
        title: "Second",
        type: "object",
        properties: { def: { $ref: "#/$defs/Shared" } },
        $defs: { Shared: { ...shared, description: "A much longer description that used to win." } },
      },
    },
    async ({ schemasDir, tsOutDir }) => {
      await assert.rejects(
        () => generateContracts(schemasDir, tsOutDir),
        /conflicting description between first\.schema\.json#\/\$defs\/Shared and second\.schema\.json#\/\$defs\/Shared/,
      );
    },
  );
});

test("generateContracts output stays canonical for shared defs with identical docstrings", async () => {
  const shared = canonicalize({
    description: "Shared documentation.",
    type: "object",
    properties: { code: { type: "string" } },
    required: ["code"],
  });
  await withSchemas(
    {
      first: {
        $schema: DRAFT,
        title: "First",
        type: "object",
        properties: { def: { $ref: "#/$defs/Shared" } },
        $defs: { Shared: shared },
      },
      second: {
        $schema: DRAFT,
        title: "Second",
        type: "object",
        properties: { def: { $ref: "#/$defs/Shared" } },
        $defs: { Shared: shared },
      },
    },
    async ({ schemasDir, tsOutDir }) => {
      const result = await generateContracts(schemasDir, tsOutDir);
      const emitted = await readFile(result.tsOutFile, "utf8");
      assert.match(emitted, /Shared documentation\./);
      assert.strictEqual(
        (emitted.match(/export interface Shared \{/g) ?? []).length,
        1,
        "the shared definition must be emitted exactly once",
      );
    },
  );
});
