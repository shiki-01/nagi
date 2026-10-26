<script lang="ts">
  import Meta from '$lib/components/Meta.svelte'
  import CodeExample from '$lib/components/site/CodeExample.svelte'
  import TableWrap from '$lib/components/site/TableWrap.svelte'
  import {
    ASSET_LIMITS,
    BUILTIN_LAYOUT_IDS,
    KEY_KINDS,
    KNOWN_ACTIONS,
    LAYOUT_FILE_MAX_BYTES,
    LIMITS,
    LOOK_LIMITS,
    LOOK_STATES,
    LOOK_STATE_INFO,
    LOOK_TOKENS,
    METRICS_PROPS,
    SCHEMA_VERSION,
    STATE_PRECEDENCE,
    TOOLBAR_ROW
  } from '$lib/keyboard/types'
  import {
    ACTION_DOCS,
    BASE_LAYOUT,
    CLIPBOARD_CAPTURE_TIMING,
    CLIPBOARD_PRIVACY,
    CLIPBOARD_SETTINGS,
    COLOR_FORMS,
    COMMON_KEY_FIELDS,
    ERROR_DOCS,
    IMAGE_ROWS,
    KIND_DOCS,
    LAYOUT_DEFAULT_ICONS,
    LAYOUT_SETTINGS,
    LOOK_PROP_DOCS,
    LOOK_UNITS,
    PARTIAL_OVERRIDE_RULES,
    POPUP_EXAMPLE,
    POPUP_FIELDS,
    RESOLVE_ORDER,
    ROLE_DOCS,
    STATE_DOCS,
    TARGET_DOCS,
    TOKEN_DOCS,
    TOOLBAR_EXAMPLE,
    TOOLBAR_ITEMS,
    TOOLBAR_RESOLVE_ORDER,
    TOP_FIELDS,
    describeLookSpec
  } from '$lib/reference/data'
  import type { FieldRow, SettingRow } from '$lib/reference/data'

  const json = (v: unknown) => JSON.stringify(v, null, 2)
  const errorGroups = [...new Set(ERROR_DOCS.map((e) => e.group))]

  const toc = [
    { id: 'overview', label: '概要' },
    { id: 'top', label: 'トップレベルのフィールド' },
    { id: 'keys', label: 'キーの種類とフィールド' },
    { id: 'actions', label: 'action 名一覧' },
    { id: 'look', label: '見た目（look・状態・rules）' },
    { id: 'toolbar', label: 'ツールバー' },
    { id: 'settings', label: 'iOS 設定キー' },
    { id: 'errors', label: '検証エラーと対処' },
    { id: 'limits', label: '利用者に関係する制約' }
  ]

  const lookNames = Object.keys(LOOK_PROP_DOCS)
  const layoutStateOrder = STATE_PRECEDENCE
  const sizeKB = (b: number) => `${b / 1024}KB`
</script>

