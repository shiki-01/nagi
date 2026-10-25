import { describe, expect, it } from 'vitest'
import Ajv2020 from 'ajv/dist/2020'
import schema from '../../../../config/keyboard/layout.schema.json'
import { migrateText } from '../../../scripts/migrate-layout-v2.mjs'
import { dataUriBytes, assetsUsage, canAddAsset } from './look-assets'
import { parseColor, parseSolidColor, resolveColor } from './look-color'
import { checkLookValue, lookContent, releasedHoldMs, resolveLook, sanitizeLook, transitionMs } from './look-resolve'
import type { ResolvedLook } from './look-resolve'
import { sampleLayout } from './ops'
import { presetResult } from './presets'
import {
  ASSET_LIMITS,
  BACKGROUND_PROPS,
  LOOK_CONTENT_PROPS,
  LOOK_LIMITS,
  LOOK_PROPS,
  LOOK_STATES,
  METRICS_PROPS,
  STATE_PRECEDENCE,
  TOOLBAR_ROW
} from './types'
import type { Key, KeyboardLayout, StateName } from './types'
import { describePath, errorsOf, isValidLayout, parseLayoutJSON, validateLayout, warningsOf } from './validate'

const ajv = new Ajv2020({ strict: false })
const schemaValidate = ajv.compile(schema)

// ---- 共有テストベクタ（config/keyboard/test-looks/*.json。Swift の KeyLookVectorTests と同じファイル） ----------

interface VectorCase {
  name: string
  key: string
  states?: StateName[]
  origin?: string
  scheme?: 'light' | 'dark'
  expect: ResolvedLook
  expectHoldMs?: number
  expectContent?: { label: string | null; icon: string | null; hidesKeyIcon: boolean }
}
interface VectorFile {
  description: string
  layout: KeyboardLayout
  cases: VectorCase[]
  transitions?: { name: string; from: number | null; to: number | null; reduceMotion: boolean; expect: number }[]
}

const vectorModules = import.meta.glob('../../../../config/keyboard/test-looks/*.json', { eager: true, import: 'default' }) as Record<string, VectorFile>
const vectors = Object.entries(vectorModules).sort(([a], [b]) => a.localeCompare(b))

function findKey(layout: KeyboardLayout, id: string): { rowIndex: number; key: Key } {
  const tb = layout.toolbar?.keys.find((k) => k.id === id)
  if (tb) return { rowIndex: TOOLBAR_ROW, key: tb }
  for (const [rowIndex, row] of layout.rows.entries()) {
    const key = row.keys.find((k) => k.id === id)
    if (key) return { rowIndex, key }
  }
  throw new Error(`key ${id} not found`)
}

describe('shared test vectors (config/keyboard/test-looks)', () => {
  it('all vector files are present', () => {
    const names = vectors.map(([path]) => path.split('/').pop())
    for (const f of ['defaults', 'hierarchy', 'invalid', 'colors', 'states', 'rules', 'image', 'toolbar'])
      expect(names).toContain(`resolve-${f}.json`)
  })
  for (const [path, vf] of vectors) {
    const file = path.split('/').pop()
    describe(file!, () => {
      it('layout has no errors (warnings allowed)', () => {
        expect(errorsOf(validateLayout(vf.layout))).toEqual([])
      })
      for (const c of vf.cases) {
        it(c.name, () => {
          const { rowIndex, key } = findKey(vf.layout, c.key)
          const ctx = { states: c.states, origin: c.origin, scheme: c.scheme }
          const resolved = resolveLook(vf.layout, rowIndex, key, ctx)
          expect(resolved).toEqual(c.expect)
          if (c.expectHoldMs !== undefined) expect(releasedHoldMs(vf.layout, key)).toBe(c.expectHoldMs)
          if (c.expectContent) expect(lookContent(resolved)).toEqual(c.expectContent)
        })
      }
      for (const t of vf.transitions ?? []) {
        it(`transition: ${t.name}`, () => {
          const look = (ms: number | null): ResolvedLook => (ms === null ? {} : { animationMs: ms })
          expect(transitionMs(look(t.from), look(t.to), t.reduceMotion)).toBe(t.expect)
        })
      }
    })
  }
})

// ---- 色 ---------------------------------------------------------------------------------------------------------

