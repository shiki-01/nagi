import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'
import { cssColor, faceCss, metricsCss, PALETTES, paletteVars, resolveBackground, backgroundCss, spaceDragOrigin, statesForKey, toStyle } from './look-css'
import { colorMode, hexToRgbAlpha, mergeColor, rgbAlphaToHex, splitColor } from './look-color-edit'
import { assetByteBudget, assetIdFromName, assetsUsage, dataUriBytes, encodePlan, fitWithin, formatKB } from './look-assets'
import { jumpTarget } from './look-jump'
import {
  addAsset,
  addRule,
  allowsProp,
  assetRefCount,
  clearLookProp,
  hasAnyLook,
  inheritedValue,
  moveRule,
  newTarget,
  readHoldMs,
  readLook,
  removeAsset,
  removeRule,
  removeRulesFor,
  resetAllLooks,
  resetKeyLooks,
  resetLook,
  rulesFor,
  setHoldMs,
  setLookProp,
  setMetric,
  setRuleTarget,
  targetKind
} from './look-ops'
import { resolveLook } from './look-resolve'
import { changeKind, clone, sampleLayout } from './ops'
import { parseDraft } from './json-sync'
import { EditorStore } from './store.svelte'
import { ASSET_LIMITS, LOOK_LIMITS, LOOK_STATES, LOOK_STATE_INFO } from './types'
import type { KeyboardLayout } from './types'
import { errorsOf, validateLayout, warningsOf } from './validate'

/** デコード後 n バイトの（中身はでたらめな）PNG の data URI。形式・サイズの検査だけを通す。 */
const uri = (bytes: number, mime = 'image/png') => `data:${mime};base64,${'A'.repeat(Math.ceil(bytes / 3) * 4)}`

/** 3 の倍数で 1 枚の上限以内の最大。 */
const MAXB = ASSET_LIMITS.bytesMax - 1
const frozen = <T>(v: T): T => {
  const deepFreeze = (o: unknown) => {
    if (typeof o === 'object' && o !== null && !Object.isFrozen(o)) {
      Object.freeze(o)
      Object.values(o).forEach(deepFreeze)
    }
  }
  deepFreeze(v)
  return v
}
const issues = (l: KeyboardLayout) => validateLayout(l)

describe('look-ops: プロパティの追加・変更・削除', () => {
  it('入力を変更しない（凍結した入力でも動く）', () => {
    const base = frozen(sampleLayout())
    const a = setLookProp(base, { kind: 'layout' }, 'cornerRadius', 12)
    const b = setLookProp(a, { kind: 'key', keyId: 'q' }, 'fill', '#FF0000')
    const c = setLookProp(b, { kind: 'keyState', keyId: 'q', state: 'pressed' }, 'label', 'X')
    expect(base.look).toBeUndefined()
    expect(a.look).toEqual({ cornerRadius: 12 })
    expect(c.rows[0].keys[0].look).toEqual({ fill: '#FF0000' })
    expect(c.rows[0].keys[0].states).toEqual({ pressed: { label: 'X' } })
  })

  it('変更が無ければ同じ参照を返す（履歴に積まれない）', () => {
    const a = setLookProp(sampleLayout(), { kind: 'layout' }, 'opacity', 0.5)
    expect(setLookProp(a, { kind: 'layout' }, 'opacity', 0.5)).toBe(a)
    expect(clearLookProp(a, { kind: 'layout' }, 'fill')).toBe(a)
    expect(resetLook(sampleLayout(), { kind: 'layout' })).toEqual(sampleLayout())
  })

  it('setLookProp → clearLookProp で元の JSON に戻る（空の入れ物を残さない）', () => {
    const base = sampleLayout()
    const locs = [
      { kind: 'layout' },
      { kind: 'role', role: 'function' },
      { kind: 'row', row: 1 },
      { kind: 'key', keyId: 'q' },
      { kind: 'keyState', keyId: 'q', state: 'longPress' },
      { kind: 'background' }
    ] as const
    for (const loc of locs) {
      const prop = loc.kind === 'background' ? 'fillOpacity' : 'opacity'
      const set = setLookProp(base, loc, prop, 0.5)
      expect(JSON.stringify(set)).not.toBe(JSON.stringify(base))
      expect(readLook(set, loc)[prop]).toBe(0.5)
      expect(JSON.stringify(clearLookProp(set, loc, prop))).toBe(JSON.stringify(base))
    }
  })

  it('label / icon は states・rules の中だけ。background は 5 つだけ', () => {
    expect(allowsProp({ kind: 'layout' }, 'label')).toBe(false)
    expect(allowsProp({ kind: 'key', keyId: 'q' }, 'icon')).toBe(false)
    expect(allowsProp({ kind: 'keyState', keyId: 'q', state: 'pressed' }, 'label')).toBe(true)
    expect(allowsProp({ kind: 'rule', index: 0 }, 'icon')).toBe(true)
    expect(allowsProp({ kind: 'background' }, 'cornerRadius')).toBe(false)
    expect(allowsProp({ kind: 'layout' }, 'unknownProp')).toBe(false)
    const base = sampleLayout()
    expect(setLookProp(base, { kind: 'layout' }, 'label', 'x')).toBe(base)
    expect(setLookProp(base, { kind: 'background' }, 'cornerRadius', 3)).toBe(base)
  })

  it('範囲外の値もそのまま書く（クランプしない）。検証は警告にする', () => {
    const l = setLookProp(sampleLayout(), { kind: 'layout' }, 'cornerRadius', 99)
    expect(l.look).toEqual({ cornerRadius: 99 })
    expect(errorsOf(issues(l))).toEqual([])
    expect(warningsOf(issues(l)).map((w) => w.path)).toContain('look.cornerRadius')
  })

  it('存在しないキー・行には何もしない', () => {
    const base = sampleLayout()
    expect(setLookProp(base, { kind: 'key', keyId: 'nope' }, 'opacity', 0.5)).toBe(base)
    expect(setLookProp(base, { kind: 'row', row: 9 }, 'opacity', 0.5)).toBe(base)
    expect(setLookProp(base, { kind: 'rule', index: 0 }, 'opacity', 0.5)).toBe(base)
  })

  it('resetLook は場所の指定をすべて消す', () => {
    let l = setLookProp(sampleLayout(), { kind: 'role', role: 'accent' }, 'fill', '@accent')
    l = setLookProp(l, { kind: 'role', role: 'accent' }, 'fontWeight', 'bold')
    expect(resetLook(l, { kind: 'role', role: 'accent' }).roles).toBeUndefined()
  })
})

