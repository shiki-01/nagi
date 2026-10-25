// ツールバー（layout.toolbar）の検証・編集操作・見た目の解決。共有ベクタ（resolve-toolbar.json）は look.test.ts が見る。
import { describe, expect, it } from 'vitest'
import Ajv2020 from 'ajv/dist/2020'
import schema from '../../../../config/keyboard/layout.schema.json'
import { computeToolbarDrop, dropIndicator } from './canvas-geometry'
import type { RowGeom } from './canvas-geometry'
import { jumpTarget } from './look-jump'
import { clearLookProp, hasAnyLook, readLook, resetAllLooks, setLookProp } from './look-ops'
import { resolveLook } from './look-resolve'
import {
  addKey,
  addKeyInNewRow,
  addToolbar,
  allKeyIds,
  duplicateKey,
  findKey,
  insertKey,
  moveKey,
  moveKeys,
  moveKeysChecked,
  navigateKey,
  nudgeKeys,
  removeKey,
  removeKeys,
  removeToolbar,
  replaceKey,
  sampleLayout,
  shiftKey
} from './ops'
import { EditorStore } from './store.svelte'
import { KNOWN_ACTIONS, LIMITS, TOOLBAR_ROW } from './types'
import type { Key, KeyboardLayout } from './types'
import { describePath, errorsOf, validateLayout } from './validate'

const ajv = new Ajv2020({ strict: false })
const schemaValidate = ajv.compile(schema)

const act = (id: string, action: string, extra: Partial<Key> = {}): Key => ({ id, kind: 'action', action, ...extra }) as Key
const withToolbar = (keys: Key[] = [act('t1', 'paste'), act('t2', 'moveLeft'), act('t3', 'dismiss')]): KeyboardLayout => ({
  ...sampleLayout(),
  toolbar: { keys }
})
const tbIds = (l: KeyboardLayout) => l.toolbar?.keys.map((k) => k.id)
const rowIds = (l: KeyboardLayout) => l.rows.map((r) => r.keys.map((k) => k.id))

