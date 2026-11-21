import { describe, it, expect } from 'vitest';
import {
  contrastRatio,
  relativeLuminance,
  normalizeHexColor,
  sRgbToLinear,
  meetsWcag,
} from '../src/contrast.js';

describe('AnkiMBR v2 WCAG contrast math (Bundle Baseline)', () => {
  it('passes fixed dark mode flow badge (#0b1120 on #60a5fa)', () => {
    const ratio = contrastRatio('#0b1120', '#60a5fa');
    expect(ratio).toBeGreaterThanOrEqual(7.0);
    expect(ratio).toBe(7.41);
  });

  it('catches v1 broken dark mode flow badge (#ffffff on #6bb4fa)', () => {
    const ratio = contrastRatio('#ffffff', '#6bb4fa');
    expect(ratio).toBeLessThan(4.5);
    expect(ratio).toBe(2.2);
  });
});

describe('WCAG 2.1 Boundary & Limit Tests (Testes de Limites)', () => {
  it('calculates maximum contrast boundary (21.0:1) between pure black and pure white', () => {
    expect(contrastRatio('#000000', '#ffffff')).toBe(21.0);
    expect(contrastRatio('#ffffff', '#000000')).toBe(21.0);
  });

  it('calculates minimum contrast boundary (1.0:1) for identical colors', () => {
    expect(contrastRatio('#ffffff', '#ffffff')).toBe(1.0);
    expect(contrastRatio('#000000', '#000000')).toBe(1.0);
    expect(contrastRatio('#3b82f6', '#3b82f6')).toBe(1.0);
    expect(contrastRatio('#0b1120', '#0b1120')).toBe(1.0);
  });

  it('preserves commutative symmetry: contrastRatio(A, B) === contrastRatio(B, A)', () => {
    const pairs = [
      ['#0f172a', '#f8fafc'],
      ['#1e293b', '#94a3b8'],
      ['#e2e8f0', '#334155'],
      ['#60a5fa', '#0b1120'],
      ['#ef4444', '#fef2f2'],
    ] as const;

    for (const [fg, bg] of pairs) {
      expect(contrastRatio(fg, bg)).toBe(contrastRatio(bg, fg));
    }
  });

  it('normalizes 3-digit shorthand hex with equivalence to 6-digit hex', () => {
    expect(contrastRatio('#fff', '#000')).toBe(21.0);
    expect(contrastRatio('#000', '#fff')).toBe(21.0);
    expect(contrastRatio('#3b8', '#fff')).toBe(contrastRatio('#33bb88', '#ffffff'));
    expect(normalizeHexColor('#abc')).toBe('aabbcc');
    expect(normalizeHexColor('#FFF')).toBe('ffffff');
  });

  it('handles case-insensitivity uniformly', () => {
    expect(relativeLuminance('#FFFFFF')).toBe(relativeLuminance('#ffffff'));
    expect(relativeLuminance('#AbCdEf')).toBe(relativeLuminance('#abcdef'));
    expect(contrastRatio('#FFFFFF', '#000000')).toBe(contrastRatio('#ffffff', '#000000'));
  });

  it('accepts hex strings with or without leading hash "#"', () => {
    expect(contrastRatio('ffffff', '000000')).toBe(21.0);
    expect(contrastRatio('fff', '000')).toBe(21.0);
    expect(normalizeHexColor('1e293b')).toBe('1e293b');
  });

  it('trims leading and trailing whitespace safely', () => {
    expect(contrastRatio('  #ffffff  ', '  #000000  ')).toBe(21.0);
    expect(normalizeHexColor('   #3b82f6   ')).toBe('3b82f6');
  });

  it('computes exact relative luminance boundaries for primary and secondary sRGB colors', () => {
    expect(relativeLuminance('#000000')).toBe(0.0);
    expect(relativeLuminance('#ffffff')).toBe(1.0);

    // WCAG standard weights: R=0.2126, G=0.7152, B=0.0722
    expect(relativeLuminance('#ff0000')).toBeCloseTo(0.2126, 4);
    expect(relativeLuminance('#00ff00')).toBeCloseTo(0.7152, 4);
    expect(relativeLuminance('#0000ff')).toBeCloseTo(0.0722, 4);
  });

  it('exercises sRGB piecewise linearization transition around threshold (c <= 0.03928)', () => {
    // 10 / 255 = 0.03921568... (<= 0.03928: linear slope c / 12.92)
    const cLow = 10 / 255;
    expect(cLow).toBeLessThanOrEqual(0.03928);
    expect(sRgbToLinear(cLow)).toBeCloseTo(cLow / 12.92, 6);

    // 11 / 255 = 0.04313725... (> 0.03928: gamma curve ((c + 0.055) / 1.055) ^ 2.4)
    const cHigh = 11 / 255;
    expect(cHigh).toBeGreaterThan(0.03928);
    expect(sRgbToLinear(cHigh)).toBeCloseTo(Math.pow((cHigh + 0.055) / 1.055, 2.4), 6);
  });

  it('evaluates WCAG 2.1 AA and AAA compliance thresholds correctly', () => {
    // AA normal requires >= 4.5
    expect(meetsWcag(4.5, 'AA', 'normal')).toBe(true);
    expect(meetsWcag(4.49, 'AA', 'normal')).toBe(false);

    // AA large / UI components requires >= 3.0
    expect(meetsWcag(3.0, 'AA', 'large')).toBe(true);
    expect(meetsWcag(2.99, 'AA', 'large')).toBe(false);

    // AAA normal requires >= 7.0
    expect(meetsWcag(7.0, 'AAA', 'normal')).toBe(true);
    expect(meetsWcag(6.99, 'AAA', 'normal')).toBe(false);

    // AAA large requires >= 4.5
    expect(meetsWcag(4.5, 'AAA', 'large')).toBe(true);
    expect(meetsWcag(4.49, 'AAA', 'large')).toBe(false);
  });
});