describe('look-ops: キー・全体のリセット', () => {
  const styled = () => {
    let l = setLookProp(sampleLayout(), { kind: 'key', keyId: 'q' }, 'fill', '#FF0000')
    l = setLookProp(l, { kind: 'keyState', keyId: 'q', state: 'pressed' }, 'scale', 0.9)
    l = setLookProp(l, { kind: 'keyState', keyId: 'w', state: 'pressed' }, 'scale', 0.9)
    l = setLookProp(l, { kind: 'row', row: 0 }, 'opacity', 0.9)
    l = setLookProp(l, { kind: 'layout' }, 'cornerRadius', 8)
    l = addRule(l, 'spaceDrag', 'others').layout
    l = setMetric(l, 'keySpacing', 4)
    return addAsset(l, 'wood', uri(1000)).layout
  }
  it('キー単位', () => {
    const l = resetKeyLooks(styled(), ['q'])
    expect(l.rows[0].keys[0].look).toBeUndefined()
    expect(l.rows[0].keys[0].states).toBeUndefined()
    expect(l.rows[0].keys[1].states).toEqual({ pressed: { scale: 0.9 } })
    expect(l.look).toEqual({ cornerRadius: 8 })
  })
  it('全体: 見た目の指定だけが消え、レイアウトは変わらない', () => {
    const s = styled()
    expect(hasAnyLook(s)).toBe(true)
    const l = resetAllLooks(s)
    expect(JSON.stringify(l)).toBe(JSON.stringify(sampleLayout()))
    expect(hasAnyLook(l)).toBe(false)
    expect(resetAllLooks(l)).toBe(l)
  })
})

