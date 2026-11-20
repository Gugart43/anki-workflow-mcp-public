import { createHash } from 'node:crypto';
import { existsSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import {
  Client,
  ProtocolError,
  ProtocolErrorCode,
  type CallToolResult,
  type Tool,
} from '@modelcontextprotocol/client';
import { StdioClientTransport } from '@modelcontextprotocol/client/stdio';
import type { AnySchemaObject } from 'ajv';
import { Ajv2020 } from 'ajv/dist/2020.js';
import addFormats from 'ajv-formats';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';

const EXPECTED_TOOLS = [
  'batch_patch_draft',
  'cards_compile_drafts',
  'export_reviewed_files',
  'generation_contract_get',
  'quality_evaluate',
  'quality_prepare',
  'review_batch_information',
] as const;

const CANARY_MARKER = 'TS_CONTRACT_CANARY_9472';
const PRIVACY_CANARY_MARKER = 'CONFIDENTIAL_PATIENT_CANARY_5813';
const TEARDOWN_TIMEOUT_MS = 10_000;

type JsonRecord = Record<string, unknown>;

function isRecord(value: unknown): value is JsonRecord {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function resolveBinaryPath(): string {
  const fromEnv = process.env['ANKI_MCP_BIN']?.trim();
  if (fromEnv !== undefined && fromEnv.length > 0) {
    const resolvedEnv = path.resolve(fromEnv);
    if (!existsSync(resolvedEnv)) {
      throw new Error(`ANKI_MCP_BIN does not exist: ${resolvedEnv}`);
    }
    return resolvedEnv;
  }

  const currentDir = path.dirname(fileURLToPath(import.meta.url));
  const repoRoot = path.resolve(currentDir, '..', '..');
  const exeName = process.platform === 'win32' ? 'anki-workflow-mcp.exe' : 'anki-workflow-mcp';
  const fallback = path.join(repoRoot, 'target', 'debug', exeName);

  if (!existsSync(fallback)) {
    throw new Error(
      `Binary not found at ${fallback}. Build it with \`cargo build --locked -p anki-workflow-mcp\` or set ANKI_MCP_BIN.`,
    );
  }
  return fallback;
}

function extractToolPayload(result: CallToolResult): JsonRecord {
  if (isRecord(result.structuredContent)) {
    return result.structuredContent;
  }

  const firstBlock = result.content[0];
  if (firstBlock === undefined || firstBlock.type !== 'text') {
    throw new Error(`Expected structuredContent or text block, got: ${JSON.stringify(result)}`);
  }

  const parsed: unknown = JSON.parse(firstBlock.text);
  if (!isRecord(parsed)) {
    throw new Error(`Expected JSON object payload, got: ${firstBlock.text}`);
  }
  return parsed;
}

function assertTypedObjectsDenyAdditionalProperties(schema: unknown, schemaPath: string): void {
  // Strict drafts now enforced at the MCP boundary with no exemption.
  if (!isRecord(schema)) {
    return;
  }

  if (
    'properties' in schema &&
    !schemaPath.endsWith('.properties.draft') &&
    !schemaPath.endsWith('.properties.updates')
  ) {
    expect(
      schema['additionalProperties'],
      `typed object schema at ${schemaPath} must set additionalProperties: false`,
    ).toBe(false);
  }

  const properties = schema['properties'];
  if (isRecord(properties)) {
    for (const [key, propSchema] of Object.entries(properties)) {
      assertTypedObjectsDenyAdditionalProperties(propSchema, `${schemaPath}.properties.${key}`);
    }
  }

  if ('items' in schema) {
    assertTypedObjectsDenyAdditionalProperties(schema['items'], `${schemaPath}.items`);
  }

  for (const combinator of ['anyOf', 'oneOf', 'allOf'] as const) {
    const branches = schema[combinator];
    if (Array.isArray(branches)) {
      branches.forEach((branch, index) => {
        assertTypedObjectsDenyAdditionalProperties(branch, `${schemaPath}.${combinator}[${index}]`);
      });
    }
  }

  for (const defsKey of ['$defs', 'definitions'] as const) {
    const defs = schema[defsKey];
    if (isRecord(defs)) {
      for (const [key, defSchema] of Object.entries(defs)) {
        assertTypedObjectsDenyAdditionalProperties(defSchema, `${schemaPath}.${defsKey}.${key}`);
      }
    }
  }
}

function isPidAlive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch (err: unknown) {
    if (isRecord(err) && err['code'] === 'EPERM') {
      return true;
    }
    return false;
  }
}

async function waitForPidExit(pid: number, timeoutMs: number): Promise<boolean> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (!isPidAlive(pid)) {
      return true;
    }
    await new Promise<void>((resolve) => {
      const timer = setTimeout(resolve, 25);
      timer.unref?.();
    });
  }
  return !isPidAlive(pid);
}