describe('toolbar: validation', () => {
  it('a layout with a toolbar is valid and agrees with the schema', () => {
    const l = withToolbar()
    l.toolbar!.look = { cornerRadius: 8 }
    l.toolbar!.keys.push({ id: 'emoji', kind: 'layout', target: 'emoji' }, { id: 'sp', kind: 'spacer' }, act('dw', 'deleteWord', { icon: 'sf:delete.left', label: '削除' }))
    expect(validateLayout(l)).toEqual([])
    expect(schemaValidate(l), JSON.stringify(schemaValidate.errors)).toBe(true)
  })
  it('key count 1..12', () => {
    const none = withToolbar([])
    expect(validateLayout(none).some((e) => e.path === 'toolbar.keys')).toBe(true)
    expect(schemaValidate(none)).toBe(false)
    const keys = Array.from({ length: LIMITS.toolbarKeysMax }, (_, i) => act(`k${i}`, 'space', { width: 0.5 }))
    expect(validateLayout(withToolbar(keys))).toEqual([])
    expect(schemaValidate(withToolbar(keys))).toBe(true)
    const over = [...keys, act('k99', 'space', { width: 0.5 })]
    expect(validateLayout(withToolbar(over)).some((e) => e.path === 'toolbar.keys')).toBe(true)
    expect(schemaValidate(withToolbar(over))).toBe(false)
  })
  it('rejects row-only fields, unknown fields and non-objects (ajv agrees)', () => {
    for (const mutate of [
      (l: any) => (l.toolbar.height = 1),
      (l: any) => (l.toolbar.extra = 1),
      (l: any) => (l.toolbar.keys[0].extra = 1),
      (l: any) => (l.toolbar.keys[0].kind = 'bogus'),
      (l: any) => (l.toolbar.keys = 'x'),
      (l: any) => (l.toolbar = [])
    ]) {
      const l = structuredClone(withToolbar()) as any
      mutate(l)
      expect(validateLayout(l).length).toBeGreaterThan(0)
      expect(schemaValidate(l)).toBe(false)
    }
  })
  it('ids are unique across rows and the toolbar; width sum is checked', () => {
    const dup = withToolbar([act('q', 'paste')])
    expect(validateLayout(dup).map((e) => e.message).join()).toContain('キー id の重複')
    const wide = withToolbar([act('a', 'space', { width: 15 }), act('b', 'space', { width: 15 })])
    expect(validateLayout(wide).some((e) => e.path === 'toolbar.keys' && e.message.includes('toolbar の幅の合計が範囲外'))).toBe(true)
  })
  it('layout.target inside the toolbar is checked like rows', () => {
    const l = withToolbar([{ id: 'x', kind: 'layout', target: 'nowhere' }])
    expect(validateLayout(l).some((e) => e.path === 'toolbar.keys[0].target')).toBe(true)
  })
  it('toolbar.look: bad values are warnings, content props are ignored', () => {
    const l = withToolbar()
    l.toolbar!.look = { cornerRadius: 99, label: 'x' } as never
    const issues = validateLayout(l)
    expect(errorsOf(issues)).toEqual([])
    expect(issues.filter((e) => e.severity === 'warning').map((e) => e.path)).toEqual(['toolbar.look.cornerRadius', 'toolbar.look.label'])
  })
  it('paste / deleteWord are known actions and listed in the schema examples', () => {
    expect(KNOWN_ACTIONS).toContain('paste')
    expect(KNOWN_ACTIONS).toContain('deleteWord')
    const examples: string[] = (schema as any).$defs.action.examples
    for (const a of KNOWN_ACTIONS) expect(examples, a).toContain(a)
  })
  it('constants match the schema', () => {
    const tb = (schema as any).$defs.toolbar
    expect(tb.properties.keys.maxItems).toBe(LIMITS.toolbarKeysMax)
    expect(tb.properties.keys.minItems).toBe(LIMITS.toolbarKeysMin)
  })
  it('describePath names the toolbar', () => {
    expect(describePath('toolbar.keys[1].action')).toBe('ツールバー の 2 番目のキー.action')
    expect(describePath('toolbar')).toBe('ツールバー')
  })
})