describe('look-ops: 状態（states / rules）', () => {
  it('addRule: 末尾に追加、all は target を省略、上限 32', () => {
    const r = addRule(sampleLayout(), 'pressed')
    expect(r.layout.rules).toEqual([{ when: 'pressed', look: {} }])
    expect(r.index).toBe(0)
    const o = addRule(r.layout, 'spaceDrag', 'others')
    expect(o.layout.rules?.[1]).toEqual({ when: 'spaceDrag', target: 'others', look: {} })
    let l = sampleLayout()
    for (let i = 0; i < LOOK_LIMITS.rulesMax; i++) l = addRule(l, 'pressed').layout
    const over = addRule(l, 'pressed')
    expect(over.error).toMatch(/32/)
    expect(over.layout).toBe(l)
    expect(errorsOf(issues(l))).toEqual([])
  })

  it('rule の look を編集できる。空の rule は残る（対象の指定を保つ）', () => {
    let l = addRule(sampleLayout(), 'spaceDrag', 'others').layout
    l = setLookProp(l, { kind: 'rule', index: 0 }, 'opacity', 0.35)
    l = setLookProp(l, { kind: 'rule', index: 0 }, 'animationMs', 120)
    expect(l.rules).toEqual([{ when: 'spaceDrag', target: 'others', look: { opacity: 0.35, animationMs: 120 } }])
    l = clearLookProp(clearLookProp(l, { kind: 'rule', index: 0 }, 'opacity'), { kind: 'rule', index: 0 }, 'animationMs')
    expect(l.rules).toEqual([{ when: 'spaceDrag', target: 'others', look: {} }])
    expect(errorsOf(issues(l))).toEqual([])
  })

  it('target の種類を替える・並べ替え・削除（空になれば rules を消す）', () => {
    let l = addRule(addRule(sampleLayout(), 'pressed').layout, 'pressed').layout
    l = setRuleTarget(l, 0, { keys: ['q'] })
    l = setRuleTarget(l, 1, { roles: ['function'] })
    expect(rulesFor(l, 'pressed').map((r) => r.rule.target)).toEqual([{ keys: ['q'] }, { roles: ['function'] }])
    expect(targetKind(l.rules![0].target)).toBe('keys')
    expect(targetKind(undefined)).toBe('all')
    expect(newTarget('actions')).toEqual({ actions: [] })
    expect(newTarget('all')).toBeUndefined()
    l = moveRule(l, 0, 1)
    expect(l.rules!.map((r) => r.target)).toEqual([{ roles: ['function'] }, { keys: ['q'] }])
    expect(setRuleTarget(l, 0, { roles: ['function'] })).toBe(l)
    l = setRuleTarget(l, 0, 'all')
    expect(l.rules![0].target).toBeUndefined()
    l = removeRule(removeRule(l, 0), 0)
    expect(l.rules).toBeUndefined()
  })

  it('removeRulesFor は当該の状態だけ消す', () => {
    let l = addRule(addRule(sampleLayout(), 'pressed').layout, 'shift').layout
    l = removeRulesFor(l, 'pressed')
    expect(l.rules!.map((r) => r.when)).toEqual(['shift'])
    expect(removeRulesFor(l, 'pressed')).toBe(l)
  })

  it('holdMs: rule と states.released に書ける', () => {
    let l = addRule(sampleLayout(), 'released').layout
    l = setHoldMs(l, { kind: 'rule', index: 0 }, 150)
    expect(l.rules![0]).toMatchObject({ when: 'released', holdMs: 150 })
    expect(readHoldMs(l, { kind: 'rule', index: 0 })).toBe(150)
    l = resetLook(l, { kind: 'rule', index: 0 })
    expect(l.rules![0].holdMs).toBeUndefined()
    const loc = { kind: 'keyState', keyId: 'q', state: 'released' } as const
    let k = setHoldMs(sampleLayout(), loc, 500)
    expect(k.rows[0].keys[0].states).toEqual({ released: { holdMs: 500 } })
    expect(errorsOf(issues(k))).toEqual([])
    k = setHoldMs(k, loc, undefined)
    expect(JSON.stringify(k)).toBe(JSON.stringify(sampleLayout()))
    expect(setHoldMs(sampleLayout(), { kind: 'layout' }, 100)).toEqual(sampleLayout())
  })
})

describe('look-ops: metrics', () => {
  it('追加・削除で空にならない', () => {
    let l = setMetric(sampleLayout(), 'keySpacing', 4)
    l = setMetric(l, 'padding', 2)
    expect(l.metrics).toEqual({ keySpacing: 4, padding: 2 })
    l = setMetric(setMetric(l, 'keySpacing', undefined), 'padding', undefined)
    expect(l.metrics).toBeUndefined()
    expect(JSON.stringify(l)).toBe(JSON.stringify(sampleLayout()))
  })
})

describe('look-ops: assets と参照の整合', () => {
  it('addAsset: 形式・id・重複・上限を検査する', () => {
    const base = sampleLayout()
    expect(addAsset(base, 'Wood', uri(100)).error).toMatch(/画像 id/)
    expect(addAsset(base, 'wood', 'data:image/gif;base64,AAAA').error).toMatch(/PNG/)
    expect(addAsset(base, 'wood', 'https://example.com/a.png').error).toMatch(/PNG/)
    const a = addAsset(base, 'wood', uri(1000))
    expect(a.error).toBeUndefined()
    expect(a.layout.assets).toEqual({ wood: uri(1000) })
    expect(base.assets).toBeUndefined()
    expect(addAsset(a.layout, 'wood', uri(10)).error).toMatch(/使われて/)
    expect(errorsOf(issues(a.layout))).toEqual([])
  })

  it('個数（4 枚）・1 枚（256KB）・合計（768KB）の上限で追加できない', () => {
    let l = sampleLayout()
    expect(addAsset(l, 'big', uri(ASSET_LIMITS.bytesMax + 3)).error).toMatch(/256 KB/)
    for (const id of ['a', 'b', 'c']) l = addAsset(l, id, uri(MAXB)).layout
    // 3 枚で 768KB 近い。4 枚目は個数ではなく合計で拒否される
    const total = addAsset(l, 'd', uri(MAXB))
    expect(total.error).toMatch(/合計/)
    expect(total.layout).toBe(l)
    let m = sampleLayout()
    for (const id of ['a', 'b', 'c', 'd']) m = addAsset(m, id, uri(1000)).layout
    expect(Object.keys(m.assets!)).toHaveLength(4)
    expect(addAsset(m, 'e', uri(1000)).error).toMatch(/4 枚/)
    expect(errorsOf(issues(m))).toEqual([])
  })

  it('removeAsset は参照する fillImage をすべて外し、空の入れ物を残さない', () => {
    let l = addAsset(sampleLayout(), 'wood', uri(500)).layout
    l = addAsset(l, 'sky', uri(500)).layout
    l = setLookProp(l, { kind: 'layout' }, 'fillImage', 'wood')
    l = setLookProp(l, { kind: 'background' }, 'fillImage', 'wood')
    l = setLookProp(l, { kind: 'key', keyId: 'q' }, 'fillImage', 'wood')
    l = setLookProp(l, { kind: 'keyState', keyId: 'q', state: 'pressed' }, 'fillImage', 'sky')
    l = setLookProp(l, { kind: 'role', role: 'accent' }, 'fillImage', 'wood')
    l = addRule(l, 'shift').layout
    l = setLookProp(l, { kind: 'rule', index: 0 }, 'fillImage', 'wood')
    expect(assetRefCount(l, 'wood')).toBe(5)
    expect(assetRefCount(l, 'sky')).toBe(1)
    const r = removeAsset(l, 'wood')
    expect(r.assets).toEqual({ sky: uri(500) })
    expect(assetRefCount(r, 'wood')).toBe(0)
    expect(r.look).toBeUndefined()
    expect(r.background).toBeUndefined()
    expect(r.roles).toBeUndefined()
    expect(r.rows[0].keys[0].look).toBeUndefined()
    expect(r.rows[0].keys[0].states).toEqual({ pressed: { fillImage: 'sky' } })
    expect(warningsOf(issues(r))).toEqual([])
    const none = removeAsset(r, 'sky')
    expect(none.assets).toBeUndefined()
    expect(warningsOf(issues(none))).toEqual([])
    expect(removeAsset(none, 'sky')).toBe(none)
    // 元は変わらない
    expect(l.assets).toHaveProperty('wood')
  })

  it('"none" は参照ではない（画像 id と一致しない限り残る）', () => {
    let l = addAsset(sampleLayout(), 'wood', uri(500)).layout
    l = setLookProp(l, { kind: 'key', keyId: 'q' }, 'fillImage', 'none')
    expect(assetRefCount(l, 'wood')).toBe(0)
    expect(removeAsset(l, 'wood').rows[0].keys[0].look).toEqual({ fillImage: 'none' })
  })
})