describe('look colors', () => {
  it('normalizes hex forms and tokens', () => {
    expect(parseSolidColor('#abc')).toBe('#AABBCCFF')
    expect(parseSolidColor('#1a2B3c')).toBe('#1A2B3CFF')
    expect(parseSolidColor('#11223344')).toBe('#11223344')
    expect(parseSolidColor('@accent')).toBe('@accent')
  })
  it('rejects malformed colors', () => {
    for (const bad of ['', '#', '#12', '#1234', '#12345', '#1234567', '#123456789', '#GG0', 'abc', '@unknown', '@', '@Label', 'red', ' #fff', '#fff ', '#fff\n', 5, null, undefined])
      expect(parseSolidColor(bad), String(bad)).toBeUndefined()
  })
  it('light/dark pairs', () => {
    expect(parseColor({ light: '#fff', dark: '@key' })).toEqual({ light: '#FFFFFFFF', dark: '@key' })
    expect(parseColor({ dark: '#000' })).toEqual({ dark: '#000000FF' })
    for (const bad of [{}, { light: 'nope' }, { light: '#fff', dark: 'nope' }, { light: '#fff', extra: '#000' }, { light: 5 }, { light: { light: '#fff' } }, [], null, 5])
      expect(parseColor(bad), JSON.stringify(bad)).toBeUndefined()
  })
  it('one-sided pairs are unspecified in the other scheme', () => {
    const pair = parseColor({ light: '@key' })!
    expect(resolveColor(pair, 'light')).toBe('@key')
    expect(resolveColor(pair, 'dark')).toBeUndefined()
    expect(resolveColor('@accent', 'dark')).toBe('@accent')
  })
})

// ---- 範囲 ---------------------------------------------------------------------------------------------------------

describe('look sanitize', () => {
  it('every numeric property accepts its bounds and rejects just outside (no clamping)', () => {
    let checked = 0
    for (const [name, spec] of Object.entries(LOOK_PROPS)) {
      if (spec.type !== 'number') continue
      checked++
      expect(sanitizeLook({ [name]: spec.min }, false), `${name} min`).toEqual({ [name]: spec.min })
      expect(sanitizeLook({ [name]: spec.max }, false), `${name} max`).toEqual({ [name]: spec.max })
      for (const out of [spec.min - 0.01, spec.max + 0.01, '5', null, true])
        expect(sanitizeLook({ [name]: out }, false), `${name} ${String(out)}`).toEqual({})
    }
    expect(checked).toBe(16)
  })
  it('label / icon only when content is allowed', () => {
    expect(sanitizeLook({ label: 'a', icon: 'sf:globe', scale: 1 }, false)).toEqual({ scale: 1 })
    expect(sanitizeLook({ label: 'a', icon: 'sf:globe', scale: 1 }, true)).toEqual({ label: 'a', icon: 'sf:globe', scale: 1 })
  })
  it('checks enums, image refs, label and icon', () => {
    const ok = (name: string, v: unknown) => checkLookValue(name, v).ok
    expect(ok('fontWeight', 'ultraLight')).toBe(true)
    expect(ok('fontWeight', 'Bold')).toBe(false)
    expect(ok('fontDesign', 'rounded')).toBe(true)
    expect(ok('fillImageFit', 'stretch')).toBe(false)
    expect(ok('fillImage', 'wood')).toBe(true)
    expect(ok('fillImage', 'none')).toBe(true)
    expect(ok('fillImage', 'Bad Id')).toBe(false)
    expect(ok('fillImage', '')).toBe(false)
    expect(ok('label', 'あ'.repeat(16))).toBe(true)
    expect(ok('label', 'あ'.repeat(17))).toBe(false)
    expect(ok('label', '')).toBe(false)
    expect(ok('icon', 'sf:globe')).toBe(true)
    expect(ok('icon', 'iconify:mdi:home')).toBe(true)
    expect(ok('icon', 'iconify:mdi')).toBe(false)
    expect(ok('icon', 'sf:')).toBe(false)
    expect(ok('icon', '')).toBe(false)
    expect(ok('icon', 'a'.repeat(65))).toBe(false)
  })
  it('ignores prototype keys', () => {
    expect(sanitizeLook(JSON.parse('{"constructor":1,"__proto__":{"x":1},"toString":2}'), true)).toEqual({})
  })
})

