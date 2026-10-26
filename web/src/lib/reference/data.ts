// /reference ページの内容。値が web 側の定数（types.ts）にあるものは import して組み立てる。
// 定数に無いもの（説明文・既定値・iOS 側の動作）は docs/keyboard-style-spec.md・docs/ios-settings.md・docs/decisions.md から書き写している。
// reference.test.ts が、例の JSON と検証メッセージを validate.ts に通して食い違いを検出する。

import {
  ASSET_ID_PATTERN,
  ASSET_LIMITS,
  BACKGROUND_PROPS,
  BUILTIN_LAYOUT_IDS,
  FLICK_DIRECTIONS,
  ICONIFY_ICON_PATTERN,
  KEY_ID_PATTERN,
  KEY_KINDS,
  KEY_ROLES,
  KNOWN_ACTIONS,
  LAYOUT_FILE_MAX_BYTES,
  LAYOUT_ID_PATTERN,
  LAYOUT_MODES,
  LIMITS,
  LOOK_FONT_DESIGNS,
  LOOK_FONT_WEIGHTS,
  LOOK_IMAGE_FITS,
  LOOK_LIMITS,
  LOOK_PROPS,
  LOOK_STATES,
  LOOK_STATE_INFO,
  LOOK_TOKENS,
  METRICS_PROPS,
  POPUP_KINDS,
  SCHEMA_VERSION,
  STATE_PRECEDENCE,
  TOOLBAR_DEFAULT_LOOK,
  TOOLBAR_ROW
} from '$lib/keyboard/types'
import type { StateName } from '$lib/keyboard/types'

export type KnownAction = (typeof KNOWN_ACTIONS)[number]
export type KeyKind = (typeof KEY_KINDS)[number]
export type LookToken = (typeof LOOK_TOKENS)[number]

/** フィールド表の 1 行。 */
export interface FieldRow {
  name: string
  type: string
  /** 必須かどうか（「必須」「省略可」、または条件）。 */
  required: string
  /** 省略時の値。 */
  def: string
  /** 範囲・上限。 */
  limit: string
  desc: string
}

const ref = (code: string) => code

// ---- 例で使う最小のレイアウト ------------------------------------------------------------------------------------------

/** 最小の有効なレイアウト。トップレベルの例は、これにフラグメントを重ねて検証する。 */
export const BASE_LAYOUT = {
  schema: SCHEMA_VERSION,
  id: 'my-layout',
  name: '私のレイアウト',
  mode: 'alpha',
  rows: [{ keys: [{ id: 'a', kind: 'text', text: 'a' }] }]
} as const

/** 1×1 の PNG（assets の例）。 */
export const TINY_PNG =
  'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=='

// ---- トップレベルのフィールド ------------------------------------------------------------------------------------------

export interface TopField extends FieldRow {
  /** 最小の例。BASE_LAYOUT に重ねて有効（error も warning も出ない）であること。 */
  example: Record<string, unknown>
  /** 例を表示するときの断片（省略時は example 全体）。 */
  show?: unknown
}

const ranges = (o: Record<string, { min: number; max: number }>) =>
  Object.entries(o)
    .map(([k, r]) => `${k} ${r.min}〜${r.max}`)
    .join(' / ')

export const TOP_FIELDS: readonly TopField[] = [
  {
    name: '$schema',
    type: 'string',
    required: '省略可',
    def: '—',
    limit: '—',
    desc: 'エディタの補完用（スキーマの $id は https://nagi.shiki-01.com/schemas/keyboard-layout-2.json）。値は検証に使われない。',
    example: { $schema: 'https://nagi.shiki-01.com/schemas/keyboard-layout-2.json' }
  },
  {
    name: 'schema',
    type: 'number',
    required: '必須',
    def: '—',
    limit: `${SCHEMA_VERSION} のみ`,
    desc: `スキーマのバージョン。現行版（${SCHEMA_VERSION}）だけを受理する。schema 1 は未対応で、web/scripts/migrate-layout-v2.mjs で移行する。`,
    example: { schema: SCHEMA_VERSION }
  },
  {
    name: 'id',
    type: 'string',
    required: '必須',
    def: '—',
    limit: `英小文字・数字・- _、先頭は英小文字か数字、64 文字以内（${ref(String(LAYOUT_ID_PATTERN))}）`,
    desc: 'レイアウトの識別子。保存ファイル名（<id>.json）にも使う。同じ id のユーザーレイアウトを取り込むと更新される。',
    example: { id: 'my-layout' }
  },
  {
    name: 'name',
    type: 'string',
    required: '必須',
    def: '—',
    limit: `1〜${LIMITS.nameMax} 文字`,
    desc: '表示名。',
    example: { name: '私のレイアウト' }
  },
  {
    name: 'mode',
    type: 'string（列挙）',
    required: '必須',
    def: '—',
    limit: LAYOUT_MODES.join(' / '),
    desc: '変換エンジンの扱いが変わる入力モード。',
    example: { mode: 'alpha' }
  },
  {
    name: 'rows',
    type: 'Row[]',
    required: '必須',
    def: '—',
    limit: `1〜${LIMITS.rowsMax} 行。行の height の合計は ${LIMITS.rowHeightSumMax} 以下`,
    desc: `行の配列。行は { height?, look?, keys }。height は行の高さの比（0 より大きく ${LIMITS.rowHeightMax} 以下、既定 1）。keys は 1〜${LIMITS.keysPerRowMax} 個で、width の合計は ${LIMITS.rowWidthMin}〜${LIMITS.rowWidthMax}。キー id は rows と toolbar を通して一意。`,
    example: {
      rows: [
        {
          height: 1,
          keys: [
            { id: 'a', kind: 'text', text: 'a', shift: 'A' },
            { id: 'sp', kind: 'action', action: 'space', width: 3, label: '空白' }
          ]
        }
      ]
    }
  },
  {
    name: 'look',
    type: 'Look',
    required: '省略可',
    def: '現行の見た目',
    limit: 'プロパティは「look のプロパティ」の表',
    desc: '全キー共通の見た目。label・icon は書けない（states / rules の中だけ）。',
    example: { look: { cornerRadius: 12, fontDesign: 'rounded' } }
  },
  {
    name: 'roles',
    type: '{ normal?, function?, accent? }',
    required: '省略可',
    def: '—',
    limit: `キーは ${KEY_ROLES.join(' / ')}`,
    desc: '役割（キーの role）ごとの見た目。値は Look。',
    example: { roles: { function: { fill: { light: '#D9DCE3', dark: '#2C2C2E' } } } }
  },
  {
    name: 'rules',
    type: 'Rule[]',
    required: '省略可',
    def: '—',
    limit: `${LOOK_LIMITS.rulesMax} 個以内`,
    desc: '状態の見た目を全キー／指定キーへまとめて指定する。Rule は { when, target?, holdMs?, look }。配列の後ろの rule が先の rule を上書きする。',
    example: { rules: [{ when: 'pressed', target: 'all', look: { scale: 0.94 } }] }
  },
  {
    name: 'states（キー内）',
    type: '{ 状態名: Look }',
    required: '省略可',
    def: '—',
    limit: `状態名は ${LOOK_STATES.join(' / ')}`,
    desc: 'トップレベルではなく、キーのフィールド。そのキー自身の状態ごとの見た目。released の値だけ holdMs も書ける。',
    example: {
      rows: [{ keys: [{ id: 'a', kind: 'text', text: 'a', states: { pressed: { fill: '#E0B341', color: '#000000' } } }] }]
    }
  },
  {
    name: 'metrics',
    type: '{ keySpacing?, rowSpacing?, padding? }',
    required: '省略可',
    def: '設定 keyboard.key_spacing_pt（既定 4）。外周は 6pt',
    limit: `pt。${ranges(METRICS_PROPS)}`,
    desc: 'キー間・行間の隙間と外周の余白。指定があれば設定の値より優先される。',
    example: { metrics: { keySpacing: 5, rowSpacing: 5, padding: 8 } }
  },
  {
    name: 'background',
    type: 'Background',
    required: '省略可',
    def: '塗らない（システムの背景が見える）',
    limit: `プロパティは ${BACKGROUND_PROPS.join(' / ')} のみ`,
    desc: 'キーボード全体の背景。候補バーも含めた最背面に描く。キーの fill を @clear か低い fillOpacity にすると透けて見える。',
    example: { assets: { dot: TINY_PNG }, background: { fill: '#101820', fillImage: 'dot', fillImageFit: 'tile', fillImageOpacity: 0.5 } }
  },
  {
    name: 'assets',
    type: '{ 画像 id: data URI }',
    required: '省略可',
    def: '—',
    limit: `${ASSET_LIMITS.countMax} 枚以内。1 枚 ${ASSET_LIMITS.bytesMax / 1024}KB 以内、合計 ${ASSET_LIMITS.totalMax / 1024}KB 以内（デコード後）。${ASSET_LIMITS.mimeTypes.join(' / ')} の base64 data URI`,
    desc: `背景画像。画像 id は ${ref(String(ASSET_ID_PATTERN))}。look.fillImage / background.fillImage から id で参照する。`,
    example: { assets: { dot: TINY_PNG }, look: { fillImage: 'dot', fillImageFit: 'tile' } },
    show: { assets: { dot: 'data:image/png;base64,…' }, look: { fillImage: 'dot', fillImageFit: 'tile' } }
  },
  {
    name: 'toolbar',
    type: '{ keys, look? }',
    required: '省略可',
    def: '無ければ設定 toolbar.items の並び',
    limit: `keys は ${LIMITS.toolbarKeysMin}〜${LIMITS.toolbarKeysMax} 個、width の合計 ${LIMITS.rowWidthMin}〜${LIMITS.rowWidthMax}`,
    desc: '候補バーの場所に出す高さ 44pt の 1 行。キーは rows のキーと同じ型。詳細は「ツールバー」の節。',
    example: {
      toolbar: {
        keys: [
          { id: 'tb-dismiss', kind: 'action', action: 'dismiss' },
          { id: 'tb-paste', kind: 'action', action: 'paste' }
        ]
      }
    }
  }
]