describe('継承される値', () => {
  it('指定を外したときの解決結果を返す', () => {
    let l = setLookProp(sampleLayout(), { kind: 'layout' }, 'cornerRadius', 12)
    l = setLookProp(l, { kind: 'role', role: 'function' }, 'cornerRadius', 4)
    l = setLookProp(l, { kind: 'key', keyId: 'shift' }, 'cornerRadius', 20)
    const key = l.rows[1].keys[0]
    const ctx = { rowIndex: 1, key, states: [] }
    expect(inheritedValue(l, { kind: 'key', keyId: 'shift' }, 'cornerRadius', ctx)).toBe(4)
    // 代表キー自身の指定（20）が役割の指定より強いので、役割の指定を外しても 20 のまま
    expect(inheritedValue(l, { kind: 'role', role: 'function' }, 'cornerRadius', ctx)).toBe(20)
    expect(inheritedValue(l, { kind: 'layout' }, 'cornerRadius', { rowIndex: 1, key: l.rows[1].keys[1], states: [] })).toBeUndefined()
    expect(inheritedValue(l, { kind: 'background' }, 'fill', ctx)).toBeUndefined()
  })
  it('状態の指定は、状態が有効なときの継承を返す', () => {
    let l = setLookProp(sampleLayout(), { kind: 'key', keyId: 'q' }, 'fillOpacity', 0.8)
    l = setLookProp(l, { kind: 'keyState', keyId: 'q', state: 'pressed' }, 'fillOpacity', 0.2)
    const ctx = { rowIndex: 0, key: l.rows[0].keys[0], states: ['pressed'] as const }
    // 押下の組み込み既定（0.6）が下位に残る
    expect(inheritedValue(l, { kind: 'keyState', keyId: 'q', state: 'pressed' }, 'fillOpacity', ctx)).toBe(0.6)
  })
})

describe('spec §5.2〜5.4 の例を ops だけで作れる', () => {
  it('§5.3: キーの look + pressed / longPress + 全体の pressed / released ルール', () => {
    let l = sampleLayout()
    const w = { kind: 'key', keyId: 'w' } as const
    l = setLookProp(l, w, 'fill', { light: '#FFF4D6', dark: '#4A3F1F' })
    l = setLookProp(l, w, 'borderWidth', 1)
    l = setLookProp(l, w, 'borderColor', '#E0B341')
    l = setLookProp(l, { kind: 'keyState', keyId: 'w', state: 'pressed' }, 'fill', '#E0B341')
    l = setLookProp(l, { kind: 'keyState', keyId: 'w', state: 'longPress' }, 'scale', 1.08)
    const p = addRule(l, 'pressed')
    l = setLookProp(p.layout, { kind: 'rule', index: p.index }, 'scale', 0.94)
    const r = addRule(l, 'released')
    l = setHoldMs(setLookProp(r.layout, { kind: 'rule', index: r.index }, 'animationMs', 150), { kind: 'rule', index: r.index }, 150)
    expect(issues(l)).toEqual([])
    const pressed = resolveLook(l, 0, l.rows[0].keys[1], { states: ['pressed'] })
    expect(pressed).toMatchObject({ scale: 0.94, fill: '#E0B341FF', borderWidth: 1 })
  })
  it('§5.4: spaceDrag のルールとスペース自身の states', () => {
    let l = sampleLayout()
    const r = addRule(l, 'spaceDrag', 'others')
    l = setLookProp(r.layout, { kind: 'rule', index: 0 }, 'opacity', 0.35)
    const sp = { kind: 'keyState', keyId: 'space', state: 'spaceDrag' } as const
    l = setLookProp(setLookProp(l, sp, 'label', '移動'), sp, 'opacity', 0.7)
    expect(issues(l)).toEqual([])
    const origin = spaceDragOrigin(l)
    expect(origin).toBe('space')
    const ctx = { states: ['spaceDrag' as const], origin }
    expect(resolveLook(l, 1, l.rows[1].keys[1], ctx)).toMatchObject({ opacity: 0.7, label: '移動' })
    expect(resolveLook(l, 0, l.rows[0].keys[0], ctx)).toMatchObject({ opacity: 0.35 })
  })
})