// ---- 解決（ベクタ以外の補足） -----------------------------------------------------------------------------------------

describe('resolveLook details', () => {
  it('state precedence list is the documented one', () => {
    expect([...STATE_PRECEDENCE]).toEqual(['composing', 'capsLock', 'shift', 'released', 'pressed', 'spaceDrag', 'flicking', 'longPress'])
    expect([...STATE_PRECEDENCE].sort()).toEqual([...LOOK_STATES].sort())
  })
  it('does not mutate the layout', () => {
    const l = sampleLayout()
    l.look = { opacity: 0.5 }
    l.rules = [{ when: 'pressed', look: { scale: 0.9 } }]
    const before = JSON.stringify(l)
    resolveLook(l, 0, l.rows[0].keys[0], { states: ['pressed'] })
    expect(JSON.stringify(l)).toBe(before)
  })
  it('a layout without look settings resolves to nothing', () => {
    const l = sampleLayout()
    for (const [ri, row] of l.rows.entries()) for (const k of row.keys) expect(resolveLook(l, ri, k)).toEqual({})
  })
})

// ---- 検証（warning と error） ---------------------------------------------------------------------------------------

type Mut = (l: any) => void
const issuesOf = (f: Mut) => {
  const l = sampleLayout() as any
  f(l)
  return validateLayout(l)
}
const warn = (f: Mut) => warningsOf(issuesOf(f))
const err = (f: Mut) => errorsOf(issuesOf(f))

describe('validate looks', () => {
  it('sample layout still has no issues', () => expect(validateLayout(sampleLayout())).toEqual([]))
  it('warnings: value problems are located and do not block', () => {
    const w = warn((l) => {
      l.look = { fontSize: 99, mystery: 1, fill: 'red', label: 'x' }
      l.rows[0].keys[1].states = { pressed: { opacity: 2 }, bogus: {} }
    })
    expect(w.map((e) => e.path).sort()).toEqual(['look.fill', 'look.fontSize', 'look.label', 'look.mystery', 'rows[0].keys[1].states.bogus', 'rows[0].keys[1].states.pressed.opacity'].sort())
    expect(w.every((e) => e.severity === 'warning')).toBe(true)
    expect(errorsOf(issuesOf((l) => (l.look = { fontSize: 99 })))).toEqual([])
  })
  it('isValidLayout / parseLayoutJSON ignore warnings', () => {
    const l = sampleLayout() as any
    l.look = { fontSize: 99 }
    expect(isValidLayout(l)).toBe(true)
    const r = parseLayoutJSON(JSON.stringify(l))
    expect(r.ok && r.warnings.length).toBe(1)
  })
  it('items that are not objects are warnings', () => {
    for (const f of [
      (l: any) => (l.look = 5),
      (l: any) => (l.roles = []),
      (l: any) => (l.rules = {}),
      (l: any) => (l.metrics = 3),
      (l: any) => (l.background = 'x'),
      (l: any) => (l.rows[0].look = 5),
      (l: any) => (l.rows[0].keys[0].look = []),
      (l: any) => (l.rows[0].keys[0].states = 'x'),
      (l: any) => (l.rows[0].keys[0].states = { pressed: 1 })
    ]) {
      expect(err(f)).toEqual([])
      expect(warn(f).length).toBe(1)
    }
  })
  it('rules: when / target / look problems are warnings, counts are errors', () => {
    expect(warn((l) => (l.rules = [{ when: 'bogus', look: {} }])).map((e) => e.path)).toEqual(['rules[0].when'])
    expect(warn((l) => (l.rules = [{ when: 'pressed', target: 'everyone', look: {} }])).map((e) => e.path)).toEqual(['rules[0].target'])
    expect(warn((l) => (l.rules = [{ when: 'pressed', target: { keys: ['a'], roles: ['function'] }, look: {} }])).map((e) => e.path)).toEqual(['rules[0].target'])
    expect(warn((l) => (l.rules = [{ when: 'pressed', target: { roles: ['bogus'] }, look: {} }])).map((e) => e.path)).toEqual(['rules[0].target.roles'])
    expect(warn((l) => (l.rules = [{ when: 'pressed', look: { scale: 9 } }])).map((e) => e.path)).toEqual(['rules[0].look.scale'])
    expect(warn((l) => (l.rules = [{ when: 'released', holdMs: 5000, look: {} }])).map((e) => e.path)).toEqual(['rules[0].holdMs'])
    expect(warn((l) => (l.rules = [{ when: 'pressed', look: { label: 'x', icon: 'sf:star' } }]))).toEqual([])
    const rule = { when: 'pressed', look: {} }
    expect(err((l) => (l.rules = Array.from({ length: LOOK_LIMITS.rulesMax }, () => rule)))).toEqual([])
    expect(err((l) => (l.rules = Array.from({ length: LOOK_LIMITS.rulesMax + 1 }, () => rule))).map((e) => e.path)).toEqual(['rules'])
    const keys = (n: number) => Array.from({ length: n }, (_, i) => `k${i}`)
    expect(err((l) => (l.rules = [{ when: 'pressed', target: { keys: keys(LOOK_LIMITS.targetKeysMax) }, look: {} }]))).toEqual([])
    expect(err((l) => (l.rules = [{ when: 'pressed', target: { keys: keys(LOOK_LIMITS.targetKeysMax + 1) }, look: {} }])).map((e) => e.path)).toEqual(['rules[0].target.keys'])
  })
  it('metrics and background', () => {
    expect(warn((l) => (l.metrics = { keySpacing: 8, rowSpacing: 9, mystery: 1 })).map((e) => e.path).sort()).toEqual(['metrics.mystery', 'metrics.rowSpacing'])
    expect(warn((l) => (l.background = { fill: '@clear', opacity: 0.5, scale: 2 })).map((e) => e.path).sort()).toEqual(['background.opacity', 'background.scale'])
    expect([...BACKGROUND_PROPS]).toEqual(['fill', 'fillOpacity', 'fillImage', 'fillImageFit', 'fillImageOpacity'])
  })
  it('popups cannot have look', () => {
    expect(err((l) => (l.rows[0].keys[0].longPress = [{ label: 'x', kind: 'text', text: 'x', look: {} }])).length).toBe(1)
  })
  it('describePath handles rules', () => {
    expect(describePath('rules[2].look.scale')).toBe('rules 3 番目.look.scale')
  })
})