describe('Error Handling Tests (Testes de Erros e Validação)', () => {
  it('throws Error for invalid hex length', () => {
    const invalidLengths = ['#', '#f', '#12', '#1234', '#12345', '#1234567', '#12345678'];
    for (const hex of invalidLengths) {
      expect(() => normalizeHexColor(hex)).toThrowError(/Formato de cor hexadecimal inválido/);
      expect(() => relativeLuminance(hex)).toThrowError(/Formato de cor hexadecimal inválido/);
      expect(() => contrastRatio(hex, '#ffffff')).toThrowError(/Formato de cor hexadecimal inválido/);
    }
  });

  it('throws Error for non-hex characters in 3-digit and 6-digit inputs', () => {
    expect(() => normalizeHexColor('#ggg')).toThrowError(/caracteres inválidos/);
    expect(() => normalizeHexColor('#gggggg')).toThrowError(/caracteres inválidos/);
    expect(() => normalizeHexColor('#12z456')).toThrowError(/caracteres inválidos/);
    expect(() => normalizeHexColor('#xyz')).toThrowError(/caracteres inválidos/);
  });

  it('throws Error for empty or whitespace-only strings', () => {
    expect(() => normalizeHexColor('')).toThrowError(/Formato de cor hexadecimal inválido/);
    expect(() => normalizeHexColor('   ')).toThrowError(/Formato de cor hexadecimal inválido/);
    expect(() => normalizeHexColor('#')).toThrowError(/Formato de cor hexadecimal inválido/);
  });

  it('throws TypeError for non-string inputs', () => {
    // @ts-expect-error test runtime validation of non-string inputs
    expect(() => normalizeHexColor(null)).toThrow(TypeError);
    // @ts-expect-error test runtime validation of non-string inputs
    expect(() => normalizeHexColor(undefined)).toThrow(TypeError);
    // @ts-expect-error test runtime validation of non-string inputs
    expect(() => normalizeHexColor(123456)).toThrow(TypeError);
    // @ts-expect-error test runtime validation of non-string inputs
    expect(() => normalizeHexColor({})).toThrow(TypeError);
  });

  it('throws Error when named colors or CSS functional colors are provided', () => {
    expect(() => normalizeHexColor('black')).toThrowError(/Formato de cor hexadecimal inválido/);
    expect(() => normalizeHexColor('rgb(0,0,0)')).toThrowError(/Formato de cor hexadecimal inválido/);
    expect(() => normalizeHexColor('hsl(0, 100%, 50%)')).toThrowError(/Formato de cor hexadecimal inválido/);
  });
});
