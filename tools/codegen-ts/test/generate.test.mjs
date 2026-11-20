import test from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, rm, writeFile, readFile } from "node:fs/promises";
import path from "node:path";
import os from "node:os";
import {
  canonicalize,
  stripDocMetadata,
  mergeDeterministic,
  registerDef,
  generateContracts,
} from "../generate.mjs";

test("canonicalize recursively sorts object keys", () => {
  const input = {
    z: 1,
    b: { y: 2, a: 1 },
    a: [3, { d: 4, c: 5 }],
  };
  const output = canonicalize(input);
  assert.deepStrictEqual(Object.keys(output), ["a", "b", "z"]);
  assert.deepStrictEqual(Object.keys(output.b), ["a", "y"]);
  assert.deepStrictEqual(Object.keys(output.a[1]), ["c", "d"]);
});

test("stripDocMetadata removes title and description recursively", () => {
  const input = {
    title: "Root",
    description: "Root desc",
    type: "object",
    properties: {
      field: {
        title: "FieldTitle",
        description: "FieldDesc",
        type: "string",
      },
    },
  };
  const stripped = stripDocMetadata(input);
  assert.deepStrictEqual(stripped, {
    type: "object",
    properties: {
      field: {
        type: "string",
      },
    },
  });
});

test("mergeDeterministic merges shared defs with different descriptions deterministically", () => {
  const def1 = {
    title: "Diagnostic",
    description: "Short",
    type: "object",
    properties: {
      code: { type: "string" },
    },
  };
  const def2 = {
    title: "Diagnostic",
    description: "Detailed diagnostic message with full context.",
    type: "object",
    properties: {
      code: { type: "string" },
    },
  };

  const merged1 = mergeDeterministic(def1, def2, "Diagnostic", "fileA", "fileB");
  const merged2 = mergeDeterministic(def2, def1, "Diagnostic", "fileB", "fileA");

  // Both should pick the longer, more descriptive docstring
  assert.strictEqual(merged1.description, "Detailed diagnostic message with full context.");
  assert.strictEqual(merged2.description, "Detailed diagnostic message with full context.");
  assert.deepStrictEqual(merged1, merged2);
});

test("mergeDeterministic reconciles missing titles/descriptions", () => {
  const defWithoutTitle = {
    type: "string",
    enum: ["QA", "CL"],
  };
  const defWithTitle = {
    title: "NoteType",
    description: "Supported note types.",
    type: "string",
    enum: ["QA", "CL"],
  };

  const merged = mergeDeterministic(defWithoutTitle, defWithTitle, "NoteType", "fileA", "fileB");
  assert.strictEqual(merged.title, "NoteType");
  assert.strictEqual(merged.description, "Supported note types.");
  assert.deepStrictEqual(merged.enum, ["QA", "CL"]);
});

test("mergeDeterministic throws on conflicting titles", () => {
  const def1 = { title: "TypeA", type: "string" };
  const def2 = { title: "TypeB", type: "string" };

  assert.throws(
    () => mergeDeterministic(def1, def2, "Target", "fileA", "fileB"),
    /conflicting title \('TypeA' vs 'TypeB'\)/,
  );
});

test("mergeDeterministic throws on structural mismatch", () => {
  const def1 = {
    type: "object",
    properties: { id: { type: "string" } },
  };
  const def2 = {
    type: "object",
    properties: { id: { type: "integer" } },
  };

  assert.throws(
    () => mergeDeterministic(def1, def2, "Target", "fileA", "fileB"),
    /structural mismatch between fileA and fileB/,
  );
});

test("registerDef records origins and detects collisions cleanly", () => {
  const defs = {};
  const origins = {};

  registerDef(defs, origins, "Foo", { type: "string" }, "schema1.json");
  assert.strictEqual(defs.Foo.type, "string");

  // Re-register with identical structure
  registerDef(defs, origins, "Foo", { type: "string", description: "Doc" }, "schema2.json");
  assert.strictEqual(defs.Foo.description, "Doc");

  // Re-register with collision
  assert.throws(
    () => registerDef(defs, origins, "Foo", { type: "number" }, "schema3.json"),
    /structural mismatch/,
  );
});

test("mergeDeterministic preserves object properties named title or description", () => {
  const def1 = {
    title: "QualitySourceArg",
    type: "object",
    properties: {
      id: { type: "string" },
      title: { type: "string" },
      description: { type: "string" },
    },
  };
  const def2 = {
    title: "QualitySourceArg",
    type: "object",
    properties: {
      id: { type: "string" },
      title: { type: "string" },
      description: { type: "string" },
    },
  };

  const merged = mergeDeterministic(def1, def2, "QualitySourceArg", "fileA", "fileB");
  assert.deepStrictEqual(merged.properties.title, { type: "string" });
  assert.deepStrictEqual(merged.properties.description, { type: "string" });
});

test("generateContracts produces deterministic LF .d.ts from test schemas", async () => {
  const tempDir = await mkdtemp(path.join(os.tmpdir(), "codegen-test-"));
  const schemasDir = path.join(tempDir, "schemas");
  const tsOutDir = path.join(tempDir, "ts");

  const fs = await import("node:fs/promises");
  await fs.mkdir(schemasDir, { recursive: true });

  const schema1 = {
    $schema: "https://json-schema.org/draft/2020-12/schema",
    title: "Severity",
    description: "Diagnostic severity levels.",
    type: "string",
    enum: ["info", "warning", "error"],
  };

  const schema2 = {
    $schema: "https://json-schema.org/draft/2020-12/schema",
    title: "Diagnostic",
    description: "Validation diagnostic.",
    type: "object",
    properties: {
      severity: { $ref: "#/$defs/Severity" },
      message: { type: "string" },
    },
    required: ["message", "severity"],
    $defs: {
      Severity: {
        title: "Severity",
        type: "string",
        enum: ["info", "warning", "error"],
      },
    },
  };

  await writeFile(path.join(schemasDir, "severity.schema.json"), JSON.stringify(schema1, null, 2) + "\n");
  await writeFile(path.join(schemasDir, "diagnostic.schema.json"), JSON.stringify(schema2, null, 2) + "\n");

  // Run 1
  const res1 = await generateContracts(schemasDir, tsOutDir);
  const content1 = await readFile(res1.tsOutFile, "utf8");

  // Check LF endings
  assert.ok(!content1.includes("\r\n"), "must not contain CRLF");
  assert.ok(content1.endsWith("\n"), "must end with LF");

  // Run 2 (determinism check)
  const res2 = await generateContracts(schemasDir, tsOutDir);
  const content2 = await readFile(res2.tsOutFile, "utf8");

  assert.strictEqual(content1, content2, "two consecutive runs must be byte-identical");
  assert.ok(content1.includes("export type Severity ="), "must include Severity type");
  assert.ok(content1.includes("export interface Diagnostic {"), "must include Diagnostic interface");

  // Check that tsc validates the generated file with --noEmit
  const { execFileSync } = await import("node:child_process");
  const tscJs = path.resolve("node_modules", "typescript", "bin", "tsc");
  execFileSync(process.execPath, [tscJs, "--noEmit", "--target", "ES2024", "--module", "NodeNext", res1.tsOutFile], {
    cwd: tempDir,
    stdio: "pipe",
  });

  await rm(tempDir, { recursive: true, force: true });
});
