import { describe, expect, it } from 'vitest'
import Ajv2020 from 'ajv/dist/2020'
import schema from '../../../../config/keyboard/layout.schema.json'
import { History } from './history'
import { addKey, addKeyInNewRow, addRow, changeKind, duplicateLayout, insertKey, moveKey, moveKeys, moveKeysChecked, navigateKey, nudgeKeys, removeKey, removeKeys, removeRow, moveRow, sampleLayout, shiftKey, uniqueId } from './ops'
import { clampScale, computeDropTarget, dropIndicator, fitView, zoomAt } from './canvas-geometry'
import type { RowGeom } from './canvas-geometry'
import { LIMITS, SCHEMA_VERSION } from './types'
import { loadPresets } from './presets'
import { restoreState } from './store.svelte'
import { ACTION_PATTERN, BUILTIN_LAYOUT_IDS, KEY_KINDS, KEY_ROLES, LAYOUT_MODES } from './types'
import type { KeyboardLayout } from './types'
import { parseLayoutJSON, validateLayout } from './validate'

const ajv = new Ajv2020({ strict: false })
const schemaValidate = ajv.compile(schema)
const ids = (l: KeyboardLayout) => l.rows.map((r) => r.keys.map((k) => k.id))

describe('validateLayout', () => {
  it('sample is valid', () => expect(validateLayout(sampleLayout())).toEqual([]))
  it('reports duplicates, width sum and unknown target', () => {
    const l = sampleLayout()
    l.rows[0].keys[1].id = 'q'
    l.rows[1].keys.push({ id: 'go', kind: 'layout', target: 'nowhere' })
    const msgs = validateLayout(l).map((e) => e.message).join('\n')
    expect(msgs).toContain('重複')
    expect(msgs).toContain('存在するレイアウト')
    expect(validateLayout(l, { knownLayoutIds: ['nowhere'] }).some((e) => e.message.includes('存在'))).toBe(false)
  })
  it('limits', () => {
    const l = sampleLayout()
    l.rows = Array.from({ length: 9 }, () => ({ keys: [{ id: 'a', kind: 'spacer' as const }] }))
    expect(validateLayout(l).some((e) => e.path === 'rows')).toBe(true)
    const t = sampleLayout()
    ;(t.rows[0].keys[0] as { text: string }).text = 'x'.repeat(17)
    expect(validateLayout(t).some((e) => e.path.endsWith('.text'))).toBe(true)
    const w = sampleLayout()
    w.rows[1].keys[1].width = 25
    expect(validateLayout(w).length).toBeGreaterThan(0)
  })
  it('parseLayoutJSON handles syntax errors', () => {
    expect(parseLayoutJSON('{').ok).toBe(false)
    expect(parseLayoutJSON(JSON.stringify(sampleLayout())).ok).toBe(true)
  })
})

