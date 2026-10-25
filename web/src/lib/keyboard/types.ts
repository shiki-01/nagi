// config/keyboard/layout.schema.json に対応する型と定数。
// スキーマを変えたら、ここと validate.ts を揃える（schema.test.ts が ajv で突き合わせる）。

export const SCHEMA_VERSION = 2

export const LAYOUT_MODES = ['kana', 'alpha', 'number', 'symbol', 'emoji', 'custom'] as const
export const KEY_ROLES = ['normal', 'function', 'accent'] as const
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
  'dismiss',
  // ツールバー用（rows のキーに置いても動く）
  'paste',
  'deleteWord'
] as const

/** ツールバーを表す行番号（resolveLook などの rowIndex 引数。rows の番号は変わらない）。 */
export const TOOLBAR_ROW = -1
/** ツールバーのキーの解決の最下位に重ねる既定（共有ベクタ resolve-toolbar.json と iOS の KeyLookResolver と同じ値）。 */
export const TOOLBAR_DEFAULT_LOOK = { fontSize: 14, inset: 6 } as const/** ツールバーの action 別の既定の文字（アイコンは iOS 側の既定。web のプレビューは文字で近似する）。 */
export const TOOLBAR_ACTION_LABELS: Readonly<Record<string, string>> = {
  paste: '貼り付け',
  deleteWord: '単語削除',
  dismiss: '閉じる',
  space: '空白',
  enter: '改行',
  moveLeft: '←',
  moveRight: '→',
  globe: '🌐'
}

/** 組み込みレイアウト名（layout.target に指定できる）。 */
export const BUILTIN_LAYOUT_IDS = [
  'qwerty-kana',
  'qwerty-alpha',
  'flick-kana',
  'flick-alpha',
  'symbols',
  'numbers',
  'emoji',
  // 割り当て済みのかな / 英字レイアウトを指す特別名
  'kana',
  'alpha'
] as const

/** 取り込む JSON 全体の上限（バイト）。iOS の NagiLayoutStore.maxBytes と同じ（assets を含められる大きさ。§5.5）。 */
export const LAYOUT_FILE_MAX_BYTES = 1536 * 1024

export const LIMITS = {
  rowsMax: 8,
  keysPerRowMax: 24,
  rowWidthMin: 1,
  rowWidthMax: 20,
  rowHeightMax: 4,
  // 行の高さ比の合計の上限（キーボード全体の高さに収めるため。iOS 側の LayoutValidation.rowHeightSumMax と同期）
  rowHeightSumMax: 4.5,
  // ツールバー（候補バーが空のときの 1 行）のキー数。高さは 44pt 固定
  toolbarKeysMax: 12,
  toolbarKeysMin: 1,
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
// icon の書式。config/keyboard/layout.schema.json の icon.pattern と Swift の KeyboardIconRef と同じ規則。
const ICONIFY_PART = '[a-z0-9]+(?:-[a-z0-9]+)*'
export const ICONIFY_ICON_PATTERN = new RegExp(`^iconify:(${ICONIFY_PART}):(${ICONIFY_PART})$`)
export const SF_ICON_PATTERN = /^sf:[A-Za-z0-9._-]+$/
export const ACTION_PATTERN = /^[a-zA-Z][a-zA-Z0-9]{0,31}$/

export type LayoutMode = (typeof LAYOUT_MODES)[number]
export type KeyRole = (typeof KEY_ROLES)[number]
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
  role?: KeyRole
  longPress?: PopupKey[]
  visible?: Visible
  /** このキーの見た目。docs/keyboard-style-spec.md §2 */
  look?: Look
  /** このキー自身の状態ごとの見た目。 */
  states?: Partial<Record<StateName, StateLook>>
}

export interface TextKey extends KeyBase {
  kind: 'text'
  text: string
  shift?: string
}
export interface FlickKey extends KeyBase {
  kind: 'flick'
  center?: FlickCell
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
  /** この行の全キーの見た目。 */
  look?: Look
  keys: Key[]
}