// ---- キー ------------------------------------------------------------------------------------------------------------

export const COMMON_KEY_FIELDS: readonly FieldRow[] = [
  {
    name: 'id',
    type: 'string',
    required: '必須',
    def: '—',
    limit: `英数字・_ . -、先頭は英数字、32 文字以内（${ref(String(KEY_ID_PATTERN))}）`,
    desc: 'キーの識別子。rows と toolbar を通して一意。rules の target.keys からも参照する。'
  },
  { name: 'kind', type: 'string（列挙）', required: '必須', def: '—', limit: KEY_KINDS.join(' / '), desc: 'キーの種類。種類ごとの追加フィールドは下の節。' },
  { name: 'width', type: 'number', required: '省略可', def: '1', limit: `0 より大きく ${LIMITS.widthMax} 以下`, desc: '行内の幅の比。行の width の合計は 1〜20。' },
  { name: 'label', type: 'string', required: '省略可', def: '—', limit: `0〜${LIMITS.labelMax} 文字`, desc: '表示する文字。' },
  {
    name: 'icon',
    type: 'string',
    required: '省略可',
    def: '—',
    limit: `${LIMITS.iconMax} 文字以内`,
    desc: `アイコン。sf:<SF Symbols 名>、iconify:<prefix>:<name>（例 iconify:mdi:home。${ref(String(ICONIFY_ICON_PATTERN))}。ホストアプリが取り込み時に取得して保存する）、接頭辞なしは SF Symbols 名。`
  },
  { name: 'role', type: 'string（列挙）', required: '省略可', def: 'normal', limit: KEY_ROLES.join(' / '), desc: '色の役割。roles と rules の target.roles で参照される。既定の色は normal=@key、function=@function、accent=@accent。' },
  {
    name: 'longPress',
    type: 'PopupKey[]',
    required: '省略可',
    def: '—',
    limit: `1〜${LIMITS.longPressMax} 個`,
    desc: `長押しで出るポップアップ。項目は { id?, label, kind } と kind ごとの値（kind は ${POPUP_KINDS.join(' / ')}。text は text、action は action、shortcut は steps）。flick キーと ⌫ の longPress は無視される。`
  },
  {
    name: 'visible',
    type: '{ field, in }',
    required: '省略可',
    def: '常に表示',
    limit: 'field は 1〜32 文字、in は 1〜16 個の文字列（各 32 文字以内）',
    desc: '表示条件。例: { "field": "returnKeyType", "in": ["search"] }。形が違う値は iOS が無視する。'
  },
  { name: 'look', type: 'Look', required: '省略可', def: '—', limit: '—', desc: 'このキーの見た目。' },
  { name: 'states', type: '{ 状態名: Look }', required: '省略可', def: '—', limit: '—', desc: 'このキー自身の状態ごとの見た目。' }
]

export interface KindDoc {
  summary: string
  fields: readonly FieldRow[]
  example: Record<string, unknown>
  note?: string
}

export const KIND_DOCS: Record<KeyKind, KindDoc> = {
  text: {
    summary: '文字を 1 つ送る。QWERTY のローマ字入力は、かなモードではコアのローマ字変換に渡す。',
    fields: [
      { name: 'text', type: 'string', required: '必須', def: '—', limit: `1〜${LIMITS.textMax} 文字`, desc: '送る文字。' },
      { name: 'shift', type: 'string', required: '省略可', def: '—', limit: `1〜${LIMITS.textMax} 文字`, desc: 'Shift 状態のときに送る文字。' }
    ],
    example: { id: 'q', kind: 'text', text: 'q', shift: 'Q' }
  },
  flick: {
    summary: 'フリック入力のキー。',
    fields: [
      { name: 'center', type: '{ text }', required: 'kana が無いとき必須', def: '—', limit: `text は 1〜${LIMITS.textMax} 文字`, desc: 'タップ（中央）の文字。' },
      ...FLICK_DIRECTIONS.map(
        (d): FieldRow => ({ name: d, type: '{ text }', required: '省略可', def: '—', limit: `text は 1〜${LIMITS.textMax} 文字`, desc: `${{ up: '上', down: '下', left: '左', right: '右' }[d]}フリックの文字。` })
      ),
      {
        name: 'kana',
        type: 'string',
        required: 'center が無いとき必須',
        def: '—',
        limit: `1〜${LIMITS.kanaMax} 文字`,
        desc: '「あ」のような 12 キーの名前。指定するとコアのトグル・フリック・濁点循環に任せる。'
      }
    ],
    example: { id: 'a', kind: 'flick', kana: 'あ', label: 'あ' },
    note: 'longPress は無視される。'
  },
  toggle: {
    summary: '押すたびに順に切り替わる（ガラケー式）。確定は他のキーか時間経過。',
    fields: [
      { name: 'cycle', type: 'string[]', required: '必須', def: '—', limit: `1〜${LIMITS.cycleMax} 要素、各 1〜${LIMITS.textMax} 文字`, desc: '切り替える文字の列。1 要素のときは置き換えず追加する。' }
    ],
    example: { id: 'abc', kind: 'toggle', cycle: ['a', 'b', 'c'] },
    note: '同じキーを input.toggle_timeout_ms（設定ファイルに値があれば。無ければ 1000ms。200〜3000）以内に押すと次の文字に置き換わる。'
  },
  action: {
    summary: '名前付きの操作を実行する。名前は「action 名一覧」の節。',
    fields: [
      { name: 'action', type: 'string', required: '必須', def: '—', limit: '英字で始まる英数字、32 文字以内', desc: '操作の名前。一覧にない名前も検証は通るが、iOS では何も起きない。' }
    ],
    example: { id: 'sp', kind: 'action', action: 'space', label: '空白', width: 4 }
  },
  shortcut: {
    summary: '複数の操作を続けて実行する（定型文の挿入など）。',
    fields: [
      {
        name: 'steps',
        type: '({ action } | { text })[]',
        required: '必須',
        def: '—',
        limit: `1〜${LIMITS.stepsMax} 個。text は 1〜${LIMITS.shortcutTextMax} 文字`,
        desc: '手順。各手順は action か text のどちらか一方だけを持つ。'
      }
    ],
    example: { id: 'thanks', kind: 'shortcut', label: 'ありがとう', steps: [{ text: 'ありがとうございます' }] },
    note:
      '実行前に入力中の文字列を確定する。text はコアを通さずホストへそのまま挿入する。未知の action や空の手順に当たるとそこで中断する（それまでの手順は実行済み）。'
  },
  layout: {
    summary: '別のレイアウトへ切り替える。',
    fields: [
      {
        name: 'target',
        type: 'string',
        required: '必須',
        def: '—',
        limit: `レイアウト id、または組み込み名（${BUILTIN_LAYOUT_IDS.join(' / ')}）`,
        desc: '切り替え先。kana と alpha は設定で割り当て済みのかな／英字レイアウトを指す。'
      }
    ],
    example: { id: 'to-num', kind: 'layout', target: 'numbers', label: '123' }
  },
  spacer: {
    summary: '空白（幅だけを取る）。描画も操作もされない。',
    fields: [],
    example: { id: 'gap', kind: 'spacer', width: 1 }
  }
}

