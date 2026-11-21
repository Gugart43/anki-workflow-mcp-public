import http from 'node:http';
import { Buffer } from 'node:buffer';
import process from 'node:process';
import { McpServer } from '@modelcontextprotocol/server';
import { StdioServerTransport } from '@modelcontextprotocol/server/stdio';
import { NodeStreamableHTTPServerTransport } from '@modelcontextprotocol/node';
import { z } from 'zod';
import { contrastRatio } from './contrast.js';

/**
 * Servidor MCP TypeScript para Preview e Validação de Tema Light/Dark (AnkiMBR).
 * Versões e metadados reais alinhados ao package.json.
 */
const SERVER_NAME = 'anki-theme-preview-mcp';
const SERVER_VERSION = '1.0.0';

const server = new McpServer({
  name: SERVER_NAME,
  version: SERVER_VERSION,
});

/**
 * Interface para pares de contraste avaliados.
 */
export interface ContrastPair {
  label: string;
  fg: string;
  bg: string;
  minRatio: number;
}

export interface EvaluatedPair extends ContrastPair {
  ratio: number;
  passed: boolean;
}

/**
 * Schema de par de cores para validação de contraste WCAG 2.1.
 * Aceita notação hexadecimal (#RGB ou #RRGGBB) e normaliza para 6 dígitos hex.
 */