/** 候補バーが空のときに出る 1 行（高さ 44pt 固定）。キーは rows と同じ型。 */
export interface Toolbar {
  keys: Key[]
  /** ツールバーの全キーの見た目（rows[].look と同じ型・同じ継承）。 */
  look?: Look
}

export interface KeyboardLayout {
  $schema?: string
  schema: 2
  id: string
  name: string
  mode: LayoutMode
  /** 全キー共通の見た目。 */
  look?: Look
  /** 役割（role）ごとの見た目。 */
  roles?: Partial<Record<KeyRole, Look>>
  /** 状態の見た目（全キー／指定キー対象）。 */
  rules?: Rule[]
  metrics?: Metrics
  background?: Background
  /** 画像 id → data URI（PNG / JPEG）。 */
  assets?: Record<string, string>
  rows: Row[]
  toolbar?: Toolbar
}

// ---- 見た目（look）。docs/keyboard-style-spec.md §2〜§6 ----------------------------------------------------------
// 範囲・名前は Swift の KeyLookLimits（frontend-ios-spike/Shared/KeyLook.swift）と layout.schema.json と同じ値にする
// （keyboard.test.ts の「constants match schema」と look.test.ts が突き合わせる）。

export const LOOK_FONT_WEIGHTS = ['ultraLight', 'thin', 'light', 'regular', 'medium', 'semibold', 'bold', 'heavy', 'black'] as const
export const LOOK_FONT_DESIGNS = ['system', 'rounded', 'monospaced', 'serif'] as const
export const LOOK_IMAGE_FITS = ['fill', 'fit', 'tile'] as const
/** 色のトークン（`@` を付けて指定する）。 */
export const LOOK_TOKENS = ['label', 'secondaryLabel', 'key', 'function', 'accent', 'separator', 'clear'] as const

export type LookPropSpec =
  | { type: 'number'; min: number; max: number }
  | { type: 'enum'; values: readonly string[] }
  | { type: 'color' }
  /** 画像 id（assets のキー）または "none"。 */
  | { type: 'imageRef' }
  /** 表示文字（1〜labelMax 文字）。states / rules の中だけ。 */
  | { type: 'label' }
  /** アイコン（icon の書式）。states / rules の中だけ。 */
  | { type: 'icon' }

const num = (min: number, max: number): LookPropSpec => ({ type: 'number', min, max })

/** look のプロパティ名 → 型・範囲（§2.1）。 */
export const LOOK_PROPS: Readonly<Record<string, LookPropSpec>> = {
  fontSize: num(8, 40),
  fontWeight: { type: 'enum', values: LOOK_FONT_WEIGHTS },
  fontDesign: { type: 'enum', values: LOOK_FONT_DESIGNS },
  color: { type: 'color' },
  fill: { type: 'color' },
  fillOpacity: num(0, 1),
  fillImage: { type: 'imageRef' },
  fillImageFit: { type: 'enum', values: LOOK_IMAGE_FITS },
  fillImageOpacity: num(0, 1),
  cornerRadius: num(0, 24),
  borderWidth: num(0, 4),
  borderColor: { type: 'color' },
  shadowColor: { type: 'color' },
  shadowRadius: num(0, 16),
  shadowX: num(-8, 8),
  shadowY: num(-8, 8),
  shadowOpacity: num(0, 1),
  opacity: num(0, 1),
  scale: num(0.5, 1.5),
  offsetX: num(-20, 20),
  offsetY: num(-20, 20),
  inset: num(0, 12),
  padding: num(0, 16),
  animationMs: num(0, 1000),
  label: { type: 'label' },
  icon: { type: 'icon' }
}

/** states / rules の中だけで有効なプロパティ。 */
export const LOOK_CONTENT_PROPS = ['label', 'icon'] as const

/** 背景（background）に指定できるプロパティ。 */
export const BACKGROUND_PROPS = ['fill', 'fillOpacity', 'fillImage', 'fillImageFit', 'fillImageOpacity'] as const