{#snippet fieldTable(rows: readonly FieldRow[], label: string)}
  <TableWrap {label}>
    <table>
      <thead>
        <tr><th>フィールド</th><th>型</th><th>必須</th><th>既定</th><th>範囲・上限</th><th>説明</th></tr>
      </thead>
      <tbody>
        {#each rows as r (r.name)}
          <tr>
            <th scope="row"><code>{r.name}</code></th>
            <td>{r.type}</td>
            <td>{r.required}</td>
            <td>{r.def}</td>
            <td>{r.limit}</td>
            <td>{r.desc}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  </TableWrap>
{/snippet}

{#snippet settingTable(rows: readonly SettingRow[], label: string)}
  <TableWrap {label}>
    <table>
      <thead>
        <tr><th>キー</th><th>項目</th><th>型・値</th><th>既定</th><th>備考</th></tr>
      </thead>
      <tbody>
        {#each rows as r (r.id)}
          <tr>
            <th scope="row"><code>{r.id}</code></th>
            <td>{r.desc}</td>
            <td>{r.type}</td>
            <td>{r.def}</td>
            <td>{r.note ?? ''}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  </TableWrap>
{/snippet}

<Meta
  title="リファレンス"
  path="/reference"
  description="Nagi のキーボードレイアウト JSON のリファレンス。スキーマ 2 のフィールド、キーの種類、action 名、見た目（look・状態・rules）、ツールバー、iOS の設定キー、検証エラーの一覧。"
/>

<main class="doc ref">
  <header class="doc-head">
    <p class="eyebrow">リファレンス</p>
    <h1>レイアウト JSON リファレンス</h1>
    <p class="lead">
      Nagi（iOS 版）のキーボードレイアウト（JSON、schema {SCHEMA_VERSION}）の技術仕様と、レイアウトで使える項目のまとめです。
      編集は<a href="/custom-keyboard">カスタムキーボード</a>のページで行えます。開発中の機能を含み、仕様は変わることがあります。
    </p>
  </header>

  <div class="layout">
    <nav class="toc" aria-label="このページの目次">
      <p class="toc-title">目次</p>
      <ul>
        {#each toc as t (t.id)}
          <li><a href={`#${t.id}`}>{t.label}</a></li>
        {/each}
      </ul>
    </nav>

    <div class="body">
      <section aria-labelledby="overview">
        <h2 id="overview">概要</h2>
        <ul>
          <li>レイアウトは 1 つの JSON オブジェクトです。<code>schema</code> は {SCHEMA_VERSION} のみ受理します（schema 1 は未対応）。</li>
          <li>
            作り方は 2 通りです。<a href="/custom-keyboard">編集ページ</a>で作って書き出す方法と、JSON を直接書く方法です。
            本体アプリの「レイアウト」タブで、貼り付け・ファイル・<code>nagi://layout?data=…</code> のリンクから取り込みます。
          </li>
          <li>行（<code>rows</code>）にキー（<code>keys</code>）を並べます。候補バーが空のときに出る 1 行は <code>toolbar</code> で指定します。</li>
          <li>
            見た目（<code>look</code> ほか）は省略できます。省略したときの見た目は、組み込みのレイアウトと同じです。
          </li>
          <li>
            検証の結果は 2 種類です。<strong>エラー</strong>は構造の破損と資源の上限で、取り込みも書き出しもできません。
            <strong>警告</strong>は見た目の値の問題で、取り込めます。iOS は警告の対象のプロパティだけを無視して、下位の値（最終的には既定の見た目）で描きます。
          </li>
        </ul>
        <CodeExample caption="最小のレイアウト" code={json(BASE_LAYOUT)} />
        <table class="kv">
          <tbody>
            <tr><th scope="row">JSON 全体の上限</th><td>{sizeKB(LAYOUT_FILE_MAX_BYTES)}（assets を含められる大きさ）</td></tr>
            <tr><th scope="row">行数</th><td>1〜{LIMITS.rowsMax}（高さ比の合計は {LIMITS.rowHeightSumMax} 以下）</td></tr>
            <tr><th scope="row">1 行のキー数</th><td>1〜{LIMITS.keysPerRowMax}（width の合計は {LIMITS.rowWidthMin}〜{LIMITS.rowWidthMax}）</td></tr>
            <tr><th scope="row">ツールバーのキー数</th><td>{LIMITS.toolbarKeysMin}〜{LIMITS.toolbarKeysMax}</td></tr>
            <tr><th scope="row">rules</th><td>{LOOK_LIMITS.rulesMax} 個以内</td></tr>
            <tr><th scope="row">assets</th><td>{ASSET_LIMITS.countMax} 枚以内、1 枚 {sizeKB(ASSET_LIMITS.bytesMax)}、合計 {sizeKB(ASSET_LIMITS.totalMax)}</td></tr>
          </tbody>
        </table>
      </section>

      <section aria-labelledby="top">
        <h2 id="top">トップレベルのフィールド</h2>
        <p>これ以外のフィールドは、エラー「未知のフィールド」になります。</p>
        {@render fieldTable(TOP_FIELDS, 'トップレベルのフィールド')}
        {#each TOP_FIELDS as f (f.name)}
          <CodeExample caption={`${f.name} の最小例`} code={json(f.show ?? f.example)} />
        {/each}
      </section>

      <section aria-labelledby="keys">
        <h2 id="keys">キーの種類とフィールド</h2>
        <p>
          キーの <code>kind</code> は {KEY_KINDS.join(' / ')} の {KEY_KINDS.length} 種類です。次の共通フィールドに加えて、種類ごとのフィールドがあります。
          上のほかのフィールドは「未知のフィールド」のエラーです。
        </p>
        <h3 id="keys-common">共通フィールド</h3>
        {@render fieldTable(COMMON_KEY_FIELDS, '共通フィールド')}
        {#each KEY_KINDS as kind (kind)}
          {@const doc = KIND_DOCS[kind]}
          <h3 id={`kind-${kind}`}><code>{kind}</code></h3>
          <p>{doc.summary}</p>
          {#if doc.fields.length > 0}{@render fieldTable(doc.fields, `${kind} のフィールド`)}{/if}
          {#if doc.note}<p class="note">{doc.note}</p>{/if}
          <CodeExample caption={`${kind} の例`} code={json(doc.example)} />
        {/each}
        <h3 id="kind-popup">長押しの項目（<code>longPress</code>）</h3>
        <p>
          長押し（<code>flick.long_press_ms</code>、既定 350ms）でキーの上にポップアップが出ます。指を滑らせて選び、離すと実行します。
          ポップアップが出る前に離せば通常のタップで、キーの下に大きく外れて離すと取り消しです。
        </p>
        {@render fieldTable(POPUP_FIELDS, '長押しの項目')}
        <CodeExample caption="longPress の例" code={json(POPUP_EXAMPLE)} />
      </section>

      <section aria-labelledby="actions">
        <h2 id="actions">action 名一覧</h2>
        <p>
          <code>action</code> キー、<code>shortcut</code> の手順、長押しの項目で使う名前です。一覧にない名前も検証は通りますが、iOS では何も起きません
          （shortcut の途中なら、そこで中断します）。「ツールバーの既定アイコン」は、ツールバーのキーで <code>icon</code> と <code>label</code> が両方とも無いときに付きます。
          アイコンは SF Symbols の名前です。
        </p>
        <TableWrap label="action 名一覧">
          <table>
            <thead><tr><th>action</th><th>動作</th><th>ツールバーの既定アイコン</th><th>備考</th></tr></thead>
            <tbody>
              {#each KNOWN_ACTIONS as a (a)}
                <tr>
                  <th scope="row"><code>{a}</code></th>
                  <td>{ACTION_DOCS[a].behavior}</td>
                  <td>{#if ACTION_DOCS[a].icon}<code>{ACTION_DOCS[a].icon}</code>{:else}なし{/if}</td>
                  <td>{ACTION_DOCS[a].note ?? ''}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </TableWrap>
        <p class="note">
          <code>layout</code> キーの既定アイコンは、target ごとに決まります:
          {#each Object.entries(LAYOUT_DEFAULT_ICONS) as [t, i] (t)}<code>{t}</code> = <code>{i}</code>{/each}。
          組み込みのレイアウト名（<code>layout.target</code> に指定できる）: {BUILTIN_LAYOUT_IDS.join(' / ')}。
        </p>
      </section>

      <section aria-labelledby="look">
        <h2 id="look">見た目（look・状態・rules）</h2>
        <p>
          <code>look</code> は平らなオブジェクトです。置き場所は、レイアウト（<code>look</code>）、役割（<code>roles</code>）、行（<code>rows[].look</code>）、
          ツールバー（<code>toolbar.look</code>）、キー（<code>look</code>）で、状態ごとの指定は <code>rules</code> とキーの <code>states</code> に書きます。
        </p>

        <h3 id="look-props">look のプロパティ</h3>
        <TableWrap label="look のプロパティ">
          <table>
            <thead><tr><th>名前</th><th>型</th><th>範囲</th><th>既定（省略時）</th><th>意味</th></tr></thead>
            <tbody>
              {#each lookNames as n (n)}
                {@const s = describeLookSpec(n)}
                <tr>
                  <th scope="row"><code>{n}</code></th>
                  <td>{s.type}</td>
                  <td>{s.limit}{LOOK_UNITS[n] ? `（${LOOK_UNITS[n]}）` : ''}</td>
                  <td>{LOOK_PROP_DOCS[n].def}</td>
                  <td>{LOOK_PROP_DOCS[n].desc}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </TableWrap>
        <p class="note">
          <code>label</code> と <code>icon</code> は <code>states</code> と <code>rules</code> の中だけで有効です（他に書くと警告で、無視されます）。
          状態が <code>label</code> だけを指定したときは、その状態の間はキー本来の <code>icon</code> を出さず、状態の <code>label</code> を出します。
          <code>metrics</code> の範囲（pt）:
          {#each Object.entries(METRICS_PROPS) as [k, r], i (k)}{i > 0 ? ' / ' : ''}<code>{k}</code> {r.min}〜{r.max}{/each}。
        </p>
        <CodeExample
          caption="look の例（全体）"
          code={json({ look: { cornerRadius: 12, fontDesign: 'rounded', shadowRadius: 2, shadowY: 1, shadowOpacity: 0.2 } })}
        />

        <h3 id="look-roles">役割（role）</h3>
        <ul>
          {#each Object.entries(ROLE_DOCS) as [r, d] (r)}<li><code>{r}</code>: {d}</li>{/each}
        </ul>

        <h3 id="look-color">色の形式</h3>
        <TableWrap label="色の形式">
          <table>
            <thead><tr><th>形式</th><th>例</th><th>説明</th></tr></thead>
            <tbody>
              {#each COLOR_FORMS as c (c.form)}
                <tr><th scope="row"><code>{c.form}</code></th><td><code>{c.example}</code></td><td>{c.desc}</td></tr>
              {/each}
            </tbody>
          </table>
        </TableWrap>
        <TableWrap label="色のトークン">
          <table>
            <thead><tr><th>トークン</th><th>意味</th></tr></thead>
            <tbody>
              {#each LOOK_TOKENS as t (t)}
                <tr><th scope="row"><code>{`@${t}`}</code></th><td>{TOKEN_DOCS[t]}</td></tr>
              {/each}
            </tbody>
          </table>
        </TableWrap>
        <p class="note">
          ライト／ダークは、設定 <code>keyboard.theme</code> の結果の外観で決まります。絶対色（<code>#…</code>）は設定のアクセントの影響を受けません。
          不正な文字列（形式違い・未知のトークン・<code>light</code> / <code>dark</code> 以外のキー・空）は、そのプロパティを無視します。
        </p>

        <h3 id="look-states">状態（<code>states</code> と <code>rules</code> の <code>when</code>）</h3>
        <p>
          局所の状態はそのキー自身に、大域の状態は同じレイアウトの全キーに同時に入ります。<code>rules</code> で局所の状態を書くと、
          <code>target</code> に当てはまる各キーの、それぞれの状態の見た目になります。
        </p>
        <TableWrap label="状態の一覧">
          <table>
            <thead><tr><th>状態</th><th>種類</th><th>表示名</th><th>入る</th><th>抜ける</th><th>備考</th></tr></thead>
            <tbody>
              {#each LOOK_STATES as s (s)}
                <tr>
                  <th scope="row"><code>{s}</code></th>
                  <td>{LOOK_STATE_INFO[s].scope === 'local' ? '局所（キー単位）' : '大域（全キー）'}</td>
                  <td>{LOOK_STATE_INFO[s].label}</td>
                  <td>{STATE_DOCS[s].enter}</td>
                  <td>{STATE_DOCS[s].leave}</td>
                  <td>{STATE_DOCS[s].note ?? ''}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </TableWrap>
        <p>
          複数の状態が同時に有効なときの優先順位（低 → 高。後が勝つ）: <code>{layoutStateOrder.join(' < ')}</code>
        </p>
        <p class="note">
          <code>holdMs</code> は <code>released</code> の保持時間（{LOOK_LIMITS.holdMsMin}〜{LOOK_LIMITS.holdMsMax}ms、既定 {LOOK_LIMITS.holdMsDefault}）です。
          押下中の背景の不透明度は、組み込みで {LOOK_LIMITS.pressedFillOpacity} です。
        </p>

        <h3 id="look-rules"><code>rules</code></h3>
        <p>
          書式は <code>{`{ "when": 状態名, "target": …, "holdMs": …, "look": … }`}</code> です（<code>target</code> と <code>holdMs</code> は省略可、
          <code>holdMs</code> は <code>released</code> のときだけ有効）。1 つの rule に書ける状態は 1 つで、複数条件の組み合わせ（AND）はありません。
          <code>rules</code> は最大 {LOOK_LIMITS.rulesMax} 個で、超えるとエラーです。<code>spacer</code> は <code>target</code> に含まれません。
        </p>
        <TableWrap label="target の書式">
          <table>
            <thead><tr><th>target</th><th>対象</th></tr></thead>
            <tbody>
              {#each TARGET_DOCS as t (t.form)}
                <tr><th scope="row"><code>{t.form}</code></th><td>{t.desc}</td></tr>
              {/each}
            </tbody>
          </table>
        </TableWrap>
        <CodeExample
          caption="スペースが移動状態のとき、他のキーを薄くし、スペース自身のラベルを変える"
          code={json({
            rules: [{ when: 'spaceDrag', target: 'others', look: { opacity: 0.35, animationMs: 120 } }],
            rows: [
              {
                keys: [
                  {
                    id: 'sp',
                    width: 4.4,
                    label: '空白',
                    kind: 'action',
                    action: 'space',
                    states: { spaceDrag: { label: '移動', opacity: 0.7, fontWeight: 'semibold' } }
                  }
                ]
              }
            ]
          })}
        />

        <h3 id="look-order">重ね順（解決順序）</h3>
        <p>低い順に並べます。後のものが先のものを上書きします。</p>
        <ol>
          {#each RESOLVE_ORDER as r (r)}<li>{r}</li>{/each}
        </ol>
        <p>ツールバーのキー（行番号 <code>{TOOLBAR_ROW}</code> の行にいるものとして解決する）の順序は次のとおりです。</p>
        <ol>
          {#each TOOLBAR_RESOLVE_ORDER as r (r)}<li>{r}</li>{/each}
        </ol>
        <p class="note">
          rows の行の <code>look</code> はツールバーに効かず、<code>toolbar.look</code> は rows に効きません。<code>rules</code> には行の指定が無いので、
          ツールバーのキーも <code>target</code> に当てはまれば効きます。
        </p>
        <ul>
          {#each PARTIAL_OVERRIDE_RULES as r (r)}<li>{r}</li>{/each}
        </ul>

        <h3 id="look-image">画像の制約</h3>
        <TableWrap label="画像の制約">
          <table>
            <thead><tr><th>項目</th><th>値</th></tr></thead>
            <tbody>
              {#each IMAGE_ROWS as r (r.item)}
                <tr><th scope="row">{r.item}</th><td>{r.value}</td></tr>
              {/each}
            </tbody>
          </table>
        </TableWrap>
        <p class="note">
          画像はキーごと（<code>look.fillImage</code>。状態でも切り替えられる）と、キーボード全体（<code>background</code>）に使えます。
          キーの形（<code>cornerRadius</code>、<code>inset</code> 後）で画像と背景色をクリップします。
          <code>fillImage</code> の参照先が <code>assets</code> に無い・どこからも使われていない・読み込めない場合は、取り込みはできて警告が出ます（画像なしで描きます）。
        </p>
      </section>

      <section aria-labelledby="toolbar">
        <h2 id="toolbar">ツールバー</h2>
        <h3 id="toolbar-show">表示条件</h3>
        <ul>
          <li>入力中でなく、候補もメッセージ（辞書が無いなど）も無いときに、候補バーの場所へ出ます。高さは 44pt 固定の 1 行です。</li>
          <li>変換候補（確定直後の次語予測を含む）が来ると候補バーに入れ替わります。バーの高さは変わりません。</li>
          <li>キーは {LIMITS.toolbarKeysMin}〜{LIMITS.toolbarKeysMax} 個で、<code>width</code> の比で幅を按分します（合計 {LIMITS.rowWidthMin}〜{LIMITS.rowWidthMax}）。<code>id</code> は rows のキーと合わせて一意です。</li>
          <li>
            キーは rows のキーと同じ型ですが、使われるのはタップだけです。<code>longPress</code>、フリックの方向、スペースのスワイプは無視されます。
            <code>role</code> の既定は rows と同じ <code>normal</code> です。
          </li>
        </ul>

        <h3 id="toolbar-paste"><code>paste</code> の挙動</h3>
        <ul>
          <li>クリップボードに文字があるときだけ表示します。判定は <code>hasStrings</code> のみで、中身は読みません。</li>
          <li>設定 <code>candbar.paste_button</code> がオフなら、<code>toolbar</code> や <code>toolbar.items</code> に入れていても出しません。</li>
          <li>中身を読むのはタップしたときだけです。そのとき iOS の貼り付け許可の確認が出ることがあります。フルアクセスが無い・許可されないと何も入らず、ボタンも隠れます。</li>
          <li>入力中の文字列は先に確定してから入れます。入れた文字列は、履歴がオンのときだけ履歴にも記録されます。内容は診断ログに出しません。</li>
        </ul>

        <h3 id="toolbar-history"><code>history</code> の挙動</h3>
        <ul>
          <li>キー領域（候補の展開表示と同じ位置・高さ）を、クリップボード履歴のページに置き換えます。もう一度押すか「戻る」で閉じます。</li>
          <li>ページは 2 列のカードです（最大 4 行・種類のアイコン・相対時刻・ピン留めの印）。タップで入力中の文字列を確定してから内容を挿入し、ページを閉じます。長押しでピン留め／削除です。</li>
          <li>上部の「全削除」は 2 度押しです。1 度目で確認が出て、3 秒で戻ります。</li>
          <li>履歴の設定（<code>clipboard.history.*</code>）がオフのときは、ページ内に案内を出します。設定は「iOS 設定キー」の節にあります。</li>
          <li>パスワード欄では取り込みも表示もしません。見た目はシステムの配色で、レイアウトの <code>look</code> には接続しません。</li>
        </ul>

        <h3 id="toolbar-settings">設定 <code>toolbar.items</code> との関係</h3>
        <ul>
          <li><strong>レイアウトに <code>toolbar</code> があれば、それを使い、設定 <code>toolbar.items</code> は使いません。</strong></li>
          <li><code>toolbar</code> が無いレイアウトでだけ、<code>toolbar.items</code>（既定 <code>[dismiss, paste, cursorLeft, cursorRight]</code>）の並びから同じ内容のキー列を作って出します。キー id は <code>tb-&lt;項目名&gt;</code> です。</li>
          <li>知らない名前と重複は捨てます。空配列は「何も出さない」です。</li>
        </ul>
        <TableWrap label="toolbar.items の項目">
          <table>
            <thead><tr><th>項目名</th><th>対応するキー</th></tr></thead>
            <tbody>
              {#each TOOLBAR_ITEMS as t (t.item)}
                <tr><th scope="row"><code>{t.item}</code></th><td>{t.key}</td></tr>
              {/each}
            </tbody>
          </table>
        </TableWrap>
        <p class="note">
          取り消し（undo）・やり直し・全選択・ホストアプリを開く、は、キーボード拡張から安全に行える API が無いため未対応です。
        </p>
        <CodeExample caption="toolbar の例" code={json(TOOLBAR_EXAMPLE)} />
      </section>

      <section aria-labelledby="settings">
        <h2 id="settings">iOS 設定キー</h2>
        <p>
          iOS 固有の設定は <code>config.toml</code> には書かず、本体アプリの設定画面から変えます（共有領域の設定ファイルに保存します）。
          ここではレイアウトに関係するものを挙げます。全項目は <code>docs/ios-settings.md</code> にあります。
        </p>
        <h3 id="settings-layout">レイアウトに関係する設定</h3>
        {@render settingTable(LAYOUT_SETTINGS, 'レイアウトに関係する設定')}

        <h3 id="settings-clipboard">クリップボード履歴（<code>clipboard.history.*</code>）</h3>
        <p>ツールバーの <code>history</code> から開く履歴ページの設定です。既定の <code>toolbar.items</code> には入っていません。</p>
        {@render settingTable(CLIPBOARD_SETTINGS, 'クリップボード履歴の設定')}
        <h4>取り込みのタイミングの制約</h4>
        <ul>
          {#each CLIPBOARD_CAPTURE_TIMING as t (t)}<li>{t}</li>{/each}
        </ul>
        <h4>プライバシー</h4>
        <ul>
          {#each CLIPBOARD_PRIVACY as t (t)}<li>{t}</li>{/each}
        </ul>
        <p class="note">履歴の機能は開発中で、実機での確認が済んでいない部分があります。</p>
      </section>

      <section aria-labelledby="errors">
        <h2 id="errors">検証エラーと対処</h2>
        <p>
          メッセージはエディタの検証表示と同じです。N などは実際の数値に置き換わります。エディタでは、場所（例: <code>rows[0].keys[2].text</code> は「1 行目の 3 番目のキー」）を示し、
          「その場所を開く」で該当の欄へ移れます。<strong>エラー</strong>がある間は書き出せません。<strong>警告</strong>は書き出せます。
        </p>
        {#each errorGroups as g (g)}
          <h3>{g}</h3>
          <TableWrap label={`${g}の検証メッセージ`}>
            <table>
              <thead><tr><th>種別</th><th>メッセージ</th><th>原因</th><th>対処</th></tr></thead>
              <tbody>
                {#each ERROR_DOCS.filter((e) => e.group === g) as e (e.message)}
                  <tr>
                    <td><span class={['badge', e.severity]}>{e.severity === 'error' ? 'エラー' : '警告'}</span></td>
                    <td>{e.message}</td>
                    <td>{e.cause}</td>
                    <td>{e.fix}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </TableWrap>
        {/each}
      </section>

      <section aria-labelledby="limits">
        <h2 id="limits">利用者に関係する制約</h2>
        <h3>取り込み・反映</h3>
        <ul>
          <li>取り込んだレイアウトは、設定の「かな」「英字」などの割り当てで選びます。反映は、キーボードを開き直したときです。</li>
          <li>不正なレイアウトは読み込まれず、同梱の既定のレイアウトに戻ります（入力を失わないための動作です）。</li>
          <li>同じ <code>id</code> のユーザーレイアウトを取り込むと更新されます。同梱のレイアウトと同じ <code>id</code> でも、ユーザーのものが優先されます。</li>
          <li>割り当て中のレイアウトを削除すると、既定に戻ります。</li>
          <li>
            URL スキーム（<code>nagi://layout?data=…</code>）の上限は 1MB です。<code>assets</code> を含むレイアウトは URL では渡せず、ファイルかコピーで渡します。
            JSON 全体の上限は {sizeKB(LAYOUT_FILE_MAX_BYTES)} です。
          </li>
          <li>
            <code>iconify:</code> のアイコンは、取り込み時にホストアプリが取得して保存します（通信が入るため、取り込みの完了まで少し待ちます）。
            取得できず <code>label</code> で代替したキーは完了メッセージに出ます。取得できず <code>label</code> も無いキーがあるレイアウトは保存されません。
            <code>states</code> / <code>rules</code> の <code>icon</code> も取り込み時に解決します。
          </li>
        </ul>
        <h3>表示</h3>
        <ul>
          <li><code>scale</code> / <code>offsetX</code> / <code>offsetY</code> / <code>inset</code> は見た目だけを変え、当たり判定は変わりません。キーの位置を自由に置く指定（絶対座標）はありません。</li>
          <li><code>metrics</code> の隙間を大きくしても行の高さは自動では縮まりません。キーボードの高さを超えることがあるので、必要なら行の <code>height</code> の比を下げてください。</li>
          <li><code>background</code> は候補バーの背面にも敷きますが、候補バー自身が不透明なので、候補バーの部分は見えません。</li>
          <li>片手モードでは、<code>metrics.padding</code> がキー領域の外周にだけ掛かります。</li>
          <li>本体アプリの「レイアウト」タブの簡易プレビューは、<code>look</code> と画像を描きません。編集ページのプレビューは CSS による近似で、実機とは細部（フォントの太さ・影の見え方）が異なります。</li>
          <li>メモリ上限のあるキーボード拡張で動くため、画像はメモリに余裕があるときだけ使われます（画像の制約の節）。</li>
        </ul>
        <h3>入力</h3>
        <ul>
          <li><code>flick</code> キーと ⌫ の <code>longPress</code> は無視されます。</li>
          <li>状態 <code>spaceDrag</code> は、設定 <code>touch.space_swipe</code> がオンで、スペースに <code>longPress</code> が無いときだけ入ります。</li>
          <li>「視差効果を減らす」が有効な端末では、<code>animationMs</code> は 0 として扱われ、<code>scale</code> の補間もしません。</li>
          <li>外付けキーボードでの入力は、キーボード拡張の仕組み上、対象外です。</li>
        </ul>
        <h3>開発状況</h3>
        <ul>
          <li>iOS 版は TestFlight で検証中の開発版です。見た目の指定（状態・画像）、ツールバー、クリップボード履歴は、実機での確認が済んでいない部分があり、仕様や画面は変わることがあります。</li>
        </ul>
      </section>
    </div>
  </div>
</main>

<style>
  .layout {
    display: grid;
    gap: 8px 48px;
  }
  @media (min-width: 1100px) {
    .layout {
      grid-template-columns: 220px minmax(0, 1fr);
    }
    .toc {
      position: sticky;
      top: 84px;
      align-self: start;
    }
  }
  .toc {
    padding-top: 40px;
    font-size: 0.875rem;
  }
  .toc-title {
    margin: 0 0 8px;
    color: var(--muted);
    font-size: 0.75rem;
    letter-spacing: 0.08em;
  }
  .toc ul {
    list-style: none;
    margin: 0;
    padding: 0;
    counter-reset: toc;
    border-top: 1px solid var(--line);
  }
  .toc li {
    counter-increment: toc;
    border-bottom: 1px solid var(--line);
  }
  .toc a {
    display: grid;
    grid-template-columns: 1.8em 1fr;
    padding: 9px 0;
    color: var(--fg);
    text-decoration: none;
    line-height: 1.5;
  }
  .toc a::before {
    content: counter(toc, decimal-leading-zero);
    font-family: var(--font-mono);
    font-size: 0.6875rem;
    color: var(--muted);
    padding-top: 0.2em;
  }
  .toc a:hover {
    color: var(--accent);
  }

  .body {
    min-width: 0;
    padding-bottom: 24px;
  }
  .body section:first-child h2 {
    margin-top: 40px;
  }
  .body h2 {
    font-family: var(--font-serif);
    font-weight: 600;
    font-size: 1.375rem;
    line-height: 1.5;
    letter-spacing: 0.02em;
    margin: 56px 0 14px;
    padding-top: 8px;
    border-top: 1px solid var(--line);
    scroll-margin-top: 88px;
  }
  .body h3 {
    font-size: 1.0625rem;
    margin: 32px 0 6px;
    scroll-margin-top: 88px;
  }
  .body h4 {
    font-size: 0.9375rem;
    margin: 20px 0 4px;
  }
  .body :global(ul),
  .body :global(ol) {
    padding-left: 1.4em;
    max-width: 52em;
  }
  .body li {
    margin: 6px 0;
  }
  .body p {
    max-width: 52em;
  }
  .note {
    color: var(--muted);
    font-size: 0.9375rem;
  }

  table {
    border-collapse: collapse;
    font-size: 0.875rem;
    line-height: 1.6;
  }
  th,
  td {
    text-align: left;
    vertical-align: top;
    padding: 8px 12px;
    border-bottom: 1px solid var(--line);
  }
  table.kv {
    margin: 12px 0;
    max-width: 40em;
  }
  table.kv th {
    width: 11em;
    font-weight: 600;
    white-space: nowrap;
  }
  .badge {
    display: inline-block;
    padding: 1px 8px;
    border-radius: 999px;
    font-size: 0.75rem;
    white-space: nowrap;
  }
  .badge.error {
    color: var(--danger);
    background: var(--danger-soft);
  }
  .badge.warning {
    color: var(--fg);
    background: var(--sun-soft);
  }
</style>