describe('changeKind は見た目の指定を残す', () => {
  it('look / states', () => {
    const k = changeKind({ id: 'x', kind: 'text', text: 'a', look: { opacity: 0.5 }, states: { pressed: { scale: 0.9 } } }, 'action')
    expect(k.look).toEqual({ opacity: 0.5 })
    expect(k.states).toEqual({ pressed: { scale: 0.9 } })
  })
})

describe('色の編集', () => {
  it('hex ⇄ rgb + alpha', () => {
    expect(hexToRgbAlpha('#FF000080')).toEqual({ rgb: '#ff0000', alpha: 0.5 })
    expect(hexToRgbAlpha('#f80')).toEqual({ rgb: '#ff8800', alpha: 1 })
    expect(hexToRgbAlpha('@accent')).toBeUndefined()
    expect(hexToRgbAlpha('#GG')).toBeUndefined()
    expect(rgbAlphaToHex('#ff0000', 1)).toBe('#FF0000')
    expect(rgbAlphaToHex('#ff0000', 0.5)).toBe('#FF000080')
    expect(rgbAlphaToHex('#ff0000', 0)).toBe('#FF000000')
  })
  it('単色 ⇄ ライト/ダーク別', () => {
    expect(colorMode('#FFF')).toBe('single')
    expect(colorMode({ light: '#FFF' })).toBe('pair')
    expect(splitColor('#112233', '#000000')).toEqual({ light: '#112233', dark: '#112233' })
    expect(splitColor(undefined, '#000000')).toEqual({ light: '#000000', dark: '#000000' })
    expect(splitColor({ dark: '#222222' }, '#000000')).toEqual({ dark: '#222222' })
    expect(mergeColor({ light: '#111111', dark: '#222222' }, '#000000')).toBe('#111111')
    expect(mergeColor({ dark: '#222222' }, '#000000')).toBe('#222222')
    expect(mergeColor(undefined, '#000000')).toBe('#000000')
  })
})

describe('画像の計算', () => {
  it('fitWithin: 縮小のみ、長辺 1024 以内', () => {
    expect(fitWithin(4000, 2000, 1024)).toEqual({ width: 1024, height: 512 })
    expect(fitWithin(300, 200, 1024)).toEqual({ width: 300, height: 200 })
    expect(fitWithin(5000, 1, 1024)).toEqual({ width: 1024, height: 1 })
  })
  it('encodePlan: 透過なしは JPEG 0.85 から、ありは PNG。徐々に小さくなる', () => {
    const jpeg = encodePlan(3000, 2000, false)
    expect(jpeg[0]).toEqual({ format: 'image/jpeg', quality: 0.85, width: 1024, height: 683 })
    expect(jpeg.every((a) => a.format === 'image/jpeg')).toBe(true)
    const q = jpeg.slice(0, 5).map((a) => a.quality)
    expect(q).toEqual([...q].sort((a, b) => b! - a!))
    const png = encodePlan(3000, 2000, true)
    expect(png[0]).toMatchObject({ format: 'image/png', width: 1024, height: 683 })
    expect(png.every((a) => a.format === 'image/png')).toBe(true)
    for (const plan of [jpeg, png]) {
      const areas = plan.map((a) => a.width * a.height)
      expect(areas).toEqual([...areas].sort((a, b) => b - a))
      expect(plan.every((a) => a.width >= 1 && a.height >= 1 && Math.max(a.width, a.height) <= ASSET_LIMITS.longSideMax)).toBe(true)
    }
  })
  it('assetByteBudget: 1 枚の上限と合計の残りの小さいほう', () => {
    expect(assetByteBudget({})).toBe(ASSET_LIMITS.bytesMax)
    expect(assetByteBudget({ a: uri(MAXB), b: uri(MAXB) })).toBe(ASSET_LIMITS.bytesMax)
    const two = { a: uri(MAXB), b: uri(MAXB), c: uri(MAXB - 30000) }
    expect(assetByteBudget(two)).toBeLessThan(40000)
    expect(assetByteBudget({ a: uri(MAXB), b: uri(MAXB), c: uri(MAXB), d: uri(1) })).toBe(0)
    expect(assetsUsage({ a: uri(3000) })).toBe(3000)
    expect(dataUriBytes(uri(3000, 'image/jpeg'))).toBe(3000)
  })
  it('assetIdFromName: パターンに合い、衝突しない', () => {
    const re = /^[a-z0-9][a-z0-9_-]{0,31}$/
    expect(assetIdFromName('Wood Texture.JPG', [])).toBe('wood-texture')
    expect(assetIdFromName('wood.png', ['wood'])).toBe('wood-2')
    expect(assetIdFromName('壁紙.png', [])).toBe('image')
    expect(assetIdFromName('--_a.png', [])).toBe('a')
    expect(re.test(assetIdFromName('x'.repeat(100) + '.png', []))).toBe(true)
    expect(re.test(assetIdFromName('x'.repeat(100) + '.png', ['x'.repeat(24)]))).toBe(true)
  })
  it('formatKB', () => {
    expect(formatKB(1536)).toBe('1.5 KB')
    expect(formatKB(262144)).toBe('256 KB')
  })
})