describe('validate assets (errors)', () => {
  const png = 'data:image/png;base64,iVBORw0KGgo='
  const uri = (bytes: number, mime = 'image/png') => `data:${mime};base64,${btoa('\0'.repeat(bytes))}`
  const assetErrors = (assets: unknown) => err((l) => (l.assets = assets)).map((e) => e.path)
  it('accepts valid assets and counts bytes', () => {
    expect(assetErrors({ wood: png, glow: uri(1024, 'image/jpeg') })).toEqual([])
    expect(assetErrors({ a: uri(ASSET_LIMITS.bytesMax) })).toEqual([])
    expect(assetErrors({})).toEqual([])
    expect(dataUriBytes(png)).toBe(8)
    expect(dataUriBytes(uri(1000))).toBe(1000)
    expect(dataUriBytes(uri(1001))).toBe(1001)
    expect(dataUriBytes(uri(1002))).toBe(1002)
  })
  it('rejects bad count, id, mime, base64 and sizes', () => {
    expect(assetErrors(Object.fromEntries([1, 2, 3, 4, 5].map((i) => [`a${i}`, png])))).toEqual(['assets'])
    expect(assetErrors({ 'Bad Id': png })).toEqual(['assets.Bad Id'])
    expect(assetErrors({ ['a'.repeat(33)]: png })).toHaveLength(1)
    expect(assetErrors({ a: 'data:image/gif;base64,R0lGODlh' })).toEqual(['assets.a'])
    expect(assetErrors({ a: 'data:image/png;base64,' })).toEqual(['assets.a'])
    expect(assetErrors({ a: 'data:image/png;base64,!!!!' })).toEqual(['assets.a'])
    expect(assetErrors({ a: 'data:image/png;base64,AAAAA' })).toEqual(['assets.a'])
    expect(assetErrors({ a: 'https://example.com/a.png' })).toEqual(['assets.a'])
    expect(assetErrors({ a: 5 })).toEqual(['assets.a'])
    expect(assetErrors({ a: uri(ASSET_LIMITS.bytesMax + 1) })).toEqual(['assets.a'])
    expect(assetErrors([png])).toEqual(['assets'])
    const each = 200 * 1024
    expect(assetErrors({ a: uri(each), b: uri(each), c: uri(each) })).toEqual([])
    expect(assetErrors({ a: uri(each), b: uri(each), c: uri(each), d: uri(each) })).toEqual(['assets'])
  })
  it('usage and addability', () => {
    expect(assetsUsage({ a: uri(1000), b: 5 })).toBe(1000)
    expect(canAddAsset({}, 1000)).toBeUndefined()
    expect(canAddAsset({}, ASSET_LIMITS.bytesMax + 1)).toBeTruthy()
    expect(canAddAsset({ a: png, b: png, c: png, d: png }, 10)).toBeTruthy()
    expect(canAddAsset({ a: uri(200 * 1024), b: uri(200 * 1024), c: uri(200 * 1024) }, 200 * 1024)).toBeTruthy()
  })
})