export const POPUP_FIELDS: readonly FieldRow[] = [
  { name: 'id', type: 'string', required: '省略可', def: '—', limit: 'キー id と同じ書式', desc: '識別子。' },
  { name: 'label', type: 'string', required: '必須', def: '—', limit: `1〜${LIMITS.labelMax} 文字`, desc: 'ポップアップに表示する文字。' },
  { name: 'kind', type: 'string（列挙）', required: '必須', def: '—', limit: POPUP_KINDS.join(' / '), desc: 'text は text、action は action、shortcut は steps を追加で持つ（キーと同じ制約）。' }
]

export const POPUP_EXAMPLE = {
  id: 'w',
  kind: 'text',
  text: 'w',
  longPress: [
    { label: 'ŵ', kind: 'text', text: 'ŵ' },
    { label: '貼付', kind: 'action', action: 'paste' }
  ]
}

// ---- action ----------------------------------------------------------------------------------------------------------

export interface ActionDoc {
  /** 動作（rows のキーとして押したとき）。 */
  behavior: string
  /** ツールバーで icon / label が無いときの既定アイコン（SF Symbols）。無ければ undefined。 */
  icon?: string
  note?: string
}

/** KNOWN_ACTIONS に action を足したら、ここも足さないと型エラーになる。 */
export const ACTION_DOCS: Record<KnownAction, ActionDoc> = {
  space: {
    behavior: '入力中は変換、変換中は次の候補。未入力なら半角スペースを入れる。',
    icon: 'space',
    note: 'ツールバーでは入力中の文字列を確定して半角スペースを入れる（コアを通さない）。長い左右ドラッグはカーソル移動（設定 touch.space_swipe が ON で、スペースに longPress が無いとき）。'
  },
  enter: { behavior: '入力中・変換中は確定。未入力なら改行を入れる。', icon: 'return' },
  backspace: { behavior: '1 文字削除（入力中はコアの削除）。', note: '長押しで連打（状態 longPress に入る）。左スワイプで直前の単語を削除。' },
  shift: { behavior: 'Shift を切り替える。1 回で次の 1 文字だけ大文字、0.5 秒以内にもう一度で Caps Lock、Caps Lock 中の 1 回で解除。', note: 'かなのレイアウトでは何も起きない。状態 shift / capsLock に対応する。' },
  capslock: { behavior: 'Caps Lock を切り替える。', note: 'かなのレイアウトでは何も起きない。' },
  convert: { behavior: '変換を開始する。' },
  moveLeft: {
    behavior: '左へ動かす。変換中は注目文節、入力中はプリエディット内のカーソル、未入力ならホストの入力欄のカーソル。',
    icon: 'chevron.left'
  },
  moveRight: {
    behavior: '右へ動かす。変換中は注目文節、入力中はプリエディット内のカーソル、未入力ならホストの入力欄のカーソル。',
    icon: 'chevron.right'
  },
  dakuten: { behavior: '直前の文字の濁点・半濁点・小書きを循環させる。', note: 'smallKana と同じ動作。' },
  smallKana: { behavior: '直前の文字の濁点・半濁点・小書きを循環させる。', note: 'dakuten と同じ動作。' },
  modeSwitch: { behavior: 'かな ⇄ 英字を切り替える。かな以外（英字・数字・記号・絵文字）からはかなに戻る。' },
  globe: { behavior: '次のキーボード（入力モード）へ切り替える。', icon: 'globe' },
  dismiss: { behavior: 'キーボードを閉じる。', icon: 'keyboard.chevron.compact.down' },
  paste: {
    behavior: 'クリップボードの文字列を入力する。入力中の文字列は先に確定する。',
    icon: 'doc.on.clipboard',
    note:
      'ツールバー用だが rows のキーに置いても動く。ツールバーのキーはクリップボードに文字があるときだけ表示し、設定 candbar.paste_button がオフなら出さない。中身はタップしたときだけ読む（iOS の貼り付け許可の確認が出ることがある）。フルアクセスが無い・許可されないと何も入らない。'
  },
  deleteWord: {
    behavior: 'カーソル手前の単語を削除する。入力中（プリエディットあり）や、文脈が取れない入力欄では何もしない。',
    icon: 'delete.left',
    note: 'ツールバー用だが rows のキーに置いても動く。'
  },
  history: {
    behavior: 'クリップボード履歴のページを開く（開いていれば閉じる）。入力中の文字列は先に確定する。',
    icon: 'clock.arrow.circlepath',
    note: '履歴の設定がオフのときは、ページ内に案内を出す。設定は「iOS 設定キー」の節。'
  }
}

/** layout キーのツールバー既定アイコン（target ごと）。 */
export const LAYOUT_DEFAULT_ICONS: Readonly<Record<string, string>> = { emoji: 'face.smiling' }

// ---- look ------------------------------------------------------------------------------------------------------------

export interface LookPropDoc {
  def: string
  desc: string
}