describe('プレビューの CSS', () => {
  const light = PALETTES.light
  it('パレットは app.css の値と同じ', () => {
    const css = readFileSync(new URL('../../app.css', import.meta.url), 'utf8')
    const lightBlock = css.slice(css.indexOf(':root {'), css.indexOf('@media (prefers-color-scheme: dark)'))
    const darkBlock = css.slice(css.indexOf('@media (prefers-color-scheme: dark)'))
    for (const [scheme, block] of [['light', lightBlock], ['dark', darkBlock]] as const) {
      for (const [name, value] of Object.entries(paletteVars(PALETTES[scheme]))) {
        const m = new RegExp(`${name}:\\s*(#[0-9a-fA-F]{6})`).exec(block)
        expect(m?.[1].toLowerCase(), `${scheme} ${name}`).toBe(value.toLowerCase())
      }
    }
  })
  it('cssColor: 16 進・トークン・不透明度', () => {
    expect(cssColor('#FF000080', light)).toBe('rgba(255,0,0,0.502)')
    expect(cssColor('#FF0000FF', light, 0.5)).toBe('rgba(255,0,0,0.5)')
    expect(cssColor('@clear', light)).toBe('rgba(0,0,0,0)')
    expect(cssColor('@key', light)).toBe('rgba(253,252,249,1)')
    expect(cssColor('@accent', PALETTES.dark)).toBe('rgba(157,176,255,1)')
  })
  it('指定なしは現行の見た目（役割の色・角丸 7・縁取り）', () => {
    const f = faceCss({}, 'normal', { palette: light })
    expect(f.face).toMatchObject({ inset: '0px', 'background-color': 'rgba(253,252,249,1)', 'border-radius': '7px', 'box-shadow': '0 1px 0 rgba(0,0,0,0.3)', transition: 'none' })
    expect(f.face.opacity).toBeUndefined()
    expect(f.face.transform).toBeUndefined()
    expect(f.image).toBeNull()
    expect(faceCss({}, 'accent', { palette: light }).face).toMatchObject({ 'background-color': 'rgba(43,67,176,1)', color: 'rgba(255,255,255,1)' })
    expect(faceCss({}, 'function', { palette: PALETTES.dark }).face['background-color']).toBe('rgba(47,51,63,1)')
  })
  it('各プロパティを CSS にする', () => {
    const look = resolveLook(
      (() => {
        let l = sampleLayout()
        for (const [p, v] of Object.entries({
          cornerRadius: 12,
          borderWidth: 2,
          borderColor: '#00FF00',
          shadowRadius: 4,
          shadowX: 1,
          shadowY: 2,
          shadowColor: '#000000',
          shadowOpacity: 0.5,
          opacity: 0.7,
          scale: 0.9,
          offsetX: 3,
          offsetY: -2,
          inset: 2,
          padding: 4,
          fontSize: 20,
          fontWeight: 'bold',
          fontDesign: 'monospaced',
          fill: '@accent',
          fillOpacity: 0.5,
          animationMs: 120
        })) l = setLookProp(l, { kind: 'layout' }, p, v)
        return l
      })(),
      0,
      sampleLayout().rows[0].keys[0]
    )
    const f = faceCss(look, 'normal', { palette: light }).face
    expect(f).toMatchObject({
      inset: '2px',
      'border-radius': '12px',
      opacity: '0.7',
      transform: 'translate(3px,-2px) scale(0.9)',
      padding: '4px',
      'font-size': '20px',
      'font-weight': '700',
      transition: 'all 120ms ease-in-out',
      'background-color': 'rgba(43,67,176,0.5)'
    })
    expect(f['box-shadow']).toBe('inset 0 0 0 2px rgba(0,255,0,1),1px 2px 4px rgba(0,0,0,0.5)')
    expect(f['font-family']).toContain('monospace')
    expect(faceCss(look, 'normal', { palette: light, reduceMotion: true }).face.transition).toBe('none')
    expect(toStyle({ a: '1', b: '2' })).toBe('a:1;b:2')
  })
  it('shadowRadius: 0 は影なし', () => {
    expect(faceCss({ shadowRadius: 0 }, 'normal', { palette: light }).face['box-shadow']).toBe('none')
  })
  it('画像: fill / fit / tile、不透明度、不正な URI は使わない', () => {
    const u = uri(300, 'image/jpeg')
    const base = { fillImage: 'a', fillOpacity: 0.5, fillImageOpacity: 0.5 }
    const fill = faceCss(base, 'normal', { palette: light, image: { uri: u } }).image
    expect(fill).toMatchObject({ 'background-size': 'cover', opacity: '0.25' })
    expect(faceCss({ ...base, fillImageFit: 'fit' }, 'normal', { palette: light, image: { uri: u } }).image).toMatchObject({ 'background-size': 'contain' })
    const tile = faceCss({ ...base, fillImageFit: 'tile' }, 'normal', { palette: light, image: { uri: u, size: { w: 90, h: 60 } } }).image
    expect(tile).toMatchObject({ 'background-size': '30px 20px', 'background-repeat': 'repeat' })
    // 1 辺 8px 未満は fill として扱う
    expect(faceCss({ ...base, fillImageFit: 'tile' }, 'normal', { palette: light, image: { uri: u, size: { w: 6, h: 60 } } }).image).toMatchObject({ 'background-size': 'cover' })
    expect(faceCss(base, 'normal', { palette: light, image: { uri: 'data:image/png;base64,AAAA");x:("' } }).image).toBeNull()
    expect(faceCss({}, 'normal', { palette: light, image: { uri: u } }).image).toBeNull()
  })
  it('背景・metrics', () => {
    let l = setLookProp(sampleLayout(), { kind: 'background' }, 'fill', { light: '#FFFFFF', dark: '#000000' })
    l = setLookProp(l, { kind: 'background' }, 'fillOpacity', 0.5)
    expect(resolveBackground(l, 'light')).toEqual({ fill: '#FFFFFFFF', fillOpacity: 0.5 })
    expect(resolveBackground(l, 'dark').fill).toBe('#000000FF')
    expect(backgroundCss(resolveBackground(l, 'dark'), { palette: PALETTES.dark })?.face['background-color']).toBe('rgba(0,0,0,0.5)')
    expect(backgroundCss({}, { palette: light })).toBeNull()
    expect(resolveBackground(sampleLayout(), 'light')).toEqual({})
    expect(metricsCss(sampleLayout())).toEqual({ keySpacing: null, rowSpacing: null, padding: null })
    expect(metricsCss(setMetric(setMetric(sampleLayout(), 'keySpacing', 3), 'padding', 2))).toEqual({ keySpacing: '3px', rowSpacing: null, padding: '2px' })
    expect(metricsCss({ ...sampleLayout(), metrics: { keySpacing: 99 } })).toEqual({ keySpacing: null, rowSpacing: null, padding: null })
  })
  it('状態: 局所は選択中のキー（無ければ全キー）、大域は全キー', () => {
    const [q, w] = sampleLayout().rows[0].keys
    const active = ['pressed', 'spaceDrag', 'shift'] as const
    expect(statesForKey(q, [], active)).toEqual(['pressed', 'spaceDrag', 'shift'])
    expect(statesForKey(q, ['q'], active)).toEqual(['pressed', 'spaceDrag', 'shift'])
    expect(statesForKey(w, ['q'], active)).toEqual(['spaceDrag', 'shift'])
    expect(LOOK_STATES.every((s) => LOOK_STATE_INFO[s])).toBe(true)
  })
  it('spaceDragOrigin: space のキー、無ければ最初のキー', () => {
    expect(spaceDragOrigin(sampleLayout())).toBe('space')
    expect(spaceDragOrigin({ ...sampleLayout(), rows: [{ keys: [{ id: 'z', kind: 'spacer' }, { id: 'a', kind: 'text', text: 'a' }] }] })).toBe('a')
  })
})