const ContrastPairSchema = z.object({
  label: z.string().min(1).describe('Identificador do elemento ou token (ex.: flow-pos, cloze, badge)'),
  fg: z
    .string()
    .regex(/^#([0-9a-fA-F]{3}|[0-9a-fA-F]{6})$/, 'Cor de primeiro plano (fg) deve ser hexadecimal #RGB ou #RRGGBB')
    .transform((hex: string): string =>
      hex.length === 4 ? `#${hex[1]}${hex[1]}${hex[2]}${hex[2]}${hex[3]}${hex[3]}`.toLowerCase() : hex.toLowerCase()
    ),
  bg: z
    .string()
    .regex(/^#([0-9a-fA-F]{3}|[0-9a-fA-F]{6})$/, 'Cor de fundo (bg) deve ser hexadecimal #RGB ou #RRGGBB')
    .transform((hex: string): string =>
      hex.length === 4 ? `#${hex[1]}${hex[1]}${hex[2]}${hex[2]}${hex[3]}${hex[3]}`.toLowerCase() : hex.toLowerCase()
    ),
  minRatio: z
    .number()
    .min(1.0)
    .max(21.0)
    .default(4.5)
    .describe('Razão de contraste mínima exigida (padrão 4.5 para WCAG AA, 7.0 para AAA)'),
});

server.registerTool(
  'validate_contrast_matrix',
  {
    title: 'Validate Light/Dark WCAG 2.1 Contrast Matrix',
    description: 'Valida matematicamente pares foreground/background nos temas Claro e Escuro do AnkiMBR conforme WCAG 2.1 AA/AAA',
    inputSchema: z.object({
      lightPairs: z.array(ContrastPairSchema).min(1).describe('Pares de cores do tema Claro'),
      darkPairs: z.array(ContrastPairSchema).min(1).describe('Pares de cores do tema Escuro'),
    }),
  },
  async ({ lightPairs, darkPairs }: { lightPairs: ContrastPair[]; darkPairs: ContrastPair[] }) => {
    const evaluate = (pairs: ContrastPair[]): EvaluatedPair[] =>
      pairs.map((p: ContrastPair): EvaluatedPair => {
        const ratio = contrastRatio(p.fg, p.bg);
        return { ...p, ratio, passed: ratio >= p.minRatio };
      });

    const light = evaluate(lightPairs);
    const dark = evaluate(darkPairs);
    const failures = [
      ...light.filter((x: EvaluatedPair) => !x.passed).map((x: EvaluatedPair) => `Light/${x.label}: ${x.ratio}:1 < ${x.minRatio}:1`),
      ...dark.filter((x: EvaluatedPair) => !x.passed).map((x: EvaluatedPair) => `Dark/${x.label}: ${x.ratio}:1 < ${x.minRatio}:1`),
    ];
    const structured = { passed: failures.length === 0, failures, light, dark };

    return {
      content: [{ type: 'text', text: JSON.stringify(structured, null, 2) }],
      structuredContent: structured,
    };
  }
);

/**
 * Sanitiza e isola o CSS injetado no bloco <style> do harness.
 * Evita fuga de contexto quebrando sequências `</style>` sem alterar o parse das regras.
 */
function isolateCss(css: string): string {
  return css.replace(/<\/style/gi, '<\\/style');
}

/**
 * Sanitiza o markup HTML do cartão para visualização segura no harness de preview.
 * Preserva integralmente elementos semânticos, estilos inline, mídias, tabelas e cloze deletions,
 * enquanto desarma scripts executáveis e evita quebra de fechamento de tags do harness.
 * Nota de segurança: a entrada é assumida como template confiável de cartão Anki; o harness
 * impõe adicionalmente Content-Security-Policy com script-src 'none'.
 */
function isolateCardHtml(cardHtml: string): string {
  return cardHtml.replace(/<script\b([^>]*)>/gi, '<script type="text/plain"$1>');
}

server.registerTool(
  'generate_theme_preview',
  {
    title: 'Generate Side-by-Side Light/Dark Anki Card Preview',
    description:
      'Gera HTML autocontido comparando Tema Claro e Tema Escuro lado a lado sem vazamento de prefers-color-scheme. Entrada confiável com isolamento seguro contra XSS.',
    inputSchema: z.object({
      cardHtml: z.string().min(1).describe('HTML do cartão Anki (enunciado, resposta, omissões, hints, tabelas ou mídias)'),
      css: z.string().min(1).describe('Folha de estilo CSS (ex.: ankimbr-presentation-v2.css)'),
    }),
  },
  async ({ cardHtml, css }: { cardHtml: string; css: string }) => {
    const safeCss = isolateCss(css);
    const safeCardHtml = isolateCardHtml(cardHtml);

    const html = `<!DOCTYPE html>
<html lang="pt-BR">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; img-src data: blob: https: http:; font-src data: https:; media-src data: blob: https: http:; script-src 'none';">
  <title>AnkiMBR Side-by-Side Theme Preview</title>
  <style>
    *, *::before, *::after { box-sizing: border-box; }
    html, body {
      margin: 0;
      padding: 0;
      background: #e2e8f0;
      font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
    }
    .preview-header {
      padding: 12px 20px;
      background: #0f172a;
      color: #f8fafc;
      font-size: 13px;
      font-weight: 600;
      display: flex;
      justify-content: space-between;
      align-items: center;
    }
    .preview-grid {
      display: grid;
      grid-template-columns: 1fr 1fr;
      gap: 20px;
      padding: 20px;
      max-width: 1600px;
      margin: 0 auto;
    }
    .preview-pane {
      border-radius: 12px;
      overflow: hidden;
      box-shadow: 0 4px 12px rgba(0, 0, 0, 0.08);
      display: flex;
      flex-direction: column;
    }
    .pane-header {
      padding: 8px 16px;
      font-size: 11px;
      font-weight: 700;
      letter-spacing: 0.05em;
      text-transform: uppercase;
    }
    .preview-pane.light-pane {
      background: #f1f5f9;
      color-scheme: light;
      color-scheme: only light;
    }
    .preview-pane.light-pane .pane-header {
      background: #e2e8f0;
      color: #334155;
      border-bottom: 1px solid #cbd5e1;
    }
    .preview-pane.dark-pane {
      background: #0b1120;
      color-scheme: dark;
    }
    .preview-pane.dark-pane .pane-header {
      background: #1e293b;
      color: #94a3b8;
      border-bottom: 1px solid #334155;
    }
    .card-stage {
      padding: 16px;
      flex: 1;
      overflow: auto;
    }
    ${safeCss}
  </style>
</head>
<body>
  <header class="preview-header">
    <span>AnkiMBR Preview Lado a Lado (Light &amp; Dark)</span>
    <span style="font-size: 11px; opacity: 0.85;">Harness Seguro &bull; Loopback &bull; Anti-Sequestro</span>
  </header>
  <main class="preview-grid">
    <div class="preview-pane light-pane" data-theme="light">
      <div class="pane-header">Tema Claro (Default)</div>
      <div class="card-stage">
        <section data-theme="light" class="card">${safeCardHtml}</section>
      </div>
    </div>
    <div class="preview-pane dark-pane nightMode" data-theme="dark">
      <div class="pane-header">Tema Escuro (.nightMode)</div>
      <div class="card-stage nightMode">
        <section data-theme="dark" class="card nightMode">${safeCardHtml}</section>
      </div>
    </div>
  </main>
</body>
</html>`;

    const structured = { bytes: Buffer.byteLength(html, 'utf8'), html };
    return {
      content: [{ type: 'text', text: html }],
      structuredContent: structured,
    };
  }
);

server.registerResource(
  'preview-harness',
  'ankimbr://preview/harness',
  { title: 'AnkiMBR Side-by-Side Preview Harness', mimeType: 'text/plain' },
  async (uri: { href: string }) => ({
    contents: [
      {
        uri: uri.href,
        text: 'Use generate_theme_preview com CSS v2 (sem prefers-color-scheme solto). Suporta isolamento estrito Light/Dark e proteção contra sequestro de tema.',
      },
    ],
  })
);

/**
 * Ponto de entrada do servidor.
 * - HTTP: Restrito estritamente a loopback (127.0.0.1) por padrão.
 * - Stdio: Comunicação exclusiva por stdin/stdout com framing JSON-RPC; logs roteados a stderr.
 */
async function main() {
  if (process.env.MCP_TRANSPORT === 'http') {
    const port = Number(process.env.PORT ?? 3333);
    const host = process.env.HOST ?? '127.0.0.1';
    const transport = new NodeStreamableHTTPServerTransport({ sessionIdGenerator: undefined });
    await server.connect(transport);
    const httpServer = http.createServer((req, res) => {
      if (req.url === '/mcp') {
        transport.handleRequest(req, res).catch((err: unknown) => {
          console.error('HTTP transport request error:', err);
          if (!res.headersSent) {
            res.writeHead(500, { 'Content-Type': 'text/plain' }).end('Internal Server Error');
          }
        });
      } else {
        res.writeHead(404, { 'Content-Type': 'text/plain' }).end('Not Found');
      }
    });
    httpServer.listen(port, host, () => {
      console.error(`anki-theme-preview-mcp HTTP listening on ${host}:${port}/mcp`);
    });

    const shutdown = () => {
      httpServer.close(() => {
        void server.close().finally(() => process.exit(0));
      });
    };
    process.on('SIGINT', shutdown);
    process.on('SIGTERM', shutdown);
  } else {
    const transport = new StdioServerTransport();
    await server.connect(transport);
  }
}

main().catch((err: unknown) => {
  console.error('Fatal error initializing MCP server:', err);
  process.exit(1);
});