describe('schema agreement with ajv', () => {
  const good: unknown[] = [
    sampleLayout(),
    {
      schema: 2, id: 'f', name: 'f', mode: 'kana',
      rows: [{ height: 1.2, keys: [
        { id: 'a', kind: 'flick', center: { text: 'あ' }, up: { text: 'う' }, kana: 'あ', longPress: [{ label: '1', kind: 'text', text: '1' }, { label: 's', kind: 'shortcut', steps: [{ text: 'x' }, { action: 'space' }] }] },
        { id: 'b', kind: 'toggle', cycle: ['a', 'b'] },
        { id: 'c', kind: 'shortcut', steps: [{ action: 'moveLeft' }], visible: { field: 'returnKeyType', in: ['search'] } },
        { id: 'd', kind: 'layout', target: 'numbers', role: 'function' },
        { id: 'e', kind: 'spacer', width: 0.5 },
        { id: 'i1', kind: 'action', action: 'space', icon: 'iconify:mdi:home' },
        { id: 'i2', kind: 'action', action: 'space', icon: 'iconify:material-symbols:arrow-back-2' },
        { id: 'i3', kind: 'action', action: 'space', icon: 'sf:globe' },
        { id: 'i4', kind: 'action', action: 'space', icon: 'shift' },
        { id: 'i5', kind: 'action', action: 'space', icon: 'Weird Name' }
      ] }]
    }
  ]
  const bad = (): unknown[] => {
    const mk = (f: (l: any) => void) => { const l = structuredClone(good[1]) as any; f(l); return l }
    return [
      mk((l) => (l.schema = 1)),
      mk((l) => (l.schema = 3)),
      mk((l) => (l.mode = 'x')),
      mk((l) => (l.extra = 1)),
      mk((l) => (l.rows = [])),
      mk((l) => (l.id = 'Bad Id')),
      mk((l) => (l.rows[0].keys[0].extra = 1)),
      mk((l) => (l.rows[0].keys[0].up = { text: 'x'.repeat(17) })),
      mk((l) => { delete l.rows[0].keys[0].center; delete l.rows[0].keys[0].kana }),
      mk((l) => (l.rows[0].keys[1].cycle = [])),
      mk((l) => (l.rows[0].keys[2].steps = [{ action: 'a', text: 'b' }])),
      mk((l) => (l.rows[0].keys[3].role = 'neon')),
      mk((l) => (l.rows[0].keys[3].style = 'function')),
      mk((l) => (l.rows[0].keys[4].width = 0)),
      mk((l) => (l.rows[0].keys[0].longPress = [{ label: 'x', kind: 'layout', target: 'a' }])),
      mk((l) => (l.rows[0].keys[0].kind = 'bogus')),
      ...['iconify:mdi', 'iconify:mdi:', 'iconify::home', 'iconify:MDI:home', 'iconify:mdi:home:x', 'iconify:mdi:-home', 'iconify:mdi:ho--me', 'iconify:mdi:home\n', 'iconify:mdi:ホーム', 'sf:', 'sf:a b', 'sf:ホーム', 'iconify:' + 'a'.repeat(60) + ':b'].map(
        (icon) => mk((l) => (l.rows[0].keys[1].icon = icon))
      )
    ]
  }
  it('good inputs pass both', () => {
    for (const g of good) {
      expect(schemaValidate(g), JSON.stringify(schemaValidate.errors)).toBe(true)
      expect(validateLayout(g)).toEqual([])
    }
  })
  it('bad inputs fail both', () => {
    for (const b of bad()) {
      expect(schemaValidate(b)).toBe(false)
      expect(validateLayout(b).length).toBeGreaterThan(0)
    }
  })
  it('constants match schema enums', () => {
    const s = schema as any
    expect(s.properties.mode.enum).toEqual([...LAYOUT_MODES])
    expect(s.$defs.role.enum).toEqual([...KEY_ROLES])
    expect(s.properties.schema.const).toBe(SCHEMA_VERSION)
    expect(s.$defs.key.oneOf).toHaveLength(KEY_KINDS.length)
    expect(new RegExp(s.$defs.action.pattern).source).toBe(ACTION_PATTERN.source)
    expect(BUILTIN_LAYOUT_IDS).toContain('flick-kana')
  })
})

describe('ops', () => {
  it('uniqueId', () => expect(uniqueId('a', ['a', 'a-2'])).toBe('a-3'))
  it('add/remove/move rows', () => {
    let l = addRow(sampleLayout())
    expect(l.rows).toHaveLength(3)
    l = moveRow(l, 2, 0)
    expect(l.rows[0].keys[0].id).toBe('key')
    l = removeRow(l, 0)
    expect(l.rows).toHaveLength(2)
  })
  it('add/remove/move/shift keys without mutating input', () => {
    const base = sampleLayout()
    const before = JSON.stringify(base)
    const { layout, keyId } = addKey(base, 1, 'flick')
    expect(JSON.stringify(base)).toBe(before)
    expect(layout.rows[1].keys.at(-1)?.id).toBe(keyId)
    expect(validateLayout(layout)).toEqual([])
    const m = moveKey(layout, 'q', 1, 0)
    expect(ids(m)[1][0]).toBe('q')
    expect(ids(m)[0]).not.toContain('q')
    expect(ids(shiftKey(base, 'w', -1))[0].slice(0, 2)).toEqual(['w', 'q'])
    expect(ids(removeKey(base, 'q'))[0]).not.toContain('q')
  })
  it('changeKind keeps common fields', () => {
    const k = changeKind({ id: 'x', kind: 'text', text: 'a', width: 2, role: 'accent', label: 'A' }, 'action')
    expect(k).toMatchObject({ id: 'x', kind: 'action', width: 2, role: 'accent' })
    expect('text' in k).toBe(false)
  })
  it('duplicateLayout gets new id', () => {
    const d = duplicateLayout(sampleLayout(), ['sample'])
    expect(d.id).toBe('sample-copy')
  })
})

describe('History', () => {
  it('undo/redo and coalescing', () => {
    const h = new History(0)
    h.push(1, 'k', 0)
    h.push(2, 'k', 100)
    h.push(3)
    expect(h.undo()).toBe(2)
    expect(h.undo()).toBe(0)
    expect(h.canUndo).toBe(false)
    expect(h.redo()).toBe(2)
    h.push(9)
    expect(h.canRedo).toBe(false)
  })
})

describe('presets / restore', () => {
  it('falls back to sample when empty', () => {
    const r = loadPresets({})
    expect(r.fallback).toBe(true)
    expect(r.presets[0].id).toBe('sample')
  })
  it('skips invalid presets', () => {
    const r = loadPresets({ '/a.json': { default: sampleLayout() }, '/b.json': { default: { id: 'x' } } })
    expect(r.presets).toHaveLength(1)
    expect(r.problems).toHaveLength(1)
  })
  it('restoreState tolerates garbage', () => {
    expect(restoreState('nope')).toBeUndefined()
    expect(restoreState(JSON.stringify({ layouts: [1] }))).toBeUndefined()
    const s = restoreState(JSON.stringify({ layouts: [sampleLayout()], currentId: 'zz' }))
    expect(s?.currentId).toBe('sample')
  })
})

