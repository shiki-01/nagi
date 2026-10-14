// config/keyboard/layout.schema.json に対応する型と定数。
// スキーマを変えたら、ここと validate.ts を揃える（schema.test.ts が ajv で突き合わせる）。

export const SCHEMA_VERSION = 1

export const LAYOUT_MODES = ['kana', 'alpha', 'number', 'symbol', 'emoji', 'custom'] as const
export const KEY_STYLES = ['normal', 'function', 'accent'] as const
export const KEY_KINDS = ['text', 'flick', 'toggle', 'action', 'shortcut', 'layout', 'spacer'] as const
export const POPUP_KINDS = ['text', 'action', 'shortcut'] as const
export const FLICK_DIRECTIONS = ['up', 'down', 'left', 'right'] as const

/** docs/ios-keyboard-layout.md に挙がっている action（「ほか」は許容するので網羅ではない）。 */
export const KNOWN_ACTIONS = [
  'space',
  'enter',
  'backspace',
  'shift',
  'capslock',
  'convert',
  'moveLeft',
  'moveRight',
  'dakuten',
  'smallKana',
  'modeSwitch',
  'globe',
  'dismiss'
] as const

/** 組み込みレイアウト名（layout.target に指定できる）。 */
export const BUILTIN_LAYOUT_IDS = [
  'qwerty-kana',
  'qwerty-alpha',
  'flick-kana',
  'flick-alpha',
  'symbols',
  'numbers',
  'emoji'
] as const

export const LIMITS = {
  rowsMax: 8,
  keysPerRowMax: 24,
  rowWidthMin: 1,
  rowWidthMax: 20,
  rowHeightMax: 4,
  widthMax: 20,
  textMax: 16,
  shortcutTextMax: 256,
  cycleMax: 16,
  stepsMax: 32,
  longPressMax: 12,
  nameMax: 64,
  labelMax: 16,
  iconMax: 64,
  kanaMax: 4
} as const

export const LAYOUT_ID_PATTERN = /^[a-z0-9][a-z0-9_-]{0,63}$/
export const KEY_ID_PATTERN = /^[A-Za-z0-9][A-Za-z0-9_.-]{0,31}$/
export const ACTION_PATTERN = /^[a-zA-Z][a-zA-Z0-9]{0,31}$/

export type LayoutMode = (typeof LAYOUT_MODES)[number]
export type KeyStyle = (typeof KEY_STYLES)[number]
export type KeyKind = (typeof KEY_KINDS)[number]
export type PopupKind = (typeof POPUP_KINDS)[number]
export type FlickDirection = (typeof FLICK_DIRECTIONS)[number]

export type Step = { action: string } | { text: string }
export type Visible = { field: string; in: string[] }
export type FlickCell = { text: string }

export type PopupKey =
  | { id?: string; label: string; kind: 'text'; text: string }
  | { id?: string; label: string; kind: 'action'; action: string }
  | { id?: string; label: string; kind: 'shortcut'; steps: Step[] }

interface KeyBase {
  id: string
  width?: number
  label?: string
  icon?: string
  style?: KeyStyle
  longPress?: PopupKey[]
  visible?: Visible
}

export interface TextKey extends KeyBase {
  kind: 'text'
  text: string
  shift?: string
}
export interface FlickKey extends KeyBase {
  kind: 'flick'
  center: FlickCell
  up?: FlickCell
  down?: FlickCell
  left?: FlickCell
  right?: FlickCell
  kana?: string
}
export interface ToggleKey extends KeyBase {
  kind: 'toggle'
  cycle: string[]
}
export interface ActionKey extends KeyBase {
  kind: 'action'
  action: string
}
export interface ShortcutKey extends KeyBase {
  kind: 'shortcut'
  steps: Step[]
}
export interface LayoutKey extends KeyBase {
  kind: 'layout'
  target: string
}
export interface SpacerKey extends KeyBase {
  kind: 'spacer'
}

export type Key = TextKey | FlickKey | ToggleKey | ActionKey | ShortcutKey | LayoutKey | SpacerKey

export interface Row {
  height?: number
  keys: Key[]
}

export interface KeyboardLayout {
  $schema?: string
  schema: 1
  id: string
  name: string
  mode: LayoutMode
  rows: Row[]
}