describe('anki-workflow-mcp TypeScript E2E contract', () => {
  let client: Client | null = null;
  let transport: StdioClientTransport | null = null;
  let stderrBuffer = '';
  let childPid: number | null = null;
  let listedTools: Tool[] = [];

  beforeAll(async () => {
    try {
      const binPath = resolveBinaryPath();
      transport = new StdioClientTransport({
        command: binPath,
        args: ['serve'],
        stderr: 'pipe',
      });

      const stderrStream = transport.stderr;
      if (stderrStream !== null) {
        stderrStream.on('data', (chunk: Buffer | string) => {
          stderrBuffer += typeof chunk === 'string' ? chunk : chunk.toString('utf8');
        });
      }

      client = new Client({
        name: 'anki-workflow-mcp-contract-ts',
        version: '0.1.0',
      });

      await client.connect(transport);
      childPid = transport.pid ?? null;
    } catch (err: unknown) {
      // Bounded teardown: if beforeAll throws, clean up any spawned child process immediately
      if (transport?.pid) {
        try {
          process.kill(transport.pid, 'SIGKILL');
        } catch {
          // Process already terminated
        }
      }
      throw err;
    }
  });

  afterAll(async () => {
    let timeoutId: NodeJS.Timeout | undefined;
    const closePromise = client ? client.close() : Promise.resolve();
    const timeoutPromise = new Promise<never>((_, reject) => {
      timeoutId = setTimeout(() => {
        reject(new Error(`MCP client close() timed out after ${TEARDOWN_TIMEOUT_MS}ms`));
      }, TEARDOWN_TIMEOUT_MS);
    });

    try {
      await Promise.race([closePromise, timeoutPromise]);
    } finally {
      if (timeoutId !== undefined) {
        clearTimeout(timeoutId);
      }
      if (childPid !== null) {
        const exitedCleanly = await waitForPidExit(childPid, 3_000);
        if (!exitedCleanly) {
          try {
            process.kill(childPid, 'SIGKILL');
          } catch {
            // Process exited between liveness check and kill.
          }
          throw new Error(`Child MCP server process (pid=${childPid}) did not exit cleanly after close()`);
        }
      }
    }

    expect(stderrBuffer).not.toContain(CANARY_MARKER);
    expect(stderrBuffer).not.toContain(PRIVACY_CANARY_MARKER);
  });

  it('completes real MCP stdio handshake with expected server metadata and capabilities', () => {
    expect(client).toBeDefined();
    if (!client) {
      throw new Error('Client not initialized');
    }

    const serverInfo = client.getServerVersion();
    expect(serverInfo).toBeDefined();
    expect(serverInfo?.name).toBe('anki-workflow-mcp');
    expect(serverInfo?.version).toMatch(/^\d+\.\d+\.\d+/);

    const capabilities = client.getServerCapabilities();
    expect(capabilities?.tools).toBeDefined();

    const instructions = client.getInstructions();
    expect(instructions).toBeDefined();
    expect(instructions?.trim().length).toBeGreaterThan(0);
    expect(instructions).toContain('generation_contract_get');
    expect(instructions).toContain('quality_v1');
  });

  it('lists exact 7 tools with valid JSON Schema Draft 2020-12 and all 4 ToolAnnotations', async () => {
    if (!client) {
      throw new Error('Client not initialized');
    }

    const response = await client.listTools();
    listedTools = response.tools;

    expect(listedTools).toHaveLength(EXPECTED_TOOLS.length);
    const actualNames = listedTools.map((tool) => tool.name).sort();
    expect(actualNames).toEqual([...EXPECTED_TOOLS]);

    const ajv = new Ajv2020({
      allErrors: true,
      strict: false,
      validateSchema: true,
    });
    addFormats(ajv);

    for (const tool of listedTools) {
      const schema = tool.inputSchema as AnySchemaObject;
      expect(schema['type'], `${tool.name}.inputSchema.type`).toBe('object');
      expect(schema['additionalProperties'], `${tool.name}.inputSchema.additionalProperties`).toBe(false);
      assertTypedObjectsDenyAdditionalProperties(schema, tool.name);

      const isValidMetaSchema = ajv.validateSchema(schema);
      expect(isValidMetaSchema, `${tool.name} meta-schema errors: ${ajv.errorsText(ajv.errors)}`).toBe(true);

      const validate = ajv.compile(schema);
      expect(typeof validate).toBe('function');

      if (tool.name === 'generation_contract_get') {
        expect(validate({})).toBe(true);
        expect(validate({ extra: true })).toBe(false);
      }

      if (tool.name === 'quality_prepare') {
        expect(validate({ qa_csv: '', cloze_csv: '' })).toBe(true);
        expect(validate({ qa_csv: '', cloze_csv: '', reviews: [] })).toBe(false);
      }

      const annotations = tool.annotations;
      expect(annotations, `${tool.name} must expose ToolAnnotations`).toBeDefined();

      // Export tool is state-mutating on disk; other tools are read-only inspection tools
      if (tool.name === 'export_reviewed_files') {
        expect(annotations?.readOnlyHint, `${tool.name}.annotations.readOnlyHint`).toBe(false);
      } else {
        expect(annotations?.readOnlyHint, `${tool.name}.annotations.readOnlyHint`).toBe(true);
      }

      expect(annotations?.destructiveHint, `${tool.name}.annotations.destructiveHint`).toBe(false);
      expect(annotations?.idempotentHint, `${tool.name}.annotations.idempotentHint`).toBe(true);
      expect(annotations?.openWorldHint, `${tool.name}.annotations.openWorldHint`).toBe(false);
    }
  });

  it('executes smoke calls for generation_contract_get and cards_compile_drafts', async () => {
    if (!client) {
      throw new Error('Client not initialized');
    }

    const contractResult = (await client.callTool({
      name: 'generation_contract_get',
      arguments: {},
    })) as CallToolResult;

    expect(contractResult.isError).not.toBe(true);
    const contract = extractToolPayload(contractResult);
    expect(contract['schema_version']).toBe('quality_v1');
    expect(contract['no_count_only_limit']).toBe(true);
    expect(Array.isArray(contract['phases'])).toBe(true);
    expect((contract['phases'] as unknown[]).length).toBeGreaterThan(0);
    expect(isRecord(contract['per_retrieval_rubric'])).toBe(true);
    expect(isRecord(contract['field_ordering'])).toBe(true);
    expect(Array.isArray(contract['limitations'])).toBe(true);
    expect(isRecord(contract['repair_protocol'])).toBe(true);

    const compileResult = (await client.callTool({
      name: 'cards_compile_drafts',
      arguments: {
        drafts: [
          {
            tipo: 'QA',
            enunciado_ou_sentenca: `Pergunta sintetica ${CANARY_MARKER}?`,
            resposta: 'Resposta sintetica objetiva.',
            campo_estudo: 'Teste',
            referencias: 'Manual Sintetico (2026), Secao 1',
          },
        ],
      },
    })) as CallToolResult;

    expect(compileResult.isError).not.toBe(true);
    const compiled = extractToolPayload(compileResult);
    const stats = compiled['stats'];
    expect(isRecord(stats)).toBe(true);
    if (isRecord(stats)) {
      expect(stats['qa_count']).toBe(1);
      expect(stats['cloze_count']).toBe(0);
    }
    expect(typeof compiled['qa_csv']).toBe('string');
    expect(compiled['qa_csv']).toContain(CANARY_MARKER);
    expect(typeof compiled['cloze_csv']).toBe('string');
  });

  it('returns domain errors as isError: true without breaking the session', async () => {
    if (!client) {
      throw new Error('Client not initialized');
    }

    const unexpectedArgResult = (await client.callTool({
      name: 'generation_contract_get',
      arguments: { unexpected: true },
    })) as CallToolResult;

    expect(unexpectedArgResult.isError).toBe(true);
    const unexpectedPayload = extractToolPayload(unexpectedArgResult);
    expect(unexpectedPayload['code']).toBe('INVALID_ARGUMENTS');

    const badTipoResult = (await client.callTool({
      name: 'cards_compile_drafts',
      arguments: {
        drafts: [
          {
            tipo: 'CLOZ',
            enunciado_ou_sentenca: 'Draft com tipo invalido',
          },
        ],
      },
    })) as CallToolResult;

    expect(badTipoResult.isError).toBe(true);
    const badTipoPayload = extractToolPayload(badTipoResult);
    expect(badTipoPayload['code']).toBe('INVALID_ARGUMENTS');
    expect(String(badTipoPayload['error'])).toContain('CLOZ');

    const badRangeResult = (await client.callTool({
      name: 'quality_evaluate',
      arguments: {
        qa_csv: '',
        cloze_csv: '',
        bindings: [
          {
            note_type: 'QA',
            source_row: 1,
            obligation_ids: [],
            evidence: [],
          },
          {
            note_type: 'CL',
            source_row: 3,
            cloze_index: 0,
            obligation_ids: [],
            evidence: [],
          },
        ],
      },
    })) as CallToolResult;

    expect(badRangeResult.isError).toBe(true);
    const rangePayload = extractToolPayload(badRangeResult);
    expect(rangePayload['code']).toBe('INVALID_INPUT_RANGES');
    const violations = rangePayload['violations'];
    expect(Array.isArray(violations)).toBe(true);
    if (Array.isArray(violations)) {
      expect(violations).toHaveLength(2);
      const firstViolation = violations[0];
      const secondViolation = violations[1];
      expect(isRecord(firstViolation) && firstViolation['code']).toBe('INVALID_SOURCE_ROW');
      expect(isRecord(secondViolation) && secondViolation['code']).toBe('INVALID_CLOZE_INDEX');
    }
  });

  it('rejects unknown tool calls with a JSON-RPC protocol error and keeps session usable', async () => {
    if (!client) {
      throw new Error('Client not initialized');
    }

    let caughtError: unknown;
    try {
      await client.callTool({
        name: 'unknown_tool_xyz',
        arguments: {},
      });
    } catch (err: unknown) {
      caughtError = err;
    }

    expect(caughtError).toBeDefined();
    expect(ProtocolError.isInstance(caughtError)).toBe(true);
    if (ProtocolError.isInstance(caughtError)) {
      expect(caughtError.code).toBe(ProtocolErrorCode.InvalidParams);
      expect(caughtError.message).toContain('Unknown tool: unknown_tool_xyz');
    }

    const followUpResult = (await client.callTool({
      name: 'generation_contract_get',
      arguments: {},
    })) as CallToolResult;
    expect(followUpResult.isError).not.toBe(true);
    expect(extractToolPayload(followUpResult)['schema_version']).toBe('quality_v1');
  });

  it('preserves privacy and stdout hygiene: sensitive inputs never leak to stderr or unhandled traces', async () => {
    if (!client) {
      throw new Error('Client not initialized');
    }

    const privacyResult = (await client.callTool({
      name: 'cards_compile_drafts',
      arguments: {
        drafts: [
          {
            tipo: 'QA',
            enunciado_ou_sentenca: `Pergunta clinica restrita ${PRIVACY_CANARY_MARKER}?`,
            resposta: 'Resposta restrita objetiva.',
            campo_estudo: 'Privacidade',
            referencias: 'Registro Clinico Sintetico (2026)',
          },
        ],
      },
    })) as CallToolResult;

    expect(privacyResult.isError).not.toBe(true);
    expect(stderrBuffer).not.toContain(PRIVACY_CANARY_MARKER);
  });

  it('executes export_reviewed_files with atomic write, idempotent replay, and verdict enforcement', async () => {
    if (!client) {
      throw new Error('Client not initialized');
    }

    const qaCsv =
      'Enunciado|Resposta|Campo de Estudo|Notas Adicionais|Imagens Relacionadas|Referências|Etiquetas\n' +
      'Pergunta sintetica export?|Resposta sintetica export.|Teste|||Manual Sintetico (2026)|teste\n';
    const clozeCsv =
      'Sentença|Notas Adicionais|Campo de Estudo|Imagens Relacionadas|Referências|Etiquetas\n';
    const receiptHash = createHash('sha256')
      .update(Buffer.from('qa\0', 'utf8'))
      .update(Buffer.from(qaCsv, 'utf8'))
      .update(Buffer.from('\0cloze\0', 'utf8'))
      .update(Buffer.from(clozeCsv, 'utf8'))
      .digest('hex');

    const tmpDeckDir = mkdtempSync(path.join(tmpdir(), 'anki-mcp-ts-export-'));
    try {
      const firstExport = (await client.callTool({
        name: 'export_reviewed_files',
        arguments: {
          deck_dir: tmpDeckDir,
          batch_id: '20261120_ts_export',
          topic_slug: 'ts_export',
          qa_csv: qaCsv,
          cloze_csv: clozeCsv,
          receipt_hash: receiptHash,
          evaluation_verdict: 'PASS',
        },
      })) as CallToolResult;
      expect(firstExport.isError).not.toBe(true);
      expect(extractToolPayload(firstExport)['status']).toBe('EXPORTED');

      const replayExport = (await client.callTool({
        name: 'export_reviewed_files',
        arguments: {
          deck_dir: tmpDeckDir,
          batch_id: '20261120_ts_export',
          topic_slug: 'ts_export',
          qa_csv: qaCsv,
          cloze_csv: clozeCsv,
          receipt_hash: receiptHash,
          evaluation_verdict: 'PASS',
        },
      })) as CallToolResult;
      expect(replayExport.isError).not.toBe(true);
      expect(extractToolPayload(replayExport)['status']).toBe('ALREADY_EXPORTED');

      const failVerdict = (await client.callTool({
        name: 'export_reviewed_files',
        arguments: {
          deck_dir: tmpDeckDir,
          batch_id: '20261120_ts_export_fail',
          topic_slug: 'ts_export_fail',
          qa_csv: qaCsv,
          cloze_csv: clozeCsv,
          receipt_hash: receiptHash,
          evaluation_verdict: 'FAIL',
        },
      })) as CallToolResult;
      expect(failVerdict.isError).toBe(true);
    } finally {
      rmSync(tmpDeckDir, { recursive: true, force: true });
    }
  });
});