// ---- スキーマとの一致（warning もスキーマ違反として数える） ------------------------------------------------------------

const example1 = {
  schema: 2, id: 'soft-flick', name: 'やわらか 12キー', mode: 'kana',
  metrics: { keySpacing: 5, rowSpacing: 5 },
  look: { cornerRadius: 12, fontDesign: 'rounded', shadowRadius: 2, shadowY: 1, shadowOpacity: 0.2 },
  roles: {
    normal: { fill: { light: '#FFFFFF', dark: '#3A3A3C' } },
    function: { fill: { light: '#D9DCE3', dark: '#2C2C2E' }, fontWeight: 'medium' },
    accent: { fill: '@accent' }
  },
  rows: [{ keys: [
    { id: 'l', width: 0.7, label: '←', kind: 'action', action: 'moveLeft', role: 'function' },
    { id: 'a', kind: 'flick', kana: 'あ', label: 'あ' },
    { id: 'bs', width: 0.85, label: '⌫', kind: 'action', action: 'backspace', role: 'function' }
  ] }]
}
const example2 = {
  schema: 2, id: 'qwerty-glow', name: 'QWERTY 光る押下', mode: 'alpha',
  rules: [
    { when: 'pressed', target: 'all', look: { scale: 0.94, fillOpacity: 1, animationMs: 80 } },
    { when: 'released', target: 'all', holdMs: 150, look: { animationMs: 150 } }
  ],
  rows: [{ keys: [
    { id: 'q', kind: 'text', text: 'q', shift: 'Q' },
    {
      id: 'w', kind: 'text', text: 'w', shift: 'W',
      look: { fill: { light: '#FFF4D6', dark: '#4A3F1F' }, borderWidth: 1, borderColor: '#E0B341' },
      states: {
        pressed: { fill: '#E0B341', color: '#000000' },
        longPress: { scale: 1.08, shadowRadius: 8, shadowOpacity: 0.5, shadowColor: '#E0B341' }
      },
      longPress: [{ label: 'ŵ', kind: 'text', text: 'ŵ' }, { label: 'ẃ', kind: 'text', text: 'ẃ' }]
    }
  ] }]
}
const example3 = {
  schema: 2, id: 'qwerty-alpha-cursor', name: 'QWERTY 英字（移動中の表示つき）', mode: 'alpha',
  rules: [{ when: 'spaceDrag', target: 'others', look: { opacity: 0.35, animationMs: 120 } }],
  rows: [{ keys: [
    { id: 'mode', width: 1.2, label: 'あ', kind: 'action', action: 'modeSwitch', role: 'function' },
    { id: 'sp', width: 4.4, label: '空白', kind: 'action', action: 'space',
      states: { spaceDrag: { label: '移動', opacity: 0.7, fontWeight: 'semibold', animationMs: 120 } } },
    { id: 'en', width: 1.6, label: '改行', kind: 'action', action: 'enter', role: 'function' }
  ] }]
}
const everything = {
  schema: 2, id: 'all-props', name: 'all', mode: 'alpha',
  look: {
    fontSize: 8, fontWeight: 'black', fontDesign: 'monospaced', color: '#fff', fill: '@clear', fillOpacity: 0, fillImage: 'wood',
    fillImageFit: 'tile', fillImageOpacity: 1, cornerRadius: 24, borderWidth: 4, borderColor: { light: '@separator' },
    shadowColor: '#00000080', shadowRadius: 16, shadowX: -8, shadowY: 8, shadowOpacity: 1, opacity: 1, scale: 0.5,
    offsetX: -20, offsetY: 20, inset: 12, padding: 16, animationMs: 1000
  },
  background: { fill: '#112233', fillOpacity: 0.5, fillImage: 'wood', fillImageFit: 'fit', fillImageOpacity: 0.5 },
  assets: { wood: 'data:image/jpeg;base64,/9j/4AAQ' },
  rules: [
    { when: 'pressed', target: { keys: ['a', 'b'] }, look: { label: 'x', icon: 'iconify:mdi:home' } },
    { when: 'spaceDrag', target: { roles: ['function', 'accent'] }, holdMs: 0, look: {} },
    { when: 'composing', target: { actions: ['space'] }, look: { icon: 'shift' } }
  ],
  rows: [{ look: { opacity: 0.9 }, keys: [{ id: 'a', kind: 'text', text: 'a', states: { released: { holdMs: 2000, animationMs: 5 }, capsLock: { fill: '#fff' } } }] }]
}

