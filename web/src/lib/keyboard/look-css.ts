// プレビュー用: 解決済みの見た目（resolveLook の結果）を CSS に近似する（純粋関数）。
// 実機（SwiftUI）とは細部が異なる。docs/keyboard-style-spec.md §8.1。

import { resolveColor } from './look-color'
import type { Scheme } from './look-color'
import { dataUriBytes } from './look-assets'
import { sanitizeLook } from './look-resolve'
import type { ParsedColor } from './look-color'
import type { ResolvedLook } from './look-resolve'
import { BACKGROUND_PROPS, LOOK_PROPS, LOOK_STATE_INFO, METRICS_PROPS } from './types'
import type { Key, KeyRole, KeyboardLayout, StateName } from './types'

/** プレビューの配色（web/src/app.css の --fg / --key-bg ほかと同じ値。look-css.test.ts が突き合わせる）。 */
export interface Palette {
  fg: string
  muted: string
  line: string
  keyBg: string
  keyFnBg: string
  keyAccentBg: string
  keyAccentFg: string
  kbBg: string
}

export const PALETTES: Readonly<Record<Scheme, Palette>> = {
  light: {
    fg: '#1c1e26',
    muted: '#5d5e66',
    line: '#d9d3c5',
    keyBg: '#fdfcf9',
    keyFnBg: '#cbc6ba',
    keyAccentBg: '#2b43b0',
    keyAccentFg: '#ffffff',
    kbBg: '#dcd8ce'
  },
  dark: {
    fg: '#ece8df',
    muted: '#a6a7b0',
    line: '#2b303d',
    keyBg: '#4a4f5e',
    keyFnBg: '#2f333f',
    keyAccentBg: '#9db0ff',
    keyAccentFg: '#0b0e1c',
    kbBg: '#1f222b'
  }
}

/** パレットを CSS 変数（.phone に付ける）にする。 */
export function paletteVars(p: Palette): Record<string, string> {
  return {
    '--fg': p.fg,
    '--muted': p.muted,
    '--line': p.line,
    '--key-bg': p.keyBg,
    '--key-fn-bg': p.keyFnBg,
    '--key-accent-bg': p.keyAccentBg,
    '--key-accent-fg': p.keyAccentFg,
    '--kb-bg': p.kbBg
  }
}

export function toStyle(rec: Readonly<Record<string, string>>): string {
  return Object.entries(rec)
    .map(([k, v]) => `${k}:${v}`)
    .join(';')
}

/** トークンを `#RRGGBB`（@clear は null＝透明）にする。 */
function tokenHex(token: string, p: Palette): string | null {
  switch (token) {
    case '@label':
      return p.fg
    case '@secondaryLabel':
      return p.muted
    case '@key':
      return p.keyBg
    case '@function':
      return p.keyFnBg
    case '@accent':
      return p.keyAccentBg
    case '@separator':
      return p.line
    default:
      return null
  }
}

/** 解決済みの色（`#RRGGBBAA` / `@トークン`）を CSS の rgba() にする。alphaMul は不透明度への掛け算。 */
export function cssColor(color: string, p: Palette, alphaMul = 1): string {
  let hex: string
  if (color.startsWith('@')) {
    const t = tokenHex(color, p)
    if (t === null) return 'rgba(0,0,0,0)'
    hex = `${t}FF`
  } else hex = color
  const n = (i: number) => parseInt(hex.slice(i, i + 2), 16)
  const a = Math.round((n(7) / 255) * alphaMul * 1000) / 1000
  return `rgba(${n(1)},${n(3)},${n(5)},${a})`
}

const WEIGHTS: Record<string, string> = {
  ultraLight: '100',
  thin: '200',
  light: '300',
  regular: '400',
  medium: '500',
  semibold: '600',
  bold: '700',
  heavy: '800',
  black: '900'
}
const DESIGNS: Record<string, string> = {
  system: 'inherit',
  rounded: "ui-rounded, 'SF Pro Rounded', 'Hiragino Maru Gothic ProN', 'M PLUS Rounded 1c', sans-serif",
  monospaced: "ui-monospace, 'SF Mono', Menlo, Consolas, monospace",
  serif: "ui-serif, 'Hiragino Mincho ProN', 'Yu Mincho', Georgia, serif"
}

