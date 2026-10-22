<script lang="ts">
  import Meta from '$lib/components/Meta.svelte'
  import ConversionDemo from '$lib/components/site/ConversionDemo.svelte'
  import Horizon from '$lib/components/site/Horizon.svelte'

  const principles = [
    {
      n: '一',
      title: 'ローカル LLM による変換',
      body: '小型のニューラルモデルで文脈を踏まえた変換候補を作ります。辞書とビタビ探索による変換も併用し、モデルが無い・使えない場合は辞書だけで動作します。'
    },
    {
      n: '二',
      title: '全処理が端末内',
      body: '変換・学習・辞書の処理はすべて手元で行います。通信するのはモデル・辞書のダウンロードと、アップデートの確認（無効にできます）だけです。'
    },
    {
      n: '三',
      title: '入力を失わない設計',
      body: '変換エンジンが落ちても入力中の文字を復元できる作りにしています。パスワード欄などでは学習を止める方針です。'
    },
    {
      n: '四',
      title: 'オープンソース',
      body: 'コードは MIT OR Apache-2.0 で公開します。辞書やモデルなどのデータは同梱せず、別途取得する構成です。'
    }
  ]

  // stage: 0 将来 / 1 予定 / 2 開発中 / 3 実機で検証中 / 4 配布中
  const stages = ['予定', '開発', '検証', '配布']
  const platforms = [
    {
      name: 'iOS',
      stage: 3,
      label: 'TestFlight で検証中',
      body: 'キーボード拡張とホストアプリが TestFlight で動作中。かな 12 キー・フリック・変換・候補バー・英字入力を実機で検証しています。'
    },
    { name: 'macOS', stage: 2, label: '開発中', body: '先行して開発中です。変換エンジンの中核は実装済みです。' },
    { name: 'Windows', stage: 1, label: '予定', body: 'macOS の後に対応する予定です。' },
    { name: 'Android / Linux', stage: 0, label: '将来の検討', body: '将来の検討対象です。' }
  ]

  // 12 キーの配列（カスタムキーボードの紹介用。実際の既定配列とは細部が異なる）
  const kana = ['あ', 'か', 'さ', 'た', 'な', 'は', 'ま', 'や', 'ら', '小ﾞﾟ', 'わ', '、。?!']
</script>

<Meta
  title="Nagi"
  path="/"
  description="Nagi は、端末内で動く小型の言語モデルでかな漢字変換を行う日本語入力システムです。入力内容を外部に送信しません。macOS 先行、iOS は TestFlight で検証中。"
/>