describe('looks: schema agreement with ajv', () => {
  const good: unknown[] = [example1, example2, example3, everything]
  const bad = (): { name: string; value: unknown }[] => {
    const mk = (name: string, f: (l: any) => void) => {
      const l = structuredClone(everything) as any
      f(l)
      return { name, value: l }
    }
    return [
      mk('fontSize too big', (l) => (l.look.fontSize = 41)),
      mk('fontSize string', (l) => (l.look.fontSize = '12')),
      mk('fontWeight unknown', (l) => (l.look.fontWeight = 'extra')),
      mk('unknown property', (l) => (l.look.mystery = 1)),
      mk('label in look', (l) => (l.look.label = 'x')),
      mk('icon in look', (l) => (l.look.icon = 'sf:star')),
      mk('label in role', (l) => (l.roles = { function: { label: 'x' } })),
      mk('bad color', (l) => (l.look.fill = 'red')),
      mk('bad color pair', (l) => (l.look.fill = { light: '#fff', extra: '#000' })),
      mk('empty color pair', (l) => (l.look.fill = {})),
      mk('bad token', (l) => (l.look.fill = '@nope')),
      mk('bad image ref', (l) => (l.look.fillImage = 'Bad Id')),
      mk('bad fit', (l) => (l.look.fillImageFit = 'stretch')),
      mk('role unknown in roles', (l) => (l.roles = { bogus: {} })),
      mk('look not object', (l) => (l.look = 5)),
      mk('row look not object', (l) => (l.rows[0].look = 5)),
      mk('state unknown', (l) => (l.rows[0].keys[0].states.bogus = {})),
      mk('state label too long', (l) => (l.rows[0].keys[0].states.capsLock = { label: 'あ'.repeat(17) })),
      mk('state icon bad', (l) => (l.rows[0].keys[0].states.capsLock = { icon: 'iconify:mdi' })),
      mk('holdMs out of range', (l) => (l.rows[0].keys[0].states.released.holdMs = 2001)),
      mk('rule when unknown', (l) => (l.rules[0].when = 'bogus')),
      mk('rule without look', (l) => delete l.rules[0].look),
      mk('rule extra field', (l) => (l.rules[0].extra = 1)),
      mk('rule target string', (l) => (l.rules[0].target = 'everyone')),
      mk('rule target two kinds', (l) => (l.rules[0].target = { keys: ['a'], roles: ['function'] })),
      mk('rule target unknown role', (l) => (l.rules[1].target = { roles: ['bogus'] })),
      mk('rule target keys > 24', (l) => (l.rules[0].target = { keys: Array.from({ length: 25 }, (_, i) => `k${i}`) })),
      mk('rules > 32', (l) => (l.rules = Array.from({ length: 33 }, () => ({ when: 'pressed', look: {} })))),
      mk('rules not array', (l) => (l.rules = {})),
      mk('metrics out of range', (l) => (l.metrics = { keySpacing: 9 })),
      mk('metrics unknown', (l) => (l.metrics = { gap: 1 })),
      mk('background opacity (not allowed)', (l) => (l.background.opacity = 0.5)),
      mk('background label', (l) => (l.background.label = 'x')),
      mk('assets > 4', (l) => (l.assets = Object.fromEntries([1, 2, 3, 4, 5].map((i) => [`a${i}`, 'data:image/png;base64,iVBORw0KGgo=']))) ),
      mk('assets bad id', (l) => (l.assets = { 'Bad Id': 'data:image/png;base64,iVBORw0KGgo=' })),
      mk('assets bad mime', (l) => (l.assets = { a: 'data:image/gif;base64,R0lGODlh' })),
      mk('assets not data uri', (l) => (l.assets = { a: 'https://example.com/a.png' })),
      mk('assets value not string', (l) => (l.assets = { a: 5 })),
      mk('assets empty payload', (l) => (l.assets = { a: 'data:image/png;base64,' })),
      mk('popup with look', (l) => (l.rows[0].keys[0].longPress = [{ label: 'x', kind: 'text', text: 'x', look: {} }])),
      mk('old style name', (l) => (l.rows[0].keys[0].style = 'function')),
      mk('schema 1', (l) => (l.schema = 1))
    ]
  }
  it('good inputs pass both', () => {
    for (const g of good) {
      expect(schemaValidate(g), JSON.stringify(schemaValidate.errors)).toBe(true)
      expect(validateLayout(g)).toEqual([])
    }
  })
  it('bad inputs fail both (warnings count as schema violations)', () => {
    for (const { name, value } of bad()) {
      expect(schemaValidate(value), `schema: ${name}`).toBe(false)
      expect(validateLayout(value).length, `validate: ${name}`).toBeGreaterThan(0)
    }
  })
  it('the example layouts are valid for the app-side rules', () => {
    for (const g of [example1, example2, example3]) expect(isValidLayout(g)).toBe(true)
  })
  it('LOOK_PROPS ranges and enums match the schema', () => {
    const s = schema as any
    const base = s.$defs.lookBase.properties
    for (const [name, spec] of Object.entries(LOOK_PROPS)) {
      if (spec.type === 'number') {
        expect(base[name].minimum, name).toBe(spec.min)
        expect(base[name].maximum, name).toBe(spec.max)
      } else if (spec.type === 'enum') expect(base[name].enum, name).toEqual([...spec.values])
      else if (spec.type === 'color') expect(base[name].$ref, name).toBe('#/$defs/color')
      else if (spec.type === 'imageRef') expect(base[name].$ref, name).toBe('#/$defs/fillImage')
    }
    const content = s.$defs.contentProperties.properties
    expect(Object.keys(content).sort()).toEqual([...LOOK_CONTENT_PROPS].sort())
    expect(content.label.maxLength).toBe(LOOK_LIMITS.labelMax)
    expect(content.icon.maxLength).toBe(LOOK_LIMITS.iconMax)
    expect(Object.keys(base).sort()).toEqual(Object.keys(LOOK_PROPS).filter((n) => !(LOOK_CONTENT_PROPS as readonly string[]).includes(n)).sort())
    for (const [name, r] of Object.entries(METRICS_PROPS)) {
      expect(s.$defs.metrics.properties[name].minimum, name).toBe(r.min)
      expect(s.$defs.metrics.properties[name].maximum, name).toBe(r.max)
    }
    expect(s.properties.rules.maxItems).toBe(LOOK_LIMITS.rulesMax)
    expect(s.$defs.target.oneOf[1].properties.keys.maxItems).toBe(LOOK_LIMITS.targetKeysMax)
    expect(s.$defs.holdMs.minimum).toBe(LOOK_LIMITS.holdMsMin)
    expect(s.$defs.holdMs.maximum).toBe(LOOK_LIMITS.holdMsMax)
    expect(s.$defs.stateName.enum).toEqual([...LOOK_STATES])
    expect(s.$defs.assets.maxProperties).toBe(ASSET_LIMITS.countMax)
    expect(new RegExp(s.$defs.assets.propertyNames.pattern).source).toBe(new RegExp('^[a-z0-9][a-z0-9_-]{0,31}$').source)
    expect(Math.ceil(ASSET_LIMITS.bytesMax / 3) * 4).toBe(s.$defs.assets.additionalProperties.maxLength)
    expect(Object.keys(s.$defs.states.properties)).toEqual([...LOOK_STATES])
    expect(Object.keys(s.$defs.background.properties)).toEqual([...BACKGROUND_PROPS])
  })
})