describe('canvas ops', () => {
  it('moveKeys: 複数キーを行をまたいで移動し、空の行は消える', () => {
    const base = sampleLayout()
    const m = moveKeys(base, ['q', 'w'], { row: 1, index: 1 })
    expect(ids(m)).toEqual([['e', 'r', 't', 'y', 'u', 'i', 'o', 'p'], ['shift', 'q', 'w', 'space', 'enter']])
    expect(validateLayout(m)).toEqual([])
    // 1 行目の全キーを 2 行目へ移すと 1 行目は消える
    const all = moveKeys(base, base.rows[0].keys.map((k) => k.id), { row: 1, index: 0 })
    expect(all.rows).toHaveLength(1)
    expect(all.rows[0].keys).toHaveLength(13)
  })
  it('moveKeys: 同じ行内の並べ替えは index が「移動するキーを除いた位置」', () => {
    const m = moveKeys(sampleLayout(), ['q'], { row: 0, index: 2 })
    expect(ids(m)[0].slice(0, 4)).toEqual(['w', 'e', 'q', 'r'])
    // 入力の並び順に依らずレイアウト上の順序を保つ
    expect(ids(moveKeys(sampleLayout(), ['w', 'q'], { row: 1, index: 0 }))[1].slice(0, 2)).toEqual(['q', 'w'])
  })
  it('moveKeys: 行間（newRow）へ移動して新しい行を作る', () => {
    const base = sampleLayout()
    const m = moveKeys(base, ['shift'], { newRow: 0 })
    expect(ids(m)).toEqual([['shift'], ids(base)[0], ['space', 'enter']])
    const end = moveKeys(base, ['q'], { newRow: 2 })
    expect(end.rows).toHaveLength(3)
    expect(end.rows[2].keys[0].id).toBe('q')
    // 入力は変更されない
    expect(base.rows).toHaveLength(2)
    expect(validateLayout(m)).toEqual([])
  })
  it('moveKeys: 単独キーの行を元の位置に戻すだけなら何もしない', () => {
    const base = moveKeys(sampleLayout(), ['shift'], { newRow: 0 })
    expect(moveKeys(base, ['shift'], { newRow: 0 })).toBe(base)
    expect(moveKeys(base, ['nope'], { row: 0, index: 0 })).toBe(base)
  })
  it('moveKeysChecked: 上限を超える移動は error を返して変更しない', () => {
    let l = sampleLayout()
    l.rows[1].keys = Array.from({ length: LIMITS.keysPerRowMax }, (_, i) => ({ id: `s${i}`, kind: 'spacer' as const }))
    const r = moveKeysChecked(l, ['q'], { row: 1, index: 0 })
    expect(r.error).toBeTruthy()
    expect(r.layout).toBe(l)
    l = { ...l, rows: Array.from({ length: LIMITS.rowsMax }, (_, i) => ({ keys: [{ id: `k${i}`, kind: 'spacer' as const }] })) }
    expect(moveKeysChecked(l, ['k0'], { newRow: 3 }).layout.rows).toHaveLength(LIMITS.rowsMax) // 元の行が消えるので上限内
    const two = { ...l, rows: [{ keys: [{ id: 'a', kind: 'spacer' as const }, { id: 'b', kind: 'spacer' as const }] }, ...l.rows.slice(1)] }
    expect(moveKeysChecked(two, ['a'], { newRow: 0 }).error).toBeTruthy()
  })
  it('removeKeys: 空になった行は消えるが最後の 1 行は残る', () => {
    const base = sampleLayout()
    expect(ids(removeKeys(base, ['shift', 'space', 'enter']))).toEqual([ids(base)[0]])
    const none = removeKeys(removeKeys(base, ids(base)[1]), ids(base)[0])
    expect(none.rows).toHaveLength(1)
    expect(removeKeys(base, ['zzz'])).toBe(base)
  })
  it('addKeyInNewRow / insertKey', () => {
    const base = sampleLayout()
    const a = addKeyInNewRow(base, 1, 'flick')
    expect(a.layout.rows).toHaveLength(3)
    expect(a.layout.rows[1].keys[0].id).toBe(a.keyId)
    expect(validateLayout(a.layout)).toEqual([])
    const b = insertKey(base, 0, 0, 'toggle')
    expect(b.layout.rows[0].keys[0].kind).toBe('toggle')
    expect(insertKey(base, 9, 0, 'text').keyId).toBe('')
  })
  it('navigateKey: 左右は行をまたぎ、上下は中心が近いキー', () => {
    const l = sampleLayout()
    expect(navigateKey(l, 'q', 'right')).toBe('w')
    expect(navigateKey(l, 'p', 'right')).toBe('shift')
    expect(navigateKey(l, 'shift', 'left')).toBe('p')
    expect(navigateKey(l, 'q', 'left')).toBeUndefined()
    expect(navigateKey(l, 'q', 'up')).toBeUndefined()
    expect(navigateKey(l, 'q', 'down')).toBe('shift')
    expect(navigateKey(l, 'p', 'down')).toBe('enter')
    expect(navigateKey(l, 'space', 'up')).toBe('t')
  })
  it('nudgeKeys: 左右は選択のまとまりごと、上下は隣の行へ', () => {
    const l = sampleLayout()
    expect(ids(nudgeKeys(l, ['w', 'e'], 'left'))[0].slice(0, 3)).toEqual(['w', 'e', 'q'])
    expect(ids(nudgeKeys(l, ['q'], 'left'))).toEqual(ids(l))
    expect(nudgeKeys(l, ['q'], 'left')).toBe(l)
    expect(ids(nudgeKeys(l, ['space', 'enter'], 'right'))[1]).toEqual(['shift', 'space', 'enter'])
    expect(ids(nudgeKeys(l, ['q'], 'down'))[1].slice(0, 2)).toEqual(['q', 'shift'])
    expect(nudgeKeys(l, ['q'], 'up')).toBe(l)
  })
})