/** metrics のプロパティ → 範囲（pt）。 */
export const METRICS_PROPS = {
  keySpacing: { min: 0, max: 8 },
  rowSpacing: { min: 0, max: 8 },
  padding: { min: 0, max: 16 }
} as const

export const LOOK_LIMITS = {
  labelMax: 16,
  iconMax: 64,
  rulesMax: 32,
  targetKeysMax: 24,
  holdMsMin: 0,
  holdMsMax: 2000,
  holdMsDefault: 200,
  /** 押下中の背景の不透明度の既定（組み込みの pressed の指定）。 */
  pressedFillOpacity: 0.6
} as const

export const ASSET_LIMITS = {
  countMax: 4,
  /** 1 枚（デコード後のバイト）。 */
  bytesMax: 256 * 1024,
  totalMax: 768 * 1024,
  /** 長辺（px）。エディタが縮小する目安（検証はしない）。 */
  longSideMax: 1024,
  mimeTypes: ['image/png', 'image/jpeg'] as readonly string[]
} as const

export const ASSET_ID_PATTERN = /^[a-z0-9][a-z0-9_-]{0,31}$/

/** 状態の名前。 */
export const LOOK_STATES = ['pressed', 'released', 'longPress', 'flicking', 'spaceDrag', 'shift', 'capsLock', 'composing'] as const
export type StateName = (typeof LOOK_STATES)[number]

/** 状態を重ねる順（低 → 高。後が先を上書きする。§4.3）。 */
export const STATE_PRECEDENCE: readonly StateName[] = [
  'composing',
  'capsLock',
  'shift',
  'released',
  'pressed',
  'spaceDrag',
  'flicking',
  'longPress'
]

/** 状態の表示名と種類（局所＝キー単位、大域＝全キー同時。§4.1）。 */
export const LOOK_STATE_INFO: Readonly<Record<StateName, { label: string; scope: 'local' | 'global' }>> = {
  pressed: { label: '押下', scope: 'local' },
  released: { label: '離した直後', scope: 'local' },
  longPress: { label: '長押し', scope: 'local' },
  flicking: { label: 'フリック中', scope: 'local' },
  spaceDrag: { label: '移動中', scope: 'global' },
  shift: { label: 'Shift', scope: 'global' },
  capsLock: { label: 'Caps', scope: 'global' },
  composing: { label: '入力中', scope: 'global' }
}

/** 色。`#RGB` / `#RRGGBB` / `#RRGGBBAA` / `@トークン`、または { light, dark }。 */
export type Color = string | { light?: string; dark?: string }

/** 見た目の指定（平らなオブジェクト。§2.1）。label / icon は states / rules の中だけで有効。 */
export interface Look {
  fontSize?: number
  fontWeight?: (typeof LOOK_FONT_WEIGHTS)[number]
  fontDesign?: (typeof LOOK_FONT_DESIGNS)[number]
  color?: Color
  fill?: Color
  fillOpacity?: number
  fillImage?: string
  fillImageFit?: (typeof LOOK_IMAGE_FITS)[number]
  fillImageOpacity?: number
  cornerRadius?: number
  borderWidth?: number
  borderColor?: Color
  shadowColor?: Color
  shadowRadius?: number
  shadowX?: number
  shadowY?: number
  shadowOpacity?: number
  opacity?: number
  scale?: number
  offsetX?: number
  offsetY?: number
  inset?: number
  padding?: number
  animationMs?: number
  label?: string
  icon?: string
}

/** states 内の見た目。holdMs は released のときだけ使う。 */
export type StateLook = Look & { holdMs?: number }

export type Target = 'all' | 'others' | { keys: string[] } | { roles: KeyRole[] } | { actions: string[] }

export interface Rule {
  when: StateName
  target?: Target
  holdMs?: number
  look: Look
}

export interface Metrics {
  keySpacing?: number
  rowSpacing?: number
  padding?: number
}

export interface Background {
  fill?: Color
  fillOpacity?: number
  fillImage?: string
  fillImageFit?: (typeof LOOK_IMAGE_FITS)[number]
  fillImageOpacity?: number
}