/** LOOK_PROPS の全プロパティの既定値と意味（reference.test.ts が過不足を検査する）。 */
export const LOOK_PROP_DOCS: Record<string, LookPropDoc> = {
  fontSize: { def: 'label 3 文字以上 16、text キー 20、他 17（フリックは 22 / 3 文字以上 16）', desc: '文字の大きさ。アイコンの大きさもこれに従う（未指定のアイコンは 17pt）。' },
  fontWeight: { def: 'regular', desc: '文字の太さ。' },
  fontDesign: { def: 'system', desc: '書体系統。日本語はいずれもシステムの日本語フォントにフォールバックされ、rounded で丸ゴシック系になるとは限らない。' },
  color: { def: '役割に応じる（accent は白、他は @label）', desc: '文字・アイコンの色。' },
  fill: { def: '役割に応じる（normal=@key、function=@function、accent=@accent）', desc: 'キーの背景色。' },
  fillOpacity: { def: '1（押下中は 0.6）', desc: '背景（fill と画像）だけの不透明度。文字・枠線・影には掛からない。' },
  fillImage: { def: 'なし', desc: '背景画像の id（assets のキー）。"none" は下位で指定された画像を外す。' },
  fillImageFit: { def: 'fill', desc: '画像の当て方。fill=縦横比を保って覆う（はみ出しは切る）、fit=縦横比を保って収める（余白は fill の色）、tile=元の大きさ（1pt=3px）で敷き詰める。' },
  fillImageOpacity: { def: '1', desc: '画像だけの不透明度。' },
  cornerRadius: { def: '設定 keyboard.key_corner_pt（既定 6）', desc: '角丸。キーの短辺の半分で頭打ち。' },
  borderWidth: { def: '0', desc: '枠線の太さ（内側に描く）。' },
  borderColor: { def: '@separator', desc: '枠線の色。borderWidth が 0 なら無効。' },
  shadowColor: { def: '#000000', desc: 'ドロップシャドウの色。' },
  shadowRadius: { def: '0（影なし）', desc: '影のぼかし半径。0 のとき影は描かない。' },
  shadowX: { def: '0', desc: '影の横オフセット。' },
  shadowY: { def: '0', desc: '影の縦オフセット。' },
  shadowOpacity: { def: '0.3', desc: '影の不透明度（shadowColor のアルファに掛ける）。' },
  opacity: { def: '1', desc: 'キー全体（背景・文字・枠線・影）の不透明度。' },
  scale: { def: '1', desc: 'キーの表示の拡縮（中心基準）。当たり判定は変わらない。' },
  offsetX: { def: '0', desc: '表示位置の横ずらし。当たり判定は変わらない。' },
  offsetY: { def: '0', desc: '表示位置の縦ずらし。当たり判定は変わらない。' },
  inset: { def: '0', desc: 'キーの描画範囲を全方向に縮める。当たり判定はセル全体のまま。' },
  padding: { def: '0', desc: '文字・アイコンの内側余白。長い文字は 0.6 倍まで縮む。' },
  animationMs: { def: '0（即時）', desc: 'この指定への切り替わりの長さ。前後の解決結果の大きいほうの長さで easeInOut 補間する。「視差効果を減らす」が有効なときは 0 として扱う。label・icon・fillImage は補間せず即座に切り替わる。' },
  label: { def: '—', desc: '表示する文字の上書き。states / rules の中だけで有効。' },
  icon: { def: '—', desc: '表示するアイコンの上書き（icon の書式）。states / rules の中だけで有効。' }
}

/** プロパティの型・範囲を表示用の文字列にする。 */
export function describeLookSpec(name: string): { type: string; limit: string } {
  const spec = LOOK_PROPS[name]
  switch (spec.type) {
    case 'number':
      return { type: 'number', limit: `${spec.min}〜${spec.max}` }
    case 'enum':
      return { type: 'string（列挙）', limit: spec.values.join(' / ') }
    case 'color':
      return { type: 'Color', limit: '「色の形式」の節' }
    case 'imageRef':
      return { type: 'string', limit: '画像 id または "none"' }
    case 'label':
      return { type: 'string', limit: `1〜${LOOK_LIMITS.labelMax} 文字` }
    case 'icon':
      return { type: 'string', limit: `icon の書式、${LOOK_LIMITS.iconMax} 文字以内` }
  }
}

/** 単位つきの補足（数値の単位）。 */
export const LOOK_UNITS: Readonly<Record<string, string>> = {
  fontSize: 'pt',
  cornerRadius: 'pt',
  borderWidth: 'pt',
  shadowRadius: 'pt',
  shadowX: 'pt',
  shadowY: 'pt',
  offsetX: 'pt',
  offsetY: 'pt',
  inset: 'pt',
  padding: 'pt',
  animationMs: 'ms'
}

export const LOOK_ENUM_LISTS = { fontWeight: LOOK_FONT_WEIGHTS, fontDesign: LOOK_FONT_DESIGNS, fillImageFit: LOOK_IMAGE_FITS } as const

export const ROLE_DOCS: Record<(typeof KEY_ROLES)[number], string> = {
  normal: '通常のキー。背景 @key、文字 @label。',
  function: '機能キー。背景 @function、文字 @label。',
  accent: '強調するキー（確定など）。背景 @accent、文字は白。'
}

export const TOKEN_DOCS: Record<LookToken, string> = {
  label: '文字色（UIColor.label）',
  secondaryLabel: '薄い文字色（UIColor.secondaryLabel）',
  key: '通常キーの背景（UIColor.systemBackground）',
  function: '機能キーの背景（UIColor.systemGray4）',
  accent: 'アクセントカラー（設定 keyboard.accent）',
  separator: '区切り線（UIColor.separator）',
  clear: '透明（UIColor.clear）'
}

export const COLOR_FORMS: readonly { form: string; example: string; desc: string }[] = [
  { form: '#RGB / #RRGGBB / #RRGGBBAA', example: '"#E0B341"', desc: 'sRGB。ライト・ダーク共通。' },
  { form: '@トークン', example: '"@accent"', desc: '現在のテーマ・アクセントに追従する色。トークンは下の表。' },
  {
    form: '{ light, dark }',
    example: '{ "light": "#FFFFFF", "dark": "#3A3A3C" }',
    desc: 'ライト／ダークで別の色。各値は上の 2 つのどちらか。片方だけ書いた場合、書いていない側の外観ではこのプロパティは未指定扱い（下位の値が使われる）。'
  }
]

// ---- 状態・rules・重ね順 ------------------------------------------------------------------------------------------------

export interface StateDoc {
  enter: string
  leave: string
  note?: string
}

export const STATE_DOCS: Record<StateName, StateDoc> = {
  pressed: {
    enter: '指がそのキーに触れた瞬間',
    leave: '指が離れた／ジェスチャが取り消された',
    note: 'フリック中・長押し中・スペースのドラッグ中も真のまま。組み込みの既定として fillOpacity 0.6 が入る。'
  },
  released: {
    enter: 'pressed が正常な終了で抜けた瞬間',
    leave: 'holdMs（既定 200、0〜2000）経過後。その間に同じキーが再び押されたら即座に',
    note: 'システムにジェスチャを奪われた取り消しでは入らない。'
  },
  longPress: {
    enter: 'longPress を持つキーはポップアップが出る瞬間（flick.long_press_ms 経過）。⌫ は連打が始まる瞬間',
    leave: '指が離れた／取り消された',
    note: 'それ以外のキーは長押しの判定自体が無いので入らない。'
  },
  flicking: {
    enter: 'フリックキーで、移動量が閾値（flick.threshold_pt）を超え、取り消しの範囲の外にいる間',
    leave: '閾値内に戻った／取り消しの範囲に出た／離した'
  },
  spaceDrag: {
    enter: 'スペースキーを左右に touch.space_swipe_step_pt 以上動かした時点',
    leave: '指が離れた／取り消された',
    note: '設定 touch.space_swipe が ON で、スペースに longPress が無いときだけ入る。起点（スペース）以外のキーには target "others" で指せる。'
  },
  shift: { enter: 'Shift が 1 文字だけ有効（once）になったとき', leave: 'オフまたは Caps Lock になったとき' },
  capsLock: { enter: 'Caps Lock になったとき', leave: 'オフになったとき' },
  composing: { enter: '入力中の文字列（プリエディット）があるとき', leave: 'なくなったとき' }
}

export const TARGET_DOCS: readonly { form: string; desc: string }[] = [
  { form: '"all"（省略時）', desc: '全キー。' },
  { form: '"others"', desc: '起点以外の全キー。when が spaceDrag のときだけ有効（他では "all" と同じ）。' },
  { form: `{ "keys": ["id", …] }`, desc: `キー id で指す（最大 ${LOOK_LIMITS.targetKeysMax} 個）。レイアウトに無い id は無視する。ツールバーのキー id も指せる。` },
  { form: `{ "roles": ["function", …] }`, desc: '役割で指す。' },
  { form: `{ "actions": ["space", …] }`, desc: 'action 名で指す。' }
]