// ---- 同梱レイアウトとテスト用レイアウト（移行後） -----------------------------------------------------------------------

const rawPresets = import.meta.glob('../../../../config/keyboard/presets/*.json', { eager: true, query: '?raw', import: 'default' }) as Record<string, string>
const rawTestLayouts = import.meta.glob('../../../../config/keyboard/test-layouts/*.json', { eager: true, query: '?raw', import: 'default' }) as Record<string, string>

describe('bundled layouts are migrated to schema 2', () => {
  it('all presets load, are schema 2 and use no look settings', () => {
    expect(presetResult.problems).toEqual([])
    expect(presetResult.fallback).toBe(false)
    expect(presetResult.presets).toHaveLength(8)
    for (const p of presetResult.presets) {
      expect(p.schema).toBe(2)
      expect(schemaValidate(p), `${p.id}: ${JSON.stringify(schemaValidate.errors)}`).toBe(true)
      for (const k of ['look', 'roles', 'rules', 'metrics', 'background', 'assets'] as const) expect(p[k], `${p.id}.${k}`).toBeUndefined()
    }
  })
  it('no file keeps the old names and migration is a no-op on them', () => {
    const all = { ...rawPresets, ...rawTestLayouts }
    expect(Object.keys(rawPresets)).toHaveLength(8)
    expect(Object.keys(rawTestLayouts).length).toBeGreaterThanOrEqual(11)
    for (const [path, text] of Object.entries(all)) {
      expect(text, path).not.toContain('"style"')
      expect(JSON.parse(text).schema, path).toBe(2)
      expect(migrateText(text).status, path).toBe('skipped')
    }
  })
})

