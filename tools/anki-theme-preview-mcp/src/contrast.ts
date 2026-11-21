/**
 * Utilitários matemáticos para cálculo de Luminância Relativa e Razão de Contraste
 * em estrita conformidade com as diretrizes W3C WCAG 2.1 (Web Content Accessibility Guidelines).
 *
 * Referências normativas:
 * - https://www.w3.org/TR/WCAG21/#dfn-relative-luminance
 * - https://www.w3.org/TR/WCAG21/#dfn-contrast-ratio
 */

/**
 * Normaliza e valida uma string de cor hexadecimal para o formato padrão RRGGBB (6 caracteres).
 * Suporta notações '#RGB', 'RGB', '#RRGGBB' e 'RRGGBB' com tolerância a maiúsculas/minúsculas
 * e espaços em branco nas extremidades.
 *
 * @param hex Código de cor hexadecimal
 * @returns String de 6 caracteres hexadecimais minúsculos sem o prefixo '#'
 * @throws {TypeError} Se o argumento não for uma string
 * @throws {Error} Se o formato for inválido ou contiver caracteres não-hexadecimais
 */
export function normalizeHexColor(hex: string): string {
  if (typeof hex !== 'string') {
    throw new TypeError(`Cor hexadecimal deve ser uma string, recebido: ${typeof hex}`);
  }

  const trimmed = hex.trim();
  const clean = trimmed.startsWith('#') ? trimmed.slice(1) : trimmed;

  if (clean.length === 3) {
    if (!/^[0-9a-fA-F]{3}$/.test(clean)) {
      throw new Error(`Cor hexadecimal de 3 dígitos contém caracteres inválidos: "${hex}"`);
    }
    const r = clean[0];
    const g = clean[1];
    const b = clean[2];
    return `${r}${r}${g}${g}${b}${b}`.toLowerCase();
  }

  if (clean.length === 6) {
    if (!/^[0-9a-fA-F]{6}$/.test(clean)) {
      throw new Error(`Cor hexadecimal de 6 dígitos contém caracteres inválidos: "${hex}"`);
    }
    return clean.toLowerCase();
  }

  throw new Error(
    `Formato de cor hexadecimal inválido: "${hex}". Esperado formato #RGB, #RRGGBB, RGB ou RRGGBB.`
  );
}

/**
 * Converte um componente de canal sRGB de 8 bits normalizado no intervalo [0.0, 1.0]
 * para luminosidade linear conforme especificado pela WCAG 2.1.
 *
 * Fórmula:
 * - Se c <= 0.03928: c / 12.92
 * - Se c > 0.03928: ((c + 0.055) / 1.055) ^ 2.4
 *
 * @param c Valor normalizado do canal de cor (0.0 a 1.0)
 * @returns Componente linearizado
 */
export function sRgbToLinear(c: number): number {
  return c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);
}

/**
 * Calcula a Luminância Relativa (L) de qualquer cor no espaço sRGB conforme WCAG 2.1.
 *
 * Fórmula:
 * L = 0.2126 * R_lin + 0.7152 * G_lin + 0.0722 * B_lin
 *
 * Onde 0.0 é o preto mais escuro e 1.0 é o branco mais claro.
 *
 * @param hex Código de cor hexadecimal (#RGB, #RRGGBB, etc.)
 * @returns Luminância relativa no intervalo [0.0, 1.0]
 */
export function relativeLuminance(hex: string): number {
  const normalized = normalizeHexColor(hex);
  const r8 = parseInt(normalized.slice(0, 2), 16);
  const g8 = parseInt(normalized.slice(2, 4), 16);
  const b8 = parseInt(normalized.slice(4, 6), 16);

  const rLin = sRgbToLinear(r8 / 255);
  const gLin = sRgbToLinear(g8 / 255);
  const bLin = sRgbToLinear(b8 / 255);

  return 0.2126 * rLin + 0.7152 * gLin + 0.0722 * bLin;
}

/**
 * Calcula a Razão de Contraste entre duas cores hexadecimais conforme WCAG 2.1.
 *
 * Fórmula:
 * Contraste = (L1 + 0.05) / (L2 + 0.05)
 * Onde L1 é a luminância relativa da cor mais clara e L2 da mais escura.
 *
 * A razão varia no intervalo de 1.0 (cores idênticas) a 21.0 (preto absoluto vs branco absoluto).
 * O resultado é retornado como número arredondado a duas casas decimais.
 *
 * @param fgHex Cor de primeiro plano (foreground)
 * @param bgHex Cor de fundo (background)
 * @returns Razão de contraste numérica no intervalo [1.0, 21.0]
 */
export function contrastRatio(fgHex: string, bgHex: string): number {
  const l1 = relativeLuminance(fgHex);
  const l2 = relativeLuminance(bgHex);
  const hi = Math.max(l1, l2);
  const lo = Math.min(l1, l2);
  const rawRatio = (hi + 0.05) / (lo + 0.05);
  return Number(rawRatio.toFixed(2));
}

/**
 * Níveis de conformidade WCAG 2.1
 */
export type WcagLevel = 'AA' | 'AAA';

/**
 * Tamanho / contexto tipográfico do elemento
 */
export type TextSize = 'normal' | 'large';

/**
 * Avalia se uma determinada razão de contraste satisfaz os critérios de conformidade da WCAG 2.1.
 *
 * - Nível AA: mínimo 4.5:1 para texto normal, 3.0:1 para texto grande / UI components
 * - Nível AAA: mínimo 7.0:1 para texto normal, 4.5:1 para texto grande
 *
 * @param ratio Razão de contraste numérica calculada
 * @param level Nível alvo ('AA' ou 'AAA', default 'AA')
 * @param size Contexto de texto ('normal' ou 'large', default 'normal')
 * @returns Booleano indicando se o critério é satisfeito
 */
export function meetsWcag(
  ratio: number,
  level: WcagLevel = 'AA',
  size: TextSize = 'normal'
): boolean {
  if (level === 'AAA') {
    return size === 'large' ? ratio >= 4.5 : ratio >= 7.0;
  }
  return size === 'large' ? ratio >= 3.0 : ratio >= 4.5;
}