describe('検証メッセージの場所へのジャンプ', () => {
  const l = addRule(sampleLayout(), 'shift').layout
  it.each([
    ['rows[0].keys[1].text', { tab: 'basic', scope: 'key', keyId: 'w', state: 'normal' }],
    ['rows[0].keys[1].look.fill', { tab: 'look', scope: 'key', keyId: 'w', state: 'normal' }],
    ['rows[0].keys[1].states.pressed.opacity', { tab: 'look', scope: 'key', keyId: 'w', state: 'pressed' }],
    ['rows[1].look.opacity', { tab: 'look', scope: 'row', row: 1, state: 'normal' }],
    ['rules[0].look.label', { tab: 'look', scope: 'layout', state: 'shift' }],
    ['look.fill', { tab: 'look', scope: 'layout', state: 'normal' }],
    ['roles.normal.fill', { tab: 'look', scope: 'layout', state: 'normal' }],
    ['assets.wood', { tab: 'look', scope: 'layout', state: 'normal' }],
    ['metrics.keySpacing', { tab: 'look', scope: 'layout', state: 'normal' }]
  ])('%s', (path, want) => expect(jumpTarget(l, path)).toEqual(want))
  it('対応する場所が無ければ undefined', () => {
    expect(jumpTarget(l, 'rows[9].keys[0].text')).toBeUndefined()
    expect(jumpTarget(l, 'rows[0].height')).toBeUndefined()
    expect(jumpTarget(l, 'name')).toBeUndefined()
    expect(jumpTarget(l, '')).toBeUndefined()
  })
})