describe('toolbar: ops', () => {
  it('addToolbar creates valid default keys once; removeToolbar drops it (inputs untouched)', () => {
    const base = sampleLayout()
    const before = JSON.stringify(base)
    const l = addToolbar(base)
    expect(JSON.stringify(base)).toBe(before)
    expect(l.toolbar!.keys.length).toBeGreaterThan(0)
    expect(validateLayout(l)).toEqual([])
    expect(addToolbar(l)).toBe(l)
    expect(removeToolbar(l).toolbar).toBeUndefined()
    expect(removeToolbar(base)).toBe(base)
  })
  it('default toolbar ids avoid ids already used in rows', () => {
    const base = sampleLayout()
    base.rows[0].keys[0].id = 'tb-paste'
    expect(new Set(allKeyIds(addToolbar(base))).size).toBe(allKeyIds(addToolbar(base)).length)
  })
  it('findKey / allKeyIds see the toolbar (row = -1)', () => {
    const l = withToolbar()
    expect(findKey(l, 't2')).toMatchObject({ row: TOOLBAR_ROW, index: 1 })
    expect(findKey(l, 'q')).toMatchObject({ row: 0 })
    expect(allKeyIds(l).slice(0, 3)).toEqual(['t1', 't2', 't3'])
  })
  it('add / insert / duplicate / replace / shift / remove inside the toolbar', () => {
    let l = withToolbar()
    const a = addKey(l, TOOLBAR_ROW, 'action')
    expect(tbIds(a.layout)).toHaveLength(4)
    expect(tbIds(a.layout)?.at(-1)).toBe(a.keyId)
    const i = insertKey(l, TOOLBAR_ROW, 1, 'spacer')
    expect(tbIds(i.layout)?.[1]).toBe(i.keyId)
    const d = duplicateKey(l, 't1')
    expect(tbIds(d.layout)).toEqual(['t1', d.keyId, 't2', 't3'])
    l = replaceKey(l, 't2', act('t2', 'moveRight'))
    expect((findKey(l, 't2')!.key as { action: string }).action).toBe('moveRight')
    expect(tbIds(shiftKey(l, 't2', -1))).toEqual(['t2', 't1', 't3'])
    expect(shiftKey(l, 't1', -1)).toBe(l)
    expect(tbIds(removeKey(l, 't1'))).toEqual(['t2', 't3'])
  })
  it('insertKey respects the toolbar limit', () => {
    const keys = Array.from({ length: LIMITS.toolbarKeysMax }, (_, i) => act(`k${i}`, 'space', { width: 0.5 }))
    const l = withToolbar(keys)
    expect(insertKey(l, TOOLBAR_ROW, 0, 'text').keyId).toBe('')
  })
  it('removing the last toolbar key removes the toolbar', () => {
    const l = withToolbar([act('only', 'paste')])
    expect(removeKey(l, 'only').toolbar).toBeUndefined()
    expect(removeKeys(l, ['only']).toolbar).toBeUndefined()
    expect(rowIds(removeKeys(l, ['only']))).toEqual(rowIds(l))
  })
  it('moving between the toolbar and rows', () => {
    const l = withToolbar()
    const toTb = moveKeys(l, ['q'], { row: TOOLBAR_ROW, index: 1 })
    expect(tbIds(toTb)).toEqual(['t1', 'q', 't2', 't3'])
    expect(rowIds(toTb)[0]).not.toContain('q')
    const toRow = moveKeys(l, ['t2'], { row: 1, index: 0 })
    expect(tbIds(toRow)).toEqual(['t1', 't3'])
    expect(rowIds(toRow)[1][0]).toBe('t2')
    expect(tbIds(moveKey(l, 't3', TOOLBAR_ROW, 0))).toEqual(['t3', 't1', 't2'])
    expect(tbIds(moveKey(l, 'q', TOOLBAR_ROW, 0))).toEqual(['q', 't1', 't2', 't3'])
    // 並び順を保ったまま、toolbar と rows のキーをまとめて新しい行へ
    const mixed = moveKeys(l, ['q', 't2'], { newRow: 0 })
    expect(rowIds(mixed)[0]).toEqual(['t2', 'q'])
    expect(tbIds(mixed)).toEqual(['t1', 't3'])
  })
  it('moving the only toolbar key into rows removes the toolbar', () => {
    const l = withToolbar([act('only', 'paste')])
    const m = moveKeys(l, ['only'], { row: 0, index: 0 })
    expect(m.toolbar).toBeUndefined()
    expect(rowIds(m)[0][0]).toBe('only')
    // ツールバーの中での並べ替えでは消えない
    expect(tbIds(moveKeys(l, ['only'], { row: TOOLBAR_ROW, index: 0 }))).toBeUndefined
  })
  it('moveKeysChecked reports the toolbar limit', () => {
    const keys = Array.from({ length: LIMITS.toolbarKeysMax }, (_, i) => act(`k${i}`, 'space', { width: 0.5 }))
    const l = withToolbar(keys)
    const r = moveKeysChecked(l, ['q'], { row: TOOLBAR_ROW, index: 0 })
    expect(r.error).toContain('ツールバー')
    expect(r.layout).toBe(l)
    // 存在しないツールバーには移せない
    expect(moveKeysChecked(sampleLayout(), ['q'], { row: TOOLBAR_ROW, index: 0 }).layout).toEqual(sampleLayout())
  })
  it('addKeyInNewRow leaves the toolbar alone', () => {
    const r = addKeyInNewRow(withToolbar(), 0, 'text')
    expect(tbIds(r.layout)).toEqual(['t1', 't2', 't3'])
  })
  it('navigateKey walks toolbar -> rows (visual order)', () => {
    const l = withToolbar()
    expect(navigateKey(l, 't3', 'right')).toBe('q')
    expect(navigateKey(l, 'q', 'left')).toBe('t3')
    expect(navigateKey(l, 't1', 'down')).toBeDefined()
    expect(navigateKey(l, 'q', 'up')).toMatch(/^t/)
    expect(navigateKey(l, 't1', 'up')).toBeUndefined()
    expect(navigateKey(l, 't1', 'left')).toBeUndefined()
    expect(navigateKey(sampleLayout(), 'q', 'up')).toBeUndefined()
  })
  it('nudgeKeys moves within the toolbar and between toolbar and rows', () => {
    const l = withToolbar()
    expect(tbIds(nudgeKeys(l, ['t2'], 'left'))).toEqual(['t2', 't1', 't3'])
    expect(tbIds(nudgeKeys(l, ['t2'], 'right'))).toEqual(['t1', 't3', 't2'])
    expect(tbIds(nudgeKeys(l, ['t2'], 'down'))).toEqual(['t1', 't3'])
    expect(tbIds(nudgeKeys(l, ['q'], 'up'))).toContain('q')
    expect(nudgeKeys(l, ['t2'], 'up')).toBe(l)
  })
})