const num = (v: unknown): number | undefined => (typeof v === 'number' ? v : undefined)
const str = (v: unknown): string | undefined => (typeof v === 'string' ? v : undefined)

/** 既定の背景（役割ごと）。 */
function roleFill(role: KeyRole, p: Palette): string {
  return role === 'accent' ? p.keyAccentBg : role === 'function' ? p.keyFnBg : p.keyBg
}

export interface ImageSource {
  /** assets の data URI。 */
  uri: string
  /** 元の大きさ（px。tile のときだけ使う）。 */
  size?: { w: number; h: number }
}

export interface FaceCss {
  /** キーの顔（背景・枠・影・文字）。 */
  face: Record<string, string>
  /** 背景画像の層。無ければ null。 */
  image: Record<string, string> | null
}

export interface FaceOptions {
  palette: Palette
  image?: ImageSource
  reduceMotion?: boolean
}

/** 画像の層の CSS。uri は dataUriBytes で検査済みのものだけを使う（CSS に任意の文字列を入れない）。 */
function imageLayer(look: ResolvedLook, image: ImageSource | undefined, fillOpacity: number): Record<string, string> | null {
  if (typeof look.fillImage !== 'string' || !image || dataUriBytes(image.uri) === undefined) return null
  const fit = str(look.fillImageFit) ?? 'fill'
  const css: Record<string, string> = {
    'background-image': `url("${image.uri}")`,
    'background-position': 'center',
    'background-repeat': 'no-repeat',
    'background-size': 'cover',
    opacity: String(Math.round((num(look.fillImageOpacity) ?? 1) * fillOpacity * 1000) / 1000)
  }
  if (fit === 'fit') css['background-size'] = 'contain'
  else if (fit === 'tile' && image.size && Math.min(image.size.w, image.size.h) >= 8) {
    // 元の大きさ（1pt = 3px の @3x として扱う）で敷き詰める
    css['background-size'] = `${image.size.w / 3}px ${image.size.h / 3}px`
    css['background-repeat'] = 'repeat'
    css['background-position'] = 'top left'
  }
  return css
}

/** キー 1 つの顔の CSS。look は resolveLook の結果、role はキーの役割。 */
export function faceCss(look: ResolvedLook, role: KeyRole, opts: FaceOptions): FaceCss {
  const p = opts.palette
  const fillOpacity = num(look.fillOpacity) ?? 1
  const face: Record<string, string> = {
    inset: `${num(look.inset) ?? 0}px`,
    'background-color': cssColor(str(look.fill) ?? `${roleFill(role, p)}FF`, p, fillOpacity),
    'border-radius': `${num(look.cornerRadius) ?? 7}px`,
    color: cssColor(str(look.color) ?? (role === 'accent' ? `${p.keyAccentFg}FF` : '@label'), p)
  }

  // 枠線（内側）→ 影の順
  const shadows: string[] = []
  const bw = num(look.borderWidth) ?? 0
  if (bw > 0) shadows.push(`inset 0 0 0 ${bw}px ${cssColor(str(look.borderColor) ?? '@separator', p)}`)
  const sr = num(look.shadowRadius)
  if (sr === undefined) shadows.push('0 1px 0 rgba(0,0,0,0.3)')
  else if (sr > 0)
    shadows.push(`${num(look.shadowX) ?? 0}px ${num(look.shadowY) ?? 0}px ${sr}px ${cssColor(str(look.shadowColor) ?? '#000000FF', p, num(look.shadowOpacity) ?? 0.3)}`)
  face['box-shadow'] = shadows.length ? shadows.join(',') : 'none'

  const opacity = num(look.opacity)
  if (opacity !== undefined) face.opacity = String(opacity)
  const parts: string[] = []
  const dx = num(look.offsetX) ?? 0
  const dy = num(look.offsetY) ?? 0
  if (dx !== 0 || dy !== 0) parts.push(`translate(${dx}px,${dy}px)`)
  const scale = num(look.scale)
  if (scale !== undefined && scale !== 1) parts.push(`scale(${scale})`)
  if (parts.length) face.transform = parts.join(' ')
  const padding = num(look.padding)
  if (padding) face.padding = `${padding}px`

  const size = num(look.fontSize)
  if (size !== undefined) face['font-size'] = `${size}px`
  const weight = str(look.fontWeight)
  if (weight && WEIGHTS[weight]) face['font-weight'] = WEIGHTS[weight]
  const design = str(look.fontDesign)
  if (design && DESIGNS[design] && design !== 'system') face['font-family'] = DESIGNS[design]

  const ms = num(look.animationMs) ?? 0
  face.transition = ms > 0 && !opts.reduceMotion ? `all ${ms}ms ease-in-out` : 'none'
  return { face, image: imageLayer(look, opts.image, fillOpacity) }
}

