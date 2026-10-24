// 色入力の編集用の変換（純粋関数）。色の解析・解決は look-color.ts。

import { parseSolidColor } from './look-color'
import type { Color } from './types'

/** `#RRGGBB[AA]`（解析可能な 16 進）を、`<input type="color">` 用の `#rrggbb` と不透明度（0〜1）に分ける。トークン・不正は undefined。 */
export function hexToRgbAlpha(text: string | undefined): { rgb: string; alpha: number } | undefined {
  const c = parseSolidColor(text)
  if (c === undefined || c.startsWith('@')) return undefined
  return { rgb: c.slice(0, 7).toLowerCase(), alpha: Math.round((parseInt(c.slice(7, 9), 16) / 255) * 100) / 100 }
}

/** `#rrggbb` と不透明度から `#RRGGBB`（不透明）または `#RRGGBBAA` を作る。 */
export function rgbAlphaToHex(rgb: string, alpha: number): string {
  const body = rgb.slice(0, 7).toUpperCase()
  const a = Math.round(Math.min(1, Math.max(0, alpha)) * 255)
  return a === 255 ? body : `${body}${a.toString(16).padStart(2, '0').toUpperCase()}`
}

export type ColorMode = 'single' | 'pair'

export const colorMode = (c: Color | undefined): ColorMode => (typeof c === 'object' && c !== null ? 'pair' : 'single')

/** 単色 → ライト／ダーク別（両方に同じ色）。すでに別々なら変えない。 */
export function splitColor(c: Color | undefined, fallback: string): Color {
  if (typeof c === 'string') return { light: c, dark: c }
  if (c && (c.light || c.dark)) return c
  return { light: fallback, dark: fallback }
}

/** ライト／ダーク別 → 単色（ライトを優先）。 */
export function mergeColor(c: Color | undefined, fallback: string): string {
  if (typeof c === 'string') return c
  return c?.light ?? c?.dark ?? fallback
}