export const RESOLVE_ORDER: readonly string[] = [
  '既定（現行の見た目。役割に応じた色・フォント・角丸）',
  'レイアウトの look',
  'レイアウトの roles[そのキーの role]',
  'その行の look',
  'そのキーの look',
  '有効な状態を、優先順位の低い順に重ねる。同じ状態の中では「組み込みの既定（pressed の fillOpacity 0.6）→ rules（配列の先頭から）→ そのキーの states[状態]」の順'
]

export const TOOLBAR_RESOLVE_ORDER: readonly string[] = [
  '既定',
  `ツールバーの既定（fontSize ${TOOLBAR_DEFAULT_LOOK.fontSize}、inset ${TOOLBAR_DEFAULT_LOOK.inset}）`,
  'レイアウトの look',
  'レイアウトの roles[そのキーの role]',
  'toolbar.look',
  'そのキーの look',
  '状態（rules → states）'
]

export const PRECEDENCE_TEXT = STATE_PRECEDENCE.join(' < ')

export const PARTIAL_OVERRIDE_RULES: readonly string[] = [
  '上書きはプロパティ単位。上位が指定したプロパティだけが下位の値を置き換える。',
  '値が無効（範囲外・型違い・不正な色）または null のプロパティは、指定が無かったものとして下位の値が残る。範囲の丸め込み（クランプ）はしない。"fillImage": "none" だけが下位の画像を外す明示指定。',
  'pressed の fillOpacity 0.6 は、通常の look.fillOpacity では置き換わらない。置き換えるには pressed の状態（rules か states）に fillOpacity を書く。押下の手応えを無くすなら pressed に "fillOpacity": 1。'
]

// ---- 画像 ------------------------------------------------------------------------------------------------------------

export const IMAGE_ROWS: readonly { item: string; value: string }[] = [
  { item: '形式', value: `${ASSET_LIMITS.mimeTypes.join(' / ')} の base64 data URI（HEIC・WebP は不可）` },
  { item: '枚数', value: `${ASSET_LIMITS.countMax} 枚以内` },
  { item: '1 枚の大きさ', value: `デコード後（base64 を戻したバイト）${ASSET_LIMITS.bytesMax / 1024}KB 以内` },
  { item: '合計', value: `${ASSET_LIMITS.totalMax / 1024}KB 以内` },
  { item: '長辺', value: `${ASSET_LIMITS.longSideMax}px 以内を推奨。超えても取り込めるが警告が出る` },
  { item: '画像 id', value: `英小文字・数字・- _、先頭は英小文字か数字、32 文字以内（${ref(String(ASSET_ID_PATTERN))}）` },
  { item: 'JSON 全体', value: `${LAYOUT_FILE_MAX_BYTES / 1024}KB（1.5MB）以内。assets を含むレイアウトは URL スキーム（nagi://layout?data=…、上限 1MB）では渡せない。ファイルかコピーで渡す` },
  { item: '1 辺が 8px 未満の画像', value: 'tile を指定しても fill として扱う' },
  { item: '実行時', value: '描画に必要になるまでデコードしない。デコード結果のキャッシュは 12MB まで。デコード中・失敗・メモリ逼迫（利用可能なメモリ 40MB 未満）・メモリ警告・キーボードの非表示の間は画像を使わず fill の色だけで描く' }
]

// ---- ツールバー ------------------------------------------------------------------------------------------------------

/** 設定 toolbar.items の項目名と、対応するキー（ToolbarKeys.spec(for:)）。 */
export const TOOLBAR_ITEMS: readonly { item: string; key: string }[] = [
  { item: 'dismiss', key: 'action dismiss' },
  { item: 'paste', key: 'action paste' },
  { item: 'cursorLeft', key: 'action moveLeft' },
  { item: 'cursorRight', key: 'action moveRight' },
  { item: 'newline', key: 'action enter' },
  { item: 'space', key: 'action space' },
  { item: 'deleteWord', key: 'action deleteWord' },
  { item: 'emoji', key: 'layout（target emoji）' },
  { item: 'nextKeyboard', key: 'action globe' },
  { item: 'history', key: 'action history' }
]

export const TOOLBAR_EXAMPLE = {
  toolbar: {
    look: { cornerRadius: 8 },
    keys: [
      { id: 'tb-dismiss', kind: 'action', action: 'dismiss' },
      { id: 'tb-paste', kind: 'action', action: 'paste' },
      { id: 'tb-history', kind: 'action', action: 'history' },
      { id: 'tb-left', kind: 'action', action: 'moveLeft' },
      { id: 'tb-right', kind: 'action', action: 'moveRight' }
    ]
  }
}

// ---- iOS 設定キー ------------------------------------------------------------------------------------------------------

export interface SettingRow {
  id: string
  desc: string
  type: string
  def: string
  note?: string
}

/** レイアウトに関係する設定（docs/ios-settings.md §2.1・§2.5 の抜粋）。 */
export const LAYOUT_SETTINGS: readonly SettingRow[] = [
  { id: 'keyboard.layout_kana', desc: 'かなモードのレイアウト', type: '組み込み（flick-kana / qwerty-kana）またはユーザー定義の id', def: 'flick-kana', note: '不正なレイアウトは既定に戻す。反映はキーボードを開き直したとき。' },
  { id: 'keyboard.layout_alpha', desc: '英字モードのレイアウト', type: 'flick-alpha / qwerty-alpha またはユーザー定義の id', def: 'qwerty-alpha' },
  { id: 'keyboard.layout_number', desc: '数字モードのレイアウト', type: 'numbers またはユーザー定義の id', def: 'numbers' },
  { id: 'keyboard.layout_symbol', desc: '記号モードのレイアウト', type: 'symbols またはユーザー定義の id', def: 'symbols' },
  { id: 'keyboard.layout_emoji', desc: '絵文字モードのレイアウト', type: 'emoji またはユーザー定義の id', def: 'emoji' },
  { id: 'keyboard.layout_by_field', desc: '入力欄の種類で自動切替（URL・メール・数字）', type: 'bool', def: 'true' },
  { id: 'keyboard.key_spacing_pt', desc: 'キーの余白', type: 'float 0〜8 pt', def: '4', note: 'レイアウトの metrics が優先。' },
  { id: 'keyboard.key_corner_pt', desc: 'キーの角丸', type: 'float 0〜16 pt', def: '6', note: 'look.cornerRadius が優先。' },
  { id: 'keyboard.theme', desc: 'テーマ', type: 'system / light / dark', def: 'system', note: 'look の { light, dark } の判定に使われる外観。' },
  { id: 'keyboard.accent', desc: 'アクセントカラー', type: 'blue / green / orange / pink / gray', def: 'blue', note: '@accent の色。' },
  { id: 'touch.space_swipe', desc: 'スペースキーのスワイプでカーソル移動', type: 'bool', def: 'true', note: '状態 spaceDrag が入る条件。' },
  { id: 'touch.space_swipe_step_pt', desc: 'カーソル移動 1 文字あたりの距離', type: 'float 4〜40 pt', def: '14' },
  { id: 'flick.long_press_ms', desc: '長押しと判定する時間', type: 'int 150〜1000（50 刻み）', def: '350', note: '状態 longPress と longPress ポップアップの起点。' },
  { id: 'toolbar.items', desc: 'ツールバーの項目と並び順（レイアウトに toolbar が無いときだけ使う）', type: 'list。項目名は「ツールバー」の節の表', def: '[dismiss, paste, cursorLeft, cursorRight]', note: '知らない名前と重複は捨てる。空配列は何も出さない。' },
  { id: 'candbar.paste_button', desc: 'ツールバーに「貼り付け」を出す', type: 'bool', def: 'true', note: 'オフなら toolbar.items やレイアウトの toolbar に paste があっても出さない。' }
]