/** キーボード全体の背景（layout.background）の解決。不正なプロパティは無視する（キーの look と同じ規則）。 */
export function resolveBackground(layout: KeyboardLayout, scheme: Scheme): ResolvedLook {
  const out: ResolvedLook = {}
  const raw: unknown = layout.background
  if (typeof raw !== 'object' || raw === null || Array.isArray(raw)) return out
  for (const [name, value] of Object.entries(sanitizeLook(raw, false))) {
    if (!(BACKGROUND_PROPS as readonly string[]).includes(name)) continue
    if (LOOK_PROPS[name].type === 'color') {
      const solid = resolveColor(value as ParsedColor, scheme)
      if (solid !== undefined) out[name] = solid
    } else out[name] = value as number | string
  }
  if (out.fillImage === 'none') delete out.fillImage
  return out
}

/** 背景の層（色と画像）。fill が無く画像も無ければ null（塗らない）。 */
export function backgroundCss(bg: ResolvedLook, opts: FaceOptions): FaceCss | null {
  const fillOpacity = num(bg.fillOpacity) ?? 1
  const image = imageLayer(bg, opts.image, fillOpacity)
  if (typeof bg.fill !== 'string' && !image) return null
  return { face: { 'background-color': typeof bg.fill === 'string' ? cssColor(bg.fill, opts.palette, fillOpacity) : 'transparent' }, image }
}

/** metrics の CSS（キー間・行間・外周）。範囲外・型違いは無視して既定（null）。 */
export function metricsCss(layout: KeyboardLayout): { keySpacing: string | null; rowSpacing: string | null; padding: string | null } {
  const raw: unknown = layout.metrics
  const m = typeof raw === 'object' && raw !== null && !Array.isArray(raw) ? (raw as Record<string, unknown>) : {}
  const pick = (k: keyof typeof METRICS_PROPS): string | null => {
    const v = m[k]
    const r = METRICS_PROPS[k]
    return typeof v === 'number' && Number.isFinite(v) && v >= r.min && v <= r.max ? `${v}px` : null
  }
  return { keySpacing: pick('keySpacing'), rowSpacing: pick('rowSpacing'), padding: pick('padding') }
}

// ---- プレビューの状態 -----------------------------------------------------------------------------------------

/**
 * キー 1 つに有効な状態。局所の状態（押下・離した直後・長押し・フリック中）は選択中のキー（選択が無ければ全キー）に、
 * 大域の状態（移動中・入力中・Shift・Caps）は全キーに適用する。
 */
export function statesForKey(key: Key, selectedIds: readonly string[], active: readonly StateName[]): StateName[] {
  const local = selectedIds.length === 0 || selectedIds.includes(key.id)
  return active.filter((s) => LOOK_STATE_INFO[s].scope === 'global' || local)
}

/** spaceDrag の起点キーの id（action: space のキー。無ければ最初のキー）。 */
export function spaceDragOrigin(layout: KeyboardLayout): string | undefined {
  const keys = layout.rows.flatMap((r) => r.keys).filter((k) => k.kind !== 'spacer')
  return (keys.find((k) => k.kind === 'action' && k.action === 'space') ?? keys[0])?.id
}