<main>
  <section class="hero wrap" aria-labelledby="hero-title">
    <div class="hero-text">
      <p class="eyebrow">開発中の日本語入力システム</p>
      <h1 id="hero-title"><span>端末の中だけで</span><span>考える、</span><br /><span>ニューラル</span><span>かな漢字変換。</span></h1>
      <p class="lead">
        Nagi（凪）は、GPT-2 級の小さなローカル言語モデルでかな漢字変換を行う日本語 IME です。
        変換も学習も端末の中で完結し、入力した文章をサーバーへ送りません。
      </p>
      <p class="actions">
        <a class="btn primary" href="#testflight">iOS 版を試す（TestFlight）</a>
        <a class="btn" href="/custom-keyboard">キーボードの配列を編集する</a>
      </p>
      <p class="note">現在は開発途中です。機能や対応状況は<a href="#status">いまの状況</a>を確認してください。</p>
    </div>
    <div class="hero-demo">
      <ConversionDemo />
    </div>
  </section>

  <Horizon sun={0.82} class="hero-horizon" />

  <section class="wrap block" aria-labelledby="features">
    <header class="block-head">
      <p class="eyebrow">特徴</p>
      <h2 id="features">静かに、手元で変換する</h2>
    </header>
    <ol class="principles">
      {#each principles as p (p.n)}
        <li>
          <span class="num" aria-hidden="true">{p.n}</span>
          <h3>{p.title}</h3>
          <p>{p.body}</p>
        </li>
      {/each}
    </ol>
  </section>

  <section class="wrap block" aria-labelledby="status">
    <header class="block-head">
      <p class="eyebrow">対応状況</p>
      <h2 id="status">いまの状況</h2>
    </header>
    <ul class="status">
      {#each platforms as pf (pf.name)}
        <li>
          <h3>{pf.name}</h3>
          <div class="gauge" aria-hidden="true">
            {#each stages as st, i (st)}
              <span class:on={i < pf.stage} class:now={i === pf.stage - 1}>{st}</span>
            {/each}
          </div>
          <p><strong>{pf.label}</strong><span class="sep" aria-hidden="true"> ― </span>{pf.body}</p>
        </li>
      {/each}
    </ul>
    <p class="note">
      変換に使うモデルと辞書は比較検証中で、現在は暫定の構成です。精度や速度は今後変わる可能性があります。
    </p>
  </section>

  <section class="sea" aria-labelledby="privacy-h">
    <div class="wrap">
      <p class="eyebrow">プライバシー</p>
      <h2 id="privacy-h"><span>入力した文章は、</span><span>端末の外へ</span><span>出ません。</span></h2>
      <ul class="sea-list">
        <li>入力した文章は端末の外に送信しません。変換は端末内のモデルと辞書だけで行います。</li>
        <li>
          ログは端末内にのみ保存します。App Store 配布版では入力内容を残しません。テスト配布（TestFlight）の期間中のみ、診断ログに入力内容を含めることがあります（パスワード欄を除く。設定で無効化・消去できます）。
        </li>
        <li>学習データは端末内に保存し、ワンコマンドで全削除できるようにする方針です。</li>
      </ul>
      <p class="sea-more">
        詳しくは<a href="/privacy">プライバシーポリシー</a>をご覧ください。使い方で困ったときは<a href="/support">サポート</a>をどうぞ。
      </p>

      <div class="fullaccess">
        <div>
          <h3 id="fullaccess">iOS の「フルアクセス」について</h3>
          <p>
            iOS のキーボードは、設定で「フルアクセスを許可」にしないと、本体アプリが取得した辞書・モデルのファイルを読めません。
            Nagi が許可をお願いする理由はこれだけで、<strong>辞書・モデルの読み込みのためです</strong>。
          </p>
          <ul>
            <li>キーボードから通信は行いません。入力内容を外部へ送信しません。</li>
            <li>
              許可しない場合の動作は端末により異なる場合があり、辞書・モデルを利用できないことがあります（入力した文字は失われません）。
            </li>
            <li>辞書・モデルの取得や更新の確認は、キーボードではなく本体アプリが行います。</li>
          </ul>
        </div>
        <div class="settings" aria-hidden="true">
          <p class="path">設定 › 一般 › キーボード › Nagi</p>
          <div class="row"><span>Nagi</span><span class="toggle on"></span></div>
          <div class="row"><span>フルアクセスを許可</span><span class="toggle on"></span></div>
          <p class="why">用途: 共有領域の辞書・モデルを読む</p>
        </div>
      </div>
    </div>
  </section>

  <section class="wrap block split" aria-labelledby="testflight">
    <picture class="big-icon">
      <source srcset="/icon-dark.svg" media="(prefers-color-scheme: dark)" />
      <img src="/icon.svg" alt="Nagi のアプリアイコン" width="120" height="120" />
    </picture>
    <div>
      <p class="eyebrow">iOS 版</p>
      <h2 id="testflight">iOS 版を試す</h2>
      <p><strong>TestFlight の案内は準備中です。</strong></p>
      <p>
        <!-- TODO: 外部テストの公開リンク（TestFlight Public Link）が決まったらここに設置する。 -->
        公開リンクが用意でき次第、ここに掲載します。
      </p>
    </div>
  </section>

  <section class="wrap block split keyboard-block" aria-labelledby="ck">
    <div>
      <p class="eyebrow">カスタムキーボード</p>
      <h2 id="ck">キーボードの配列を<br />自分で作る</h2>
      <p>
        iOS 版のキーボードは、配列とキーの動作を JSON で定義する仕組みを進めています。
        <a href="/custom-keyboard">編集ページ</a>では、ブラウザだけでキーの配置・種類（文字入力・フリック・ショートカットなど）・幅を設定して
        JSON を書き出せます。データはブラウザ内にのみ保存され、サーバーには送信されません。
      </p>
      <p class="note">
        アプリ側での取り込み機能は開発中です。今の編集ページは、仕様に沿ったレイアウトを作って書き出すためのものです。
      </p>
      <p><a class="btn" href="/custom-keyboard">編集ページを開く</a></p>
    </div>
    <div class="kb" aria-hidden="true">
      <span class="k fn">☆123</span>
      {#each kana.slice(0, 3) as k (k)}<span class="k">{k}</span>{/each}
      <span class="k fn">⌫</span>
      <span class="k fn">ABC</span>
      {#each kana.slice(3, 6) as k (k)}
        {#if k === 'な'}
          <span class="k flick">
            <b>な</b><i class="u">ぬ</i><i class="l">に</i><i class="r">ね</i><i class="d">の</i>
          </span>
        {:else}
          <span class="k">{k}</span>
        {/if}
      {/each}
      <span class="k fn">空白</span>
      <span class="k fn">あいう</span>
      {#each kana.slice(6, 9) as k (k)}<span class="k">{k}</span>{/each}
      <span class="k accent tall">改行</span>
      <span class="k fn"
        ><svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.1"
          ><circle cx="8" cy="8" r="6.5" /><ellipse cx="8" cy="8" rx="3" ry="6.5" /><path d="M1.5 8h13M2.6 4.6h10.8M2.6 11.4h10.8" /></svg
        ></span
      >
      {#each kana.slice(9, 12) as k (k)}<span class="k small">{k}</span>{/each}
    </div>
  </section>
</main>

<style>
  .wrap {
    max-width: 1080px;
    margin: 0 auto;
    padding: 0 20px;
    box-sizing: border-box;
  }
  h1,
  h2 {
    font-family: var(--font-serif);
    font-weight: 600;
    letter-spacing: 0.02em;
  }
  .note {
    color: var(--muted);
    font-size: 0.875rem;
  }

  /* ---- 冒頭 ---- */
  .hero {
    display: grid;
    gap: 48px;
    padding-top: 64px;
    align-items: center;
  }
  @media (min-width: 900px) {
    .hero {
      grid-template-columns: minmax(0, 1.15fr) minmax(0, 0.85fr);
      gap: 64px;
      padding-top: 88px;
    }
  }
  h1 {
    font-size: clamp(2rem, 5.4vw, 3.25rem);
    line-height: 1.4;
    margin: 18px 0 22px;
  }
  h1 span {
    display: inline-block;
  }
  .lead {
    font-size: 1.0625rem;
    max-width: 34em;
    margin: 0;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
    margin: 32px 0 14px;
  }
  :global(.hero-horizon) {
    margin-top: 40px;
  }

  /* ---- 各節の共通 ---- */
  .block {
    padding-top: 88px;
  }
  .block-head {
    margin-bottom: 32px;
  }
  h2 {
    font-size: clamp(1.625rem, 4vw, 2.25rem);
    line-height: 1.45;
    margin: 12px 0 0;
  }
  .split h2 {
    margin-bottom: 20px;
  }

  /* ---- 特徴: 漢数字の番号を振った罫線の段組 ---- */
  .principles {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    border-top: 1px solid var(--line-strong);
  }
  @media (min-width: 760px) {
    .principles {
      grid-template-columns: 1fr 1fr;
    }
    .principles li:nth-child(odd) {
      border-right: 1px solid var(--line);
      padding-right: 32px;
    }
    .principles li:nth-child(even) {
      padding-left: 32px;
    }
  }
  .principles li {
    display: grid;
    grid-template-columns: 2.4em minmax(0, 1fr);
    column-gap: 12px;
    padding: 28px 0 32px;
    border-bottom: 1px solid var(--line);
  }
  .num {
    grid-row: span 2;
    font-family: var(--font-serif);
    font-size: 2rem;
    line-height: 1;
    color: var(--accent);
    padding-top: 2px;
  }
  .principles h3 {
    font-size: 1.0625rem;
    margin: 0 0 8px;
  }
  .principles p {
    margin: 0;
    font-size: 0.9375rem;
    color: var(--muted);
  }

  /* ---- 対応状況: 段階のゲージ ---- */
  .status {
    list-style: none;
    margin: 0 0 16px;
    padding: 0;
    border-top: 1px solid var(--line-strong);
  }
  .status li {
    display: grid;
    gap: 10px 32px;
    padding: 22px 0;
    border-bottom: 1px solid var(--line);
    align-items: baseline;
  }
  @media (min-width: 760px) {
    .status li {
      grid-template-columns: 11em 15em minmax(0, 1fr);
    }
  }
  .status h3 {
    font-family: var(--font-serif);
    font-weight: 600;
    font-size: 1.25rem;
    margin: 0;
  }
  .status p {
    margin: 0;
    font-size: 0.9375rem;
  }
  .status .sep {
    color: var(--muted);
  }
  .gauge {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 3px;
    align-self: center;
  }
  .gauge span {
    font-size: 0.6875rem;
    color: var(--muted);
    padding-top: 6px;
    border-top: 3px solid var(--line);
    letter-spacing: 0.06em;
  }
  .gauge span.on {
    border-color: var(--accent);
    color: var(--fg);
  }
  .gauge span.now {
    font-weight: 600;
  }

  /* ---- プライバシー: 夜の海の帯 ---- */
  .sea {
    margin-top: 112px;
    padding: 88px 0 96px;
    background: var(--sea);
    color: var(--sea-fg);
    --focus: #f3d98f;
  }
  .sea a {
    color: var(--sea-fg);
  }
  .sea .eyebrow {
    color: var(--sea-muted);
  }
  .sea h2 span {
    display: inline-block;
  }
  .sea h2 {
    font-size: clamp(1.875rem, 5vw, 2.875rem);
    line-height: 1.45;
  }
  .sea-list {
    list-style: none;
    padding: 0;
    margin: 40px 0 0;
    display: grid;
    gap: 0 40px;
    counter-reset: item;
  }
  @media (min-width: 760px) {
    .sea-list {
      grid-template-columns: repeat(3, minmax(0, 1fr));
    }
  }
  .sea-list li {
    padding: 18px 0 20px;
    border-top: 1px solid var(--sea-line);
    font-size: 0.9375rem;
    color: var(--sea-muted);
  }
  .sea-list li::before {
    counter-increment: item;
    content: counter(item, cjk-ideographic);
    display: block;
    font-family: var(--font-serif);
    color: var(--sun);
    margin-bottom: 6px;
  }
  .sea-more {
    font-size: 0.9375rem;
    margin: 24px 0 0;
  }
  .fullaccess {
    display: grid;
    gap: 32px 56px;
    margin-top: 72px;
    padding-top: 40px;
    border-top: 1px solid var(--sea-line);
    align-items: start;
  }
  @media (min-width: 860px) {
    .fullaccess {
      grid-template-columns: minmax(0, 1fr) 320px;
    }
  }
  .fullaccess h3 {
    font-family: var(--font-serif);
    font-weight: 600;
    font-size: 1.375rem;
    margin: 0 0 12px;
  }
  .fullaccess p,
  .fullaccess li {
    font-size: 0.9375rem;
    color: var(--sea-muted);
  }
  .fullaccess strong {
    color: var(--sea-fg);
    text-decoration: underline 2px var(--sun);
    text-underline-offset: 0.3em;
  }
  .fullaccess ul {
    padding-left: 1.2em;
  }
  .settings {
    border: 1px solid var(--sea-line);
    border-radius: 6px;
    padding: 14px 16px;
    font-size: 0.875rem;
  }
  .settings .path {
    margin: 0 0 10px;
    font-size: 0.75rem;
    color: var(--sea-muted);
  }
  .settings .row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 10px 0;
    border-top: 1px solid var(--sea-line);
  }
  .toggle {
    width: 38px;
    height: 22px;
    border-radius: 11px;
    background: var(--sea-line);
    position: relative;
  }
  .toggle.on {
    background: var(--sun);
  }
  .toggle::after {
    content: '';
    position: absolute;
    top: 3px;
    left: 3px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: var(--sea);
  }
  .toggle.on::after {
    left: 19px;
  }
  .settings .why {
    margin: 8px 0 0;
    font-size: 0.75rem;
    color: var(--sea-muted);
  }

  /* ---- 2 段組の節 ---- */
  .split {
    display: grid;
    gap: 32px 64px;
    align-items: center;
  }
  @media (min-width: 860px) {
    .split {
      grid-template-columns: auto minmax(0, 1fr);
    }
    .keyboard-block {
      grid-template-columns: minmax(0, 1fr) 360px;
    }
  }
  .split p {
    max-width: 38em;
  }
  .big-icon img {
    display: block;
    width: 120px;
    height: 120px;
    border-radius: 27px;
    box-shadow: 6px 6px 0 var(--sun);
  }

  /* ---- 12 キーの見本 ---- */
  .kb {
    display: grid;
    grid-template-columns: 1fr 1.2fr 1.2fr 1.2fr 1fr;
    grid-auto-rows: 46px;
    gap: 6px;
    padding: 8px;
    background: var(--kb-bg);
    border-radius: 8px;
  }
  .k {
    position: relative;
    display: grid;
    place-items: center;
    background: var(--key-bg);
    border-radius: 6px;
    box-shadow: 0 1px 0 rgba(0, 0, 0, 0.18);
    font-size: 1.0625rem;
  }
  .k.fn {
    background: var(--key-fn-bg);
    font-size: 0.75rem;
  }
  .k.small {
    font-size: 0.8125rem;
  }
  .k.accent {
    background: var(--key-accent-bg);
    color: var(--key-accent-fg);
    font-size: 0.75rem;
  }
  .k.tall {
    grid-row: span 2;
  }
  .flick {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
    z-index: 1;
  }
  .flick i {
    position: absolute;
    font-style: normal;
    font-size: 0.6875rem;
    color: var(--muted);
  }
  .flick .u {
    top: 1px;
  }
  .flick .d {
    bottom: 1px;
  }
  .flick .l {
    left: 6px;
  }
  .flick .r {
    right: 6px;
  }
</style>
