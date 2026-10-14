import { describe, expect, it } from 'vitest'
import Ajv2020 from 'ajv/dist/2020'
import schema from '../../../../config/keyboard/layout.schema.json'
import { History } from './history'
import { addKey, addRow, changeKind, duplicateLayout, moveKey, removeKey, removeRow, moveRow, sampleLayout, shiftKey, uniqueId } from './ops'
import { loadPresets } from './presets'
import { restoreState } from './store.svelte'
import { ACTION_PATTERN, BUILTIN_LAYOUT_IDS, KEY_KINDS, KEY_STYLES, LAYOUT_MODES } from './types'
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
      schema: 1, id: 'f', name: 'f', mode: 'kana',
      rows: [{ height: 1.2, keys: [
        { id: 'a', kind: 'flick', center: { text: 'あ' }, up: { text: 'う' }, kana: 'あ', longPress: [{ label: '1', kind: 'text', text: '1' }, { label: 's', kind: 'shortcut', steps: [{ text: 'x' }, { action: 'space' }] }] },
        { id: 'b', kind: 'toggle', cycle: ['a', 'b'] },
        { id: 'c', kind: 'shortcut', steps: [{ action: 'moveLeft' }], visible: { field: 'returnKeyType', in: ['search'] } },
        { id: 'd', kind: 'layout', target: 'numbers', style: 'function' },
        { id: 'e', kind: 'spacer', width: 0.5 }
      ] }]
    }
  ]
  const bad = (): unknown[] => {
    const mk = (f: (l: any) => void) => { const l = structuredClone(good[1]) as any; f(l); return l }
    return [
      mk((l) => (l.schema = 2)),
      mk((l) => (l.mode = 'x')),
      mk((l) => (l.extra = 1)),
      mk((l) => (l.rows = [])),
      mk((l) => (l.id = 'Bad Id')),
      mk((l) => (l.rows[0].keys[0].extra = 1)),
      mk((l) => (l.rows[0].keys[0].up = { text: 'x'.repeat(17) })),
      mk((l) => delete l.rows[0].keys[0].center),
      mk((l) => (l.rows[0].keys[1].cycle = [])),
      mk((l) => (l.rows[0].keys[2].steps = [{ action: 'a', text: 'b' }])),
      mk((l) => (l.rows[0].keys[3].style = 'neon')),
      mk((l) => (l.rows[0].keys[4].width = 0)),
      mk((l) => (l.rows[0].keys[0].longPress = [{ label: 'x', kind: 'layout', target: 'a' }])),
      mk((l) => (l.rows[0].keys[0].kind = 'bogus'))
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
    expect(s.$defs.style.enum).toEqual([...KEY_STYLES])
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
    const k = changeKind({ id: 'x', kind: 'text', text: 'a', width: 2, style: 'accent', label: 'A' }, 'action')
    expect(k).toMatchObject({ id: 'x', kind: 'action', width: 2, style: 'accent' })
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
