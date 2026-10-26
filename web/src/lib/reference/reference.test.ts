// /reference の内容が、検証コード・型定義・スキーマと食い違わないことを確かめる。
import schema from '../../../../config/keyboard/layout.schema.json'
import { describe, expect, it } from 'vitest'
import { KEY_KINDS, KNOWN_ACTIONS, LOOK_PROPS, LOOK_STATES, TOOLBAR_DEFAULT_LOOK } from '$lib/keyboard/types'
import { parseLayoutJSON, validateLayout } from '$lib/keyboard/validate'
import {
  ACTION_DOCS,
  BASE_LAYOUT,
  COMMON_KEY_FIELDS,
  ERROR_DOCS,
  KIND_DOCS,
  LOOK_PROP_DOCS,
  POPUP_EXAMPLE,
  STATE_DOCS,
  TOKEN_DOCS,
  TOOLBAR_EXAMPLE,
  TOP_FIELDS
} from './data'

const base = () => structuredClone(BASE_LAYOUT) as unknown as Record<string, unknown>
const issuesOf = (layout: unknown) => validateLayout(layout)

describe('reference: 例の JSON は検証を通る', () => {
  it('最小のレイアウト', () => {
    expect(issuesOf(base())).toEqual([])
  })
  for (const f of TOP_FIELDS) {
    it(`トップレベル ${f.name}`, () => {
      expect(issuesOf({ ...base(), ...f.example })).toEqual([])
    })
  }
  for (const kind of KEY_KINDS) {
    it(`キー ${kind}`, () => {
      const key = KIND_DOCS[kind].example
      expect(key.kind).toBe(kind)
      expect(issuesOf({ ...base(), rows: [{ keys: [key] }] })).toEqual([])
    })
  }
  it('longPress の例', () => {
    expect(issuesOf({ ...base(), rows: [{ keys: [POPUP_EXAMPLE] }] })).toEqual([])
  })
  it('ツールバーの例', () => {
    expect(issuesOf({ ...base(), ...TOOLBAR_EXAMPLE })).toEqual([])
  })
})

describe('reference: 表が定数と過不足なく対応する', () => {
  it('action', () => {
    expect(Object.keys(ACTION_DOCS).sort()).toEqual([...KNOWN_ACTIONS].sort())
  })
  it('look のプロパティ', () => {
    expect(Object.keys(LOOK_PROP_DOCS).sort()).toEqual(Object.keys(LOOK_PROPS).sort())
  })
  it('状態', () => {
    expect(Object.keys(STATE_DOCS).sort()).toEqual([...LOOK_STATES].sort())
  })
  it('キー種別', () => {
    expect(Object.keys(KIND_DOCS).sort()).toEqual([...KEY_KINDS].sort())
  })
  it('トークン', () => {
    expect(Object.keys(TOKEN_DOCS).length).toBe(7)
  })
  it('キー共通フィールドの名前が重複しない', () => {
    const names = COMMON_KEY_FIELDS.map((f) => f.name)
    expect(new Set(names).size).toBe(names.length)
  })
  it('フィールド表の名前が KeyBase の実際のフィールドと一致する', () => {
    // validate.ts の COMMON_FIELDS と同じ集合（longPress / visible / look / states を含む）
    const probe = { id: 'a', kind: 'spacer', width: 1, label: 'x', icon: 'x', role: 'normal', longPress: [{ label: 'x', kind: 'action', action: 'space' }], visible: { field: 'f', in: ['a'] }, look: {}, states: {} }
    expect(COMMON_KEY_FIELDS.map((f) => f.name).sort()).toEqual(Object.keys(probe).sort())
    expect(issuesOf({ ...base(), rows: [{ keys: [probe] }] })).toEqual([])
  })
  it('ツールバーの既定の look はスキーマの記述と同じ値', () => {
    expect(TOOLBAR_DEFAULT_LOOK).toEqual({ fontSize: 14, inset: 6 })
  })
})

describe('reference: 検証メッセージの一覧は validate.ts の出力と一致する', () => {
  for (const e of ERROR_DOCS) {
    if (!e.sample) continue
    it(`${e.severity}: ${e.message}`, () => {
      const sample = e.sample!()
      const messages =
        typeof sample === 'string'
          ? (() => {
              const r = parseLayoutJSON(sample)
              return r.ok ? [] : r.errors
            })()
          : issuesOf(sample)
      const needle = e.match ?? e.message
      const hit = messages.find((m) => m.message.includes(needle))
      expect(hit, `${needle} が出ていない: ${JSON.stringify(messages.map((m) => m.message))}`).toBeTruthy()
      expect(hit!.severity ?? 'error').toBe(e.severity)
    })
  }
  it('image 警告は sample 無し（canvas 無しの寸法検出が必要）', () => {
    expect(ERROR_DOCS.filter((e) => !e.sample).length).toBe(1)
  })
})

describe('reference: スキーマ', () => {
  it('layout.schema.json の action examples が KNOWN_ACTIONS と一致する', () => {
    expect([...schema.$defs.action.examples].sort()).toEqual([...KNOWN_ACTIONS].sort())
  })
})