describe('toolbar: look', () => {
  it('resolveLook uses toolbar.look and the toolbar defaults for row -1 only', () => {
    const l = withToolbar()
    l.look = { opacity: 0.8 }
    l.toolbar!.look = { cornerRadius: 9 }
    l.rows[0].look = { cornerRadius: 3 }
    const t = resolveLook(l, TOOLBAR_ROW, l.toolbar!.keys[0])
    expect(t).toEqual({ fontSize: 14, inset: 6, opacity: 0.8, cornerRadius: 9 })
    expect(resolveLook(l, 0, l.rows[0].keys[0])).toEqual({ opacity: 0.8, cornerRadius: 3 })
  })
  it('role defaults to normal for toolbar keys (roles only apply when role is set)', () => {
    const l = withToolbar([act('a', 'paste'), act('b', 'paste', { role: 'function' })])
    l.roles = { function: { borderWidth: 2 } }
    expect(resolveLook(l, TOOLBAR_ROW, l.toolbar!.keys[0]).borderWidth).toBeUndefined()
    expect(resolveLook(l, TOOLBAR_ROW, l.toolbar!.keys[1]).borderWidth).toBe(2)
  })
  it('look-ops write toolbar.look at row -1 and prune it when empty', () => {
    const l = withToolbar()
    const a = setLookProp(l, { kind: 'row', row: TOOLBAR_ROW }, 'cornerRadius', 12)
    expect(a.toolbar!.look).toEqual({ cornerRadius: 12 })
    expect(readLook(a, { kind: 'row', row: TOOLBAR_ROW })).toEqual({ cornerRadius: 12 })
    expect(l.toolbar!.look).toBeUndefined()
    const b = clearLookProp(a, { kind: 'row', row: TOOLBAR_ROW }, 'cornerRadius')
    expect('look' in b.toolbar!).toBe(false)
    // 行の look とは別物
    expect(a.rows[0].look).toBeUndefined()
  })
  it('hasAnyLook / resetAllLooks cover the toolbar', () => {
    let l = withToolbar()
    expect(hasAnyLook(l)).toBe(false)
    l = setLookProp(l, { kind: 'row', row: TOOLBAR_ROW }, 'inset', 2)
    l = setLookProp(l, { kind: 'key', keyId: 't1' }, 'fontSize', 20)
    expect(hasAnyLook(l)).toBe(true)
    const r = resetAllLooks(l)
    expect(r.toolbar!.look).toBeUndefined()
    expect(r.toolbar!.keys[0].look).toBeUndefined()
    expect(hasAnyLook(r)).toBe(false)
  })
  it('jumpTarget opens the toolbar scope / toolbar keys', () => {
    const l = withToolbar()
    l.toolbar!.keys[1] = act('t2', 'moveLeft', { states: { pressed: { opacity: 2 } } })
    expect(jumpTarget(l, 'toolbar.look.cornerRadius')).toMatchObject({ tab: 'look', scope: 'row', row: TOOLBAR_ROW })
    expect(jumpTarget(l, 'toolbar.keys[1].states.pressed.opacity')).toMatchObject({ tab: 'look', scope: 'key', keyId: 't2', state: 'pressed' })
    expect(jumpTarget(l, 'toolbar.keys[0].action')).toMatchObject({ tab: 'basic', scope: 'key', keyId: 't1' })
    expect(jumpTarget(sampleLayout(), 'toolbar.look.inset')).toBeUndefined()
  })
})