/** docs/ios-settings.md §2.5.1。 */
export const CLIPBOARD_SETTINGS: readonly SettingRow[] = [
  { id: 'clipboard.history.enabled', desc: 'クリップボード履歴を使う', type: 'bool', def: 'false', note: 'オフの間はクリップボードを一切読まず、保存もしない。フルアクセスが必要。' },
  { id: 'clipboard.history.retention', desc: '保存期間', type: '1h / 24h / 7d / 30d / forever', def: '24h', note: 'ピン留めは期限で消さない。' },
  { id: 'clipboard.history.max_items', desc: '保存する最大数', type: '10 / 30 / 50 / 100 / 200', def: '50', note: '超えたら古い順に消す。ピン留めは数に含めず消さない（ピン留めは 20 件まで）。' },
  { id: 'clipboard.history.types', desc: '保存する種類', type: '複数選択: text / url / number / email', def: '全部', note: 'number は数字・記号のみ（電話番号・コード等）。' },
  { id: 'clipboard.history.preview_chars', desc: 'カードに表示する文字数', type: '20 / 40 / 80 / 120', def: '40', note: '超えた分は「…」で省略する。入力は全文。' },
  { id: 'clipboard.history.clear', desc: '履歴を全て削除', type: '操作（確認つき）', def: '—', note: '設定画面のボタン。履歴ページにも 2 度押しの全削除がある。' }
]

export const CLIPBOARD_CAPTURE_TIMING: readonly string[] = [
  'iOS のキーボード拡張はバックグラウンドで監視できないため、取り込むのは次のときだけ: (1) キーボードの表示時と UIPasteboard.changedNotification、(2) ホストアプリが前面になったとき、(3)「貼り付け」ボタンで読んだとき。',
  'changeCount が前回記録した値（ホストと拡張で共有）と違うときだけ読み、hasStrings を先に確認する。',
  'キーボードもアプリも動いていない間にコピーしたものは取りこぼすことがある。'
]

export const CLIPBOARD_PRIVACY: readonly string[] = [
  '既定はオフ。オフのときは読まない・保存しない。フルアクセスが無いときは記録しない。',
  'パスワード欄（isSecureTextEntry・パスワード／ワンタイムコードの textContentType）では取り込みも履歴ページの表示もしない。',
  '1 件 10000 文字を超えるものは記録しない。文字列のみを保存する（画像・ファイルは保存しない）。',
  '内容は診断ログに出さない。保存先は端末内の App Group の clipboard-history.json のみ（バックアップ対象外）。iCloud 等での同期はしない。',
  '履歴ページの項目をタップすると、入力中の文字列を確定してからその内容を挿入する。長押しでピン留め／削除。'
]

// ---- 検証メッセージ ----------------------------------------------------------------------------------------------------

export interface ErrorDoc {
  group: string
  /** 画面に出るメッセージ（動的な部分は N などで表す）。 */
  message: string
  /** reference.test.ts で突き合わせる、メッセージの固定部分（省略時は message 全体）。 */
  match?: string
  severity: 'error' | 'warning'
  cause: string
  fix: string
  /** このメッセージが出るレイアウト（または JSON 文字列）。test が validate.ts に通して確かめる。 */
  sample?: () => unknown
}

const base = () => structuredClone(BASE_LAYOUT) as unknown as Record<string, any>
const withKey = (key: Record<string, unknown>, rest: Record<string, unknown> = {}) => ({ ...base(), rows: [{ keys: [key] }], ...rest })
const withLook = (look: unknown) => ({ ...base(), look })
const manyKeys = (n: number, width = 1) => Array.from({ length: n }, (_, i) => ({ id: `k${i}`, kind: 'spacer', width }))