describe('JSON 直接編集との同期', () => {
  it('parseDraft: 構文エラーと読めない形は反映しない', () => {
    expect(parseDraft('{')).toMatchObject({ ok: false })
    expect(parseDraft('[]')).toMatchObject({ ok: false })
    expect(parseDraft('{"id":"a","name":"b","rows":[{"keys":[{"id":"x","kind":"nope"}]}]}')).toMatchObject({ ok: false })
    const ok = parseDraft(JSON.stringify(sampleLayout()))
    expect(ok.ok && ok.layout.id).toBe('sample')
  })

  const fresh = () => new EditorStore({ layouts: [sampleLayout()], currentId: 'sample' })

  it('フォーム編集が JSON に反映され、JSON 直編集がフォームの読む値に反映される', () => {
    const store = fresh()
    store.setLookProp([{ kind: 'layout' }], 'cornerRadius', 12)
    expect(JSON.parse(JSON.stringify(store.layout)).look).toEqual({ cornerRadius: 12 })

    // JSON を直接書き換える（スキーマ違反を含んでも反映され、検証に出る）
    const edited = JSON.parse(JSON.stringify(store.layout))
    edited.look.cornerRadius = 99
    edited.look.fill = '@accent'
    const draft = parseDraft(JSON.stringify(edited, null, 2))
    expect(draft.ok).toBe(true)
    if (draft.ok) store.replaceFromJson(draft.layout)
    expect(readLook(store.layout, { kind: 'layout' })).toEqual({ cornerRadius: 99, fill: '@accent' })
    expect(store.errors).toEqual([])
    expect(store.warnings.map((w) => w.path)).toEqual(['look.cornerRadius'])
    // 警告のあるプロパティをフォームから直すと警告が消える
    store.setLookProp([{ kind: 'layout' }], 'cornerRadius', 10)
    expect(store.warnings).toEqual([])
    store.undo()
    expect(store.warnings).toHaveLength(1)
  })

  it('スライダーの連続入力は 1 回の undo にまとまる', () => {
    const store = fresh()
    for (const v of [1, 2, 3, 4]) store.setLookProp([{ kind: 'layout' }], 'cornerRadius', v, true)
    expect(store.layout.look).toEqual({ cornerRadius: 4 })
    store.undo()
    expect(store.layout.look).toBeUndefined()
    expect(store.canUndo).toBe(false)
  })

  it('複数キーにまとめて書く', () => {
    const store = fresh()
    store.setLookProp(['q', 'w'].map((keyId) => ({ kind: 'key' as const, keyId })), 'opacity', 0.5)
    expect(store.layout.rows[0].keys.slice(0, 3).map((k) => k.look?.opacity)).toEqual([0.5, 0.5, undefined])
    store.resetKeyLooks(['q', 'w'])
    expect(JSON.stringify(store.layout)).toBe(JSON.stringify(sampleLayout()))
  })

  it('rules の上限は通知を出し、画像の上限は理由を返す', () => {
    const store = fresh()
    for (let i = 0; i < LOOK_LIMITS.rulesMax; i++) expect(store.addRule('pressed')).toBe(i)
    expect(store.addRule('pressed')).toBe(-1)
    expect(store.notice).toMatch(/32/)
    expect(store.addAsset('a', uri(ASSET_LIMITS.bytesMax + 3))).toMatch(/256 KB/)
    expect(store.addAsset('a', uri(100))).toBeUndefined()
    store.setLookProp([{ kind: 'layout' }], 'fillImage', 'a')
    store.removeAsset('a')
    expect(store.layout.assets).toBeUndefined()
    expect(store.layout.look).toBeUndefined()
  })

  it('assets の上限超過を JSON で直接書くと error になり、書き出しを止める', () => {
    const store = fresh()
    const edited = clone(sampleLayout())
    edited.assets = { a: uri(ASSET_LIMITS.bytesMax + 3) }
    store.replaceFromJson(edited)
    expect(store.errors.map((e) => e.path)).toContain('assets.a')
  })

  it('状態タブを選ぶとプレビューの状態も同じものになる。ジャンプでタブ・選択が移る', () => {
    const store = fresh()
    store.selectLookState('pressed')
    expect(store.previewStates).toEqual(['pressed'])
    store.togglePreviewState('shift', true)
    expect(store.previewStates).toEqual(['pressed', 'shift'])
    store.togglePreviewState('pressed', false)
    expect(store.previewStates).toEqual(['shift'])
    store.selectLookState('normal')
    expect(store.previewStates).toEqual([])
    store.applyJump({ tab: 'look', scope: 'key', keyId: 'q', state: 'spaceDrag' })
    expect(store).toMatchObject({ inspectorTab: 'look', lookScope: 'key', lookState: 'spaceDrag' })
    expect(store.selectedIds).toEqual(['q'])
    expect(store.previewStates).toEqual(['spaceDrag'])
  })
})