// ---- 移行スクリプト（web/scripts/migrate-layout-v2.mjs） ------------------------------------------------------------

describe('migrate-layout-v2', () => {
  const v1 = `{
  "schema": 1,
  "id": "x",
  "name": "x",
  "mode": "alpha",
  "rows": [
    {"keys": [
      {"id": "a", "kind": "text", "text": "a"},
      {"id": "f", "kind": "action", "action": "enter", "style": "function", "label": "改行",
       "longPress": [{"label": "x", "kind": "text", "text": "x", "style": "accent"}]},
      {"id": "c", "kind": "action", "action": "shift", "style": "accent"}
    ]}
  ]
}
`
  it('renames schema and style, keeping the hand-written formatting', () => {
    const r = migrateText(v1)
    expect(r.status).toBe('migrated')
    expect(r.text).toBe(v1.replace('"schema": 1', '"schema": 2').replaceAll('"style"', '"role"'))
    const data = JSON.parse(r.text)
    expect(data.schema).toBe(2)
    expect(data.rows[0].keys[1].role).toBe('function')
    expect(data.rows[0].keys[1].longPress[0].role).toBe('accent')
    expect(data.rows[0].keys[0]).not.toHaveProperty('role')
    expect(r.text).not.toContain('"style"')
  })
  it('is idempotent', () => {
    const once = migrateText(v1)
    const twice = migrateText(once.text)
    expect(twice.status).toBe('skipped')
    expect(twice.text).toBe(once.text)
  })
  it('migrates a layout with no style at all', () => {
    const text = '{"schema":1,"id":"x","name":"x","mode":"alpha","rows":[{"keys":[{"id":"a","kind":"text","text":"a"}]}]}\n'
    const r = migrateText(text)
    expect(r.status).toBe('migrated')
    expect(r.text).toBe(text.replace('"schema":1', '"schema":2'))
  })
  it('leaves schema 2 and other values alone', () => {
    expect(migrateText('{"schema":2,"id":"x"}').status).toBe('skipped')
    expect(migrateText('{"id":"x"}').status).toBe('skipped')
    expect(migrateText('[]').status).toBe('skipped')
  })
  it('refuses what it cannot migrate safely', () => {
    expect(migrateText('{').status).toBe('error')
    const odd = v1.replace('"function"', '"fancy"')
    const r = migrateText(odd)
    expect(r.status).toBe('error')
    expect(r.text).toBe(odd)
  })
  it('migrated layouts pass validation', () => {
    // 長押しの項目の style は v1 の検証でも通らない（キー本体だけが role を持つ）ので、検証にはそれを外した形を使う
    const withoutPopupStyle = v1.replace(', "style": "accent"}]', '}]')
    expect(validateLayout(JSON.parse(migrateText(withoutPopupStyle).text))).toEqual([])
  })
})