describe('canvas geometry', () => {
  const r = (left: number, top: number, w: number, h: number) => ({ left, top, right: left + w, bottom: top + h })
  // 2 行（高さ 52、行間 6）。1 行目は 100 幅のキー 3 つ、2 行目は 1 つ。
  const rows: RowGeom[] = [
    { rect: r(0, 0, 300, 52), keys: ['a', 'b', 'c'].map((id, i) => ({ id, rect: r(i * 100, 0, 100, 52) })) },
    { rect: r(0, 58, 300, 52), keys: [{ id: 'd', rect: r(0, 58, 300, 52) }] }
  ]
  it('computeDropTarget', () => {
    expect(computeDropTarget(rows, 150, 26)).toEqual({ kind: 'row', row: 0, index: 1 }) // b の中心より左
    expect(computeDropTarget(rows, 160, 26)).toEqual({ kind: 'row', row: 0, index: 2 })
    expect(computeDropTarget(rows, 290, 26)).toEqual({ kind: 'row', row: 0, index: 3 })
    expect(computeDropTarget(rows, 10, 2)).toEqual({ kind: 'newRow', row: 0 })
    expect(computeDropTarget(rows, 10, 55)).toEqual({ kind: 'newRow', row: 1 }) // 行間
    expect(computeDropTarget(rows, 10, 108)).toEqual({ kind: 'newRow', row: 2 })
    expect(computeDropTarget(rows, 10, 300)).toBeNull()
    expect(computeDropTarget(rows, -200, 26)).toBeNull()
    // ドラッグ中のキーは並びから除いて数える
    expect(computeDropTarget(rows, 290, 26, new Set(['c']))).toEqual({ kind: 'row', row: 0, index: 2 })
    expect(computeDropTarget([], 0, 0)).toBeNull()
  })
  it('dropIndicator', () => {
    expect(dropIndicator(rows, { kind: 'newRow', row: 1 })).toMatchObject({ left: 0, right: 300 })
    const v = dropIndicator(rows, { kind: 'row', row: 0, index: 1 })!
    expect(v.top).toBe(0)
    expect(v.bottom).toBe(52)
    expect((v.left + v.right) / 2).toBe(97)
    expect(dropIndicator(rows, { kind: 'row', row: 5, index: 0 })).toBeNull()
  })
  it('zoomAt keeps the anchor fixed and clamps', () => {
    const v = zoomAt({ x: 10, y: 20, scale: 1 }, 2, 100, 100)
    expect(v.scale).toBe(2)
    // ワールド座標 (100-10, 100-20) が同じ画面位置に残る
    expect(v.x + 90 * 2).toBeCloseTo(100)
    expect(v.y + 80 * 2).toBeCloseTo(100)
    expect(zoomAt({ x: 0, y: 0, scale: 1 }, 1000, 0, 0).scale).toBe(clampScale(1000))
  })
  it('fitView centers the content', () => {
    const v = fitView(1000, 800, 400, 200)
    expect(v.x + 200 * v.scale).toBeCloseTo(500)
    expect(v.y + 100 * v.scale).toBeCloseTo(400)
    expect(fitView(0, 0, 10, 10).scale).toBe(1)
    expect(fitView(200, 200, 2000, 2000).scale).toBeLessThan(0.2 + 1e-9)
  })
})
