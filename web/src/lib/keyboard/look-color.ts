// 色の解析と、ライト／ダークの解決（docs/keyboard-style-spec.md §2.2）。
// Swift の LookColor（frontend-ios-spike/Shared/KeyLook.swift）と同じ規則。純粋関数だけを置く。

import { LOOK_TOKENS } from './types'

/** 解析済みの 1 色: `#RRGGBBAA`（大文字 16 進 8 桁）か `@トークン`。 */
export type SolidColor = string

/** 解析済みの色の指定。1 色か、ライト／ダークで別の色（片方だけも可）。 */
export type ParsedColor = SolidColor | { light?: SolidColor; dark?: SolidColor }

export type Scheme = 'light' | 'dark'

const HEX = /^#([0-9a-fA-F]{3}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})$/

/** `#RGB` / `#RRGGBB` / `#RRGGBBAA` / `@トークン` を正規化する。不正なら undefined。 */
export function parseSolidColor(text: unknown): SolidColor | undefined {
  if (typeof text !== 'string') return undefined
  if (text.startsWith('@')) {
    const name = text.slice(1)
    return (LOOK_TOKENS as readonly string[]).includes(name) ? text : undefined
  }
  const m = HEX.exec(text)
  if (!m) return undefined
  const h = m[1]
  if (h.length === 3) {
    const [r, g, b] = [...h]
    return `#${r}${r}${g}${g}${b}${b}FF`.toUpperCase()
  }
  return (h.length === 6 ? `#${h}FF` : `#${h}`).toUpperCase()
}

/**
 * 色の指定を解析する。不正（形式違い・未知のトークン・light/dark 以外のキー・空・片側だけ不正）は
 * 全体を undefined にする（そのプロパティを無視して下位の値に落とす）。
 */
export function parseColor(value: unknown): ParsedColor | undefined {
  if (typeof value === 'string') return parseSolidColor(value)
  if (typeof value !== 'object' || value === null || Array.isArray(value)) return undefined
  const keys = Object.keys(value)
  if (keys.length === 0) return undefined
  const out: { light?: SolidColor; dark?: SolidColor } = {}
  for (const k of keys) {
    if (k !== 'light' && k !== 'dark') return undefined
    const c = parseSolidColor((value as Record<string, unknown>)[k])
    if (c === undefined) return undefined
    out[k] = c
  }
  return out
}

/** 外観に応じた 1 色。片方だけの指定で、書いていない側の外観では undefined（未指定扱い）。 */
export function resolveColor(color: ParsedColor, scheme: Scheme): SolidColor | undefined {
  return typeof color === 'string' ? color : color[scheme]
}