describe('toolbar: canvas geometry', () => {
  const r = (left: number, right: number, top = 0, bottom = 44) => ({ left, right, top, bottom })
  const tb: RowGeom = { rect: r(0, 300), keys: [{ id: 'a', rect: r(0, 100) }, { id: 'b', rect: r(100, 200) }, { id: 'c', rect: r(200, 300) }] }
  it('computeToolbarDrop picks the insertion index, excluding dragged keys', () => {
    expect(computeToolbarDrop(tb, 20, 20)).toEqual({ kind: 'row', row: TOOLBAR_ROW, index: 0 })
    expect(computeToolbarDrop(tb, 160, 20)).toEqual({ kind: 'row', row: TOOLBAR_ROW, index: 2 })
    expect(computeToolbarDrop(tb, 290, 20)).toEqual({ kind: 'row', row: TOOLBAR_ROW, index: 3 })
    expect(computeToolbarDrop(tb, 160, 20, new Set(['a']))).toEqual({ kind: 'row', row: TOOLBAR_ROW, index: 1 })
  })
  it('outside the toolbar band there is no toolbar target', () => {
    expect(computeToolbarDrop(tb, 160, 60)).toBeNull()
    expect(computeToolbarDrop(tb, 500, 20)).toBeNull()
  })
  it('indicator for the toolbar (as a single-row board)', () => {
    const ind = dropIndicator([tb], { kind: 'row', row: 0, index: 1 })!
    expect(ind.top).toBe(0)
    expect(ind.bottom).toBe(44)
    expect(ind.left).toBeLessThan(100)
  })
})

describe('toolbar: store', () => {
  const store = () => new EditorStore({ layouts: [sampleLayout()], currentId: 'sample' })
  it('addToolbar / removeToolbar are undoable', () => {
    const s = store()
    s.addToolbar()
    expect(s.layout.toolbar).toBeDefined()
    expect(s.errors).toEqual([])
    s.removeToolbar()
    expect(s.layout.toolbar).toBeUndefined()
    s.undo()
    expect(s.layout.toolbar).toBeDefined()
  })
  it('addKeyAt the toolbar selects the new key; the limit gives a toolbar-specific notice', () => {
    const s = store()
    s.addToolbar()
    const n0 = s.layout.toolbar!.keys.length
    expect(s.addKeyAt('action', { row: TOOLBAR_ROW, index: 0 })).toBe(true)
    expect(s.layout.toolbar!.keys).toHaveLength(n0 + 1)
    expect(findKey(s.layout, s.selectedIds[0])?.row).toBe(TOOLBAR_ROW)
    while (s.layout.toolbar!.keys.length < LIMITS.toolbarKeysMax) s.addKeyAt('spacer', { row: TOOLBAR_ROW, index: 0 })
    expect(s.addKeyAt('text', { row: TOOLBAR_ROW, index: 0 })).toBe(false)
    expect(s.notice).toContain('ツールバー')
  })
  it('deleting the selected toolbar keys removes the toolbar when empty', () => {
    const s = store()
    s.addToolbar()
    s.selectedIds = s.layout.toolbar!.keys.map((k) => k.id)
    s.removeSelected()
    expect(s.layout.toolbar).toBeUndefined()
    expect(s.selectedIds).toEqual([])
  })
})