export const ERROR_DOCS: readonly ErrorDoc[] = [
  { group: '全体', message: 'JSON として読み込めません: …', match: 'JSON として読み込めません', severity: 'error', cause: 'JSON の構文エラー（カンマ・引用符・括弧など）。', fix: '構文を直す。メッセージ末尾に位置が出る。', sample: () => '{' },
  { group: '全体', message: 'レイアウトは JSON オブジェクトである必要があります', severity: 'error', cause: '最上位が配列・文字列などになっている。', fix: '{ … } の形にする。', sample: () => [] },
  { group: '全体', message: '未知のフィールド「x」は指定できません', match: '未知のフィールド「foo」は指定できません', severity: 'error', cause: 'スキーマにない名前のフィールド（レイアウト・行・キー・手順・toolbar など）。綴りの間違いに多い。', fix: 'フィールド名を直すか、消す。', sample: () => ({ ...base(), foo: 1 }) },
  { group: '全体', message: `schema は ${SCHEMA_VERSION} にしてください`, severity: 'error', cause: 'schema が 2 以外。1 の場合は移行の案内も付く。', fix: 'schema を 2 にする。schema 1 の JSON は web/scripts/migrate-layout-v2.mjs で移行する（style → role などの改名を含む）。', sample: () => ({ ...base(), schema: 1 }) },
  { group: '全体', message: 'id は英小文字・数字・- _ で始まりが英小文字か数字、64 文字以内にしてください', severity: 'error', cause: 'レイアウト id の書式違反（大文字・空白・日本語など）。', fix: '英小文字・数字・- _ にする。', sample: () => ({ ...base(), id: 'A b' }) },
  { group: '全体', message: 'nameは空にできません', severity: 'error', cause: 'name が空文字。', fix: '1 文字以上にする。', sample: () => ({ ...base(), name: '' }) },
  { group: '全体', message: `nameは ${LIMITS.nameMax} 文字以内にしてください（現在 N 文字）`, match: `nameは ${LIMITS.nameMax} 文字以内にしてください`, severity: 'error', cause: 'name が長すぎる。', fix: `${LIMITS.nameMax} 文字以内にする。`, sample: () => ({ ...base(), name: 'あ'.repeat(LIMITS.nameMax + 1) }) },
  { group: '全体', message: `mode は ${LAYOUT_MODES.join(' / ')} のいずれかにしてください`, severity: 'error', cause: 'mode が列挙にない。', fix: '列挙のどれかにする。', sample: () => ({ ...base(), mode: 'abc' }) },
  { group: '行', message: 'rows は配列で指定してください', severity: 'error', cause: 'rows が配列でない。', fix: '[ { "keys": […] } ] の形にする。', sample: () => ({ ...base(), rows: {} }) },
  { group: '行', message: `行数は 1〜${LIMITS.rowsMax} にしてください（現在 N）`, match: `行数は 1〜${LIMITS.rowsMax} にしてください`, severity: 'error', cause: '行が 0 行、または 9 行以上。', fix: `1〜${LIMITS.rowsMax} 行にする。`, sample: () => ({ ...base(), rows: [] }) },
  { group: '行', message: `行の高さ比の合計は ${LIMITS.rowHeightSumMax} 以下にしてください（現在 N）`, match: `行の高さ比の合計は ${LIMITS.rowHeightSumMax} 以下にしてください`, severity: 'error', cause: 'height（省略は 1）の合計が大きすぎる。キーボード全体の高さに収まらない。', fix: '行を減らすか、各行の height を小さくする。', sample: () => ({ ...base(), rows: Array.from({ length: 5 }, (_, i) => ({ keys: [{ id: `r${i}`, kind: 'spacer' }] })) }) },
  { group: '行', message: `height は 0 より大きく ${LIMITS.rowHeightMax} 以下の数にしてください`, severity: 'error', cause: '行の height が範囲外。', fix: `0 より大きく ${LIMITS.rowHeightMax} 以下にする。`, sample: () => ({ ...base(), rows: [{ height: 0, keys: [{ id: 'a', kind: 'spacer' }] }] }) },
  { group: '行', message: '行には 1 つ以上のキーが必要です', severity: 'error', cause: 'keys が空。', fix: 'キーを足すか、行ごと消す。', sample: () => ({ ...base(), rows: [{ keys: [] }] }) },
  { group: '行', message: `1 行のキーは ${LIMITS.keysPerRowMax} 個以内にしてください`, severity: 'error', cause: 'キーが多すぎる。', fix: '行を分ける。', sample: () => ({ ...base(), rows: [{ keys: manyKeys(LIMITS.keysPerRowMax + 1, 0.5) }] }) },
  { group: '行', message: `行内の幅の合計は ${LIMITS.rowWidthMin}〜${LIMITS.rowWidthMax} にしてください（現在 N）`, match: `行内の幅の合計は ${LIMITS.rowWidthMin}〜${LIMITS.rowWidthMax} にしてください`, severity: 'error', cause: '行の width（省略は 1）の合計が範囲外。', fix: 'width を調整する。', sample: () => ({ ...base(), rows: [{ keys: manyKeys(2, 0.1) }] }) },
  { group: 'キー', message: 'キー id の重複: 「x」が rows[0].keys[0] と重複しています', match: 'キー id の重複: 「a」が rows[0].keys[0] と重複しています', severity: 'error', cause: 'rows と toolbar を通して同じ id が 2 回ある。', fix: 'id を変える。', sample: () => ({ ...base(), rows: [{ keys: [{ id: 'a', kind: 'spacer' }, { id: 'a', kind: 'spacer' }] }] }) },
  { group: 'キー', message: 'id は英数字・_ . - で 32 文字以内にしてください', severity: 'error', cause: 'キー id の書式違反。', fix: '英数字・_ . - にする（先頭は英数字）。', sample: () => withKey({ id: 'a b', kind: 'spacer' }) },
  { group: 'キー', message: `kind は ${KEY_KINDS.join(' / ')} のいずれかにしてください`, severity: 'error', cause: 'kind が無い、または列挙にない。', fix: '列挙のどれかにする。', sample: () => withKey({ id: 'a', kind: 'x' }) },
  { group: 'キー', message: `width は 0 より大きく ${LIMITS.widthMax} 以下の数にしてください`, severity: 'error', cause: 'width が範囲外。', fix: '範囲内にする。', sample: () => withKey({ id: 'a', kind: 'spacer', width: 0 }) },
  { group: 'キー', message: `role は ${KEY_ROLES.join(' / ')} のいずれかにしてください`, severity: 'error', cause: 'role が列挙にない（旧 style の値を role に書いた場合など）。', fix: '列挙のどれかにする。', sample: () => withKey({ id: 'a', kind: 'spacer', role: 'x' }) },
  { group: 'キー', message: `textは ${LIMITS.textMax} 文字以内にしてください（現在 N 文字）`, match: `textは ${LIMITS.textMax} 文字以内にしてください`, severity: 'error', cause: 'text / shift / flick の文字が長い。長い定型文は shortcut の text（256 文字以内）を使う。', fix: `${LIMITS.textMax} 文字以内にする。`, sample: () => withKey({ id: 'a', kind: 'text', text: 'a'.repeat(LIMITS.textMax + 1) }) },
  { group: 'キー', message: 'textは文字列で指定してください', severity: 'error', cause: '必須の text が無い、または文字列でない。', fix: '文字列を書く。', sample: () => withKey({ id: 'a', kind: 'text' }) },
  { group: 'キー', message: 'フリックには center（中央の文字）か kana が必要です', severity: 'error', cause: 'flick キーに center も kana も無い。', fix: 'どちらかを書く。', sample: () => withKey({ id: 'a', kind: 'flick' }) },
  { group: 'キー', message: `cycle は 1〜${LIMITS.cycleMax} 要素にしてください`, severity: 'error', cause: 'toggle の cycle が空、または多すぎる。', fix: `1〜${LIMITS.cycleMax} 要素にする。`, sample: () => withKey({ id: 'a', kind: 'toggle', cycle: [] }) },
  { group: 'キー', message: 'action は英字で始まる英数字（32 文字以内）で指定してください（例: space, backspace）', severity: 'error', cause: 'action が無い、または書式違反。', fix: '英字で始まる英数字にする。', sample: () => withKey({ id: 'a', kind: 'action', action: '1x' }) },
  { group: 'キー', message: '手順は action か text のどちらか一方だけを指定してください', severity: 'error', cause: 'steps の要素が両方、または両方とも無い。', fix: '一方だけにする。', sample: () => withKey({ id: 'a', kind: 'shortcut', steps: [{ action: 'space', text: 'x' }] }) },
  { group: 'キー', message: `steps は 1 つ以上必要です / steps は ${LIMITS.stepsMax} 個以内にしてください`, match: 'steps は 1 つ以上必要です', severity: 'error', cause: 'shortcut の steps が空、または多すぎる。', fix: `1〜${LIMITS.stepsMax} 個にする。`, sample: () => withKey({ id: 'a', kind: 'shortcut', steps: [] }) },
  { group: 'キー', message: '切り替え先「x」は存在するレイアウトでも組み込み名でもありません', match: '切り替え先「nope」は存在するレイアウトでも組み込み名でもありません', severity: 'error', cause: 'layout の target が、このレイアウト自身・同じエディタ内の他のレイアウト・組み込み名のどれでもない。', fix: 'target を直す。組み込み名は layout キーの節。', sample: () => withKey({ id: 'a', kind: 'layout', target: 'nope' }) },
  { group: 'キー', message: 'target は切り替え先のレイアウト id（英小文字・数字・- _）で指定してください', severity: 'error', cause: 'layout の target が無い、または書式違反。', fix: 'レイアウト id を書く。', sample: () => withKey({ id: 'a', kind: 'layout' }) },
  { group: 'キー', message: `longPress は 1〜${LIMITS.longPressMax} 個にしてください（不要なら項目ごと削除）`, severity: 'error', cause: 'longPress が空配列、または多すぎる。', fix: '項目ごと消すか、個数を減らす。', sample: () => withKey({ id: 'a', kind: 'text', text: 'a', longPress: [] }) },
  { group: 'キー', message: `長押しの kind は ${POPUP_KINDS.join(' / ')} のいずれかにしてください`, severity: 'error', cause: 'ポップアップの kind が列挙にない（flick・toggle などは不可）。', fix: '列挙のどれかにする。', sample: () => withKey({ id: 'a', kind: 'text', text: 'a', longPress: [{ label: 'x', kind: 'flick' }] }) },
  { group: 'キー', message: 'icon の書式（iconify: / sf:）違反', match: 'iconify: の書式は iconify:<prefix>:<name>', severity: 'error', cause: 'iconify: や sf: で始まる icon が書式に合わない。', fix: 'iconify:<prefix>:<name>（小文字英数字とハイフン）か sf:<名前> にする。', sample: () => withKey({ id: 'a', kind: 'spacer', icon: 'iconify:Bad' }) },
  { group: 'ツールバー', message: `toolbar のキーは ${LIMITS.toolbarKeysMin}〜${LIMITS.toolbarKeysMax} 個にしてください（現在 N）。不要なら toolbar ごと削除`, match: `toolbar のキーは ${LIMITS.toolbarKeysMin}〜${LIMITS.toolbarKeysMax} 個にしてください`, severity: 'error', cause: 'toolbar.keys が空、または 13 個以上。', fix: 'toolbar ごと消すか、個数を直す。', sample: () => ({ ...base(), toolbar: { keys: [] } }) },
  { group: 'ツールバー', message: `toolbar の幅の合計が範囲外です（${LIMITS.rowWidthMin}〜${LIMITS.rowWidthMax} にしてください。現在 N）`, match: `toolbar の幅の合計が範囲外です（${LIMITS.rowWidthMin}〜${LIMITS.rowWidthMax} にしてください`, severity: 'error', cause: 'toolbar.keys の width の合計が範囲外。', fix: 'width を調整する。', sample: () => ({ ...base(), toolbar: { keys: [{ id: 't', kind: 'spacer', width: 0.5 }] } }) },
  { group: '資源の上限', message: `rules は ${LOOK_LIMITS.rulesMax} 個以内にしてください（現在 N）`, match: `rules は ${LOOK_LIMITS.rulesMax} 個以内にしてください`, severity: 'error', cause: 'rules が 33 個以上。', fix: 'まとめるか、キー側の states に移す。', sample: () => ({ ...base(), rules: Array.from({ length: LOOK_LIMITS.rulesMax + 1 }, () => ({ when: 'pressed', look: { opacity: 0.9 } })) }) },
  { group: '資源の上限', message: `target.keys は ${LOOK_LIMITS.targetKeysMax} 個以内にしてください（現在 N）`, match: `target.keys は ${LOOK_LIMITS.targetKeysMax} 個以内にしてください`, severity: 'error', cause: 'rule の target.keys が 25 個以上。', fix: 'roles や actions で指すか、rule を分ける。', sample: () => ({ ...base(), rules: [{ when: 'pressed', target: { keys: Array.from({ length: LOOK_LIMITS.targetKeysMax + 1 }, (_, i) => `k${i}`) }, look: { opacity: 0.9 } }] }) },
  { group: '資源の上限', message: 'assets は { "画像 id": "data URI" } のオブジェクトで指定してください', severity: 'error', cause: 'assets がオブジェクトでない。', fix: '{ "id": "data:image/png;base64,…" } の形にする。', sample: () => ({ ...base(), assets: [] }) },
  { group: '資源の上限', message: `画像は ${ASSET_LIMITS.countMax} 枚以内にしてください（現在 N）`, match: `画像は ${ASSET_LIMITS.countMax} 枚以内にしてください`, severity: 'error', cause: 'assets が 5 枚以上。', fix: '使う画像を減らす。', sample: () => ({ ...base(), assets: Object.fromEntries(Array.from({ length: ASSET_LIMITS.countMax + 1 }, (_, i) => [`img${i}`, TINY_PNG])) }) },
  { group: '資源の上限', message: '画像 id は英小文字・数字・- _ で始まりが英小文字か数字、32 文字以内にしてください', severity: 'error', cause: '画像 id の書式違反。', fix: '英小文字・数字・- _ にする。', sample: () => ({ ...base(), assets: { Bad: TINY_PNG } }) },
  { group: '資源の上限', message: `画像は ${ASSET_LIMITS.mimeTypes.join(' か ')} の base64 data URI で指定してください`, severity: 'error', cause: '形式が data URI でない、MIME が PNG / JPEG 以外、base64 が不正。', fix: 'PNG か JPEG を base64 data URI にする。', sample: () => ({ ...base(), assets: { a: 'data:image/gif;base64,AAAA' } }) },
  { group: '資源の上限', message: `画像は 1 枚 ${ASSET_LIMITS.bytesMax / 1024} KB 以内にしてください（現在 N KB）`, match: `画像は 1 枚 ${ASSET_LIMITS.bytesMax / 1024} KB 以内にしてください`, severity: 'error', cause: '1 枚がデコード後 256KB を超える。', fix: '縮小・圧縮する（エディタの画像追加は自動で縮小する）。', sample: () => ({ ...base(), assets: { a: `data:image/png;base64,${'A'.repeat(Math.ceil((ASSET_LIMITS.bytesMax * 4) / 3 / 4) * 4 + 4)}` } }) },
  { group: '見た目（警告）', message: '未知のプロパティ「x」は無視されます', match: '未知のプロパティ「foo」は無視されます', severity: 'warning', cause: 'look / roles / metrics の名前が表にない。', fix: '綴りを直すか消す。', sample: () => withLook({ foo: 1 }) },
  { group: '見た目（警告）', message: '「x」は無視されます: 範囲・形式の説明', match: '「cornerRadius」は無視されます: 0〜24 の数で指定してください', severity: 'warning', cause: '範囲外・型違い・不正な色。丸め込みはされず、そのプロパティだけが無視される。', fix: 'メッセージの範囲・形式に合わせる。', sample: () => withLook({ cornerRadius: 99 }) },
  { group: '見た目（警告）', message: '「label」は states / rules の中だけで有効です（ここでは無視されます）', match: '「label」は states / rules の中だけで有効です', severity: 'warning', cause: 'look 本体・roles・行・toolbar の look に label / icon を書いた。', fix: 'キーの label / icon を使うか、states / rules の中に移す。', sample: () => withLook({ label: 'x' }) },
  { group: '見た目（警告）', message: '「x」は背景には使えません（無視されます）', match: '「color」は背景には使えません', severity: 'warning', cause: `background に ${BACKGROUND_PROPS.join(' / ')} 以外を書いた。`, fix: '使えるプロパティだけにする。', sample: () => ({ ...base(), background: { color: '#000000' } }) },
  { group: '見た目（警告）', message: '未知の状態「x」は無視されます', match: '未知の状態「foo」は無視されます', severity: 'warning', cause: 'states のキーが状態名にない。', fix: '状態名の表から選ぶ。', sample: () => withKey({ id: 'a', kind: 'text', text: 'a', states: { foo: {} } }) },
  { group: '見た目（警告）', message: `when は ${LOOK_STATES.join(' / ')} のいずれかにしてください（この rule は無視されます）`, severity: 'warning', cause: 'rule の when が状態名にない。この rule 全体が無視される。', fix: '状態名にする。', sample: () => ({ ...base(), rules: [{ when: 'x', look: { opacity: 0.9 } }] }) },
  { group: '見た目（警告）', message: 'target は "all" / "others" / { keys } / { roles } / { actions } のいずれか 1 つにしてください（この rule は無視されます）', severity: 'warning', cause: 'target の形が違う（複数指定など）。', fix: '表の形のどれか 1 つにする。', sample: () => ({ ...base(), rules: [{ when: 'pressed', target: { keys: ['a'], roles: ['normal'] }, look: { opacity: 0.9 } }] }) },
  { group: '見た目（警告）', message: `holdMs は ${LOOK_LIMITS.holdMsMin}〜${LOOK_LIMITS.holdMsMax} の数で指定してください（無視されます）`, severity: 'warning', cause: 'holdMs が範囲外。', fix: '範囲内にする。', sample: () => ({ ...base(), rules: [{ when: 'released', holdMs: 5000, look: { opacity: 0.9 } }] }) },
  { group: '見た目（警告）', message: 'look がありません（この rule は何も変えません）', severity: 'warning', cause: 'rule に look が無い。', fix: 'look を書くか、rule を消す。', sample: () => ({ ...base(), rules: [{ when: 'pressed' }] }) },
  { group: '見た目（警告）', message: '未知の役割「x」は無視されます', match: '未知の役割「foo」は無視されます', severity: 'warning', cause: 'roles のキー、または target.roles の値が役割名にない。', fix: `${KEY_ROLES.join(' / ')} にする。`, sample: () => ({ ...base(), roles: { foo: {} } }) },
  { group: '見た目（警告）', message: `画像の長辺は ${ASSET_LIMITS.longSideMax}px 以内を推奨します（現在 Npx。…）`, match: `画像の長辺は ${ASSET_LIMITS.longSideMax}px 以内を推奨します`, severity: 'warning', cause: '画像の長辺が 1024px を超える。取り込みはできる。', fix: '縮小する。' }
]
