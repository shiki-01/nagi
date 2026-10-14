<svelte:head>
  <title>Nagi - ローカル LLM による日本語入力</title>
  <meta
    name="description"
    content="Nagi は、端末内で動く小型の言語モデルでかな漢字変換を行う日本語入力システムです。入力内容を外部に送信しません。macOS 先行、iOS は TestFlight で検証中。"
  />
</svelte:head>

<main class="page">
  <section class="hero" aria-labelledby="hero-title">
    <p class="eyebrow">開発中の日本語入力システム</p>
    <h1 id="hero-title">端末の中だけで考える、<br />ニューラルかな漢字変換。</h1>
    <p class="lead">
      Nagi（凪）は、GPT-2 級の小さなローカル言語モデルでかな漢字変換を行う日本語 IME です。
      変換も学習も端末の中で完結し、入力した文章をサーバーへ送りません。
    </p>
    <p class="actions flex gap:12px">
      <a class="btn primary" href="#testflight">iOS 版を試す（TestFlight）</a>
      <a class="btn" href="/custom-keyboard">キーボードの配列を編集する</a>
    </p>
    <p class="note">
      現在は開発途中です。機能や対応状況は<a href="#status">対応状況</a>を確認してください。
    </p>
  </section>

  <section aria-labelledby="features">
    <h2 id="features">特徴</h2>
    <ul class="cards">
      <li>
        <h3>ローカル LLM による変換</h3>
        <p>
          小型のニューラルモデルで文脈を踏まえた変換候補を作ります。辞書とビタビ探索による変換も併用し、
          モデルが無い・使えない場合は辞書だけで動作します。
        </p>
      </li>
      <li>
        <h3>全処理が端末内</h3>
        <p>
          変換・学習・辞書の処理はすべて手元で行います。通信するのはモデル・辞書のダウンロードと、
          アップデートの確認（無効にできます）だけです。
        </p>
      </li>
      <li>
        <h3>入力を失わない設計</h3>
        <p>
          変換エンジンが落ちても入力中の文字を復元できる作りにしています。パスワード欄などでは学習を止める方針です。
        </p>
      </li>
      <li>
        <h3>オープンソース</h3>
        <p>
          コードは MIT OR Apache-2.0 で公開します。辞書やモデルなどのデータは同梱せず、別途取得する構成です。
        </p>
      </li>
    </ul>
  </section>

  <section aria-labelledby="status">
    <h2 id="status">対応状況</h2>
    <div class="table-wrap">
      <table>
        <caption class="sr-only">プラットフォームごとの対応状況</caption>
        <thead>
          <tr><th scope="col">プラットフォーム</th><th scope="col">状況</th></tr>
        </thead>
        <tbody>
          <tr><th scope="row">macOS</th><td>先行して開発中（変換エンジンの中核は実装済み）</td></tr>
          <tr>
            <th scope="row">iOS</th>
            <td>
              キーボード拡張とホストアプリが TestFlight で動作中。かな 12 キー・フリック・変換・候補バー・英字入力を実機で検証しています
            </td>
          </tr>
          <tr><th scope="row">Windows</th><td>後続（macOS の後に対応予定）</td></tr>
          <tr><th scope="row">Android / Linux</th><td>将来の検討対象</td></tr>
        </tbody>
      </table>
    </div>
    <p class="note">変換に使うモデルと辞書は比較検証中で、現在は暫定の構成です。精度や速度は今後変わる可能性があります。</p>
  </section>

  <section aria-labelledby="privacy-h">
    <h2 id="privacy-h">プライバシー</h2>
    <ul class="plain">
      <li>入力した文章は端末の外に送信しません。変換は端末内のモデルと辞書だけで行います。</li>
      <li>ログに入力内容を残しません（開発用ビルドで明示的に有効にした場合を除く）。</li>
      <li>学習データは端末内に保存し、ワンコマンドで全削除できるようにする方針です。</li>
    </ul>
    <p>詳しくは<a href="/privacy">プライバシーポリシー</a>をご覧ください（現在は草案です）。</p>
  </section>

  <section aria-labelledby="fullaccess">
    <h2 id="fullaccess">iOS の「フルアクセス」について</h2>
    <p>
      iOS のキーボードは、設定で「フルアクセスを許可」にしないと、本体アプリが取得した辞書・モデルのファイルを読めません。
      Nagi が許可をお願いする理由はこれだけで、<strong>辞書・モデルの読み込みのためです</strong>。
    </p>
    <ul class="plain">
      <li>キーボードから通信は行いません。入力内容を外部へ送信しません。</li>
      <li>許可しない場合は、ひらがな・カタカナへの変換のみで動作します（入力した文字は失われません）。</li>
      <li>辞書・モデルの取得や更新の確認は、キーボードではなく本体アプリが行います。</li>
    </ul>
  </section>

  <section aria-labelledby="testflight">
    <h2 id="testflight">iOS 版を試す</h2>
    <div class="callout">
      <p><strong>TestFlight の案内は準備中です。</strong></p>
      <p>
        <!-- TODO: 外部テストの公開リンク（TestFlight Public Link）が決まったらここに設置する。 -->
        公開リンクが用意でき次第、ここに掲載します。
      </p>
    </div>
  </section>

  <section aria-labelledby="ck">
    <h2 id="ck">キーボードの配列を自分で作る</h2>
    <p>
      iOS 版のキーボードは、配列とキーの動作を JSON で定義する仕組みを進めています。
      <a href="/custom-keyboard">編集ページ</a>では、ブラウザだけでキーの配置・種類（文字入力・フリック・ショートカットなど）・幅を設定して
      JSON を書き出せます。データはブラウザ内にのみ保存され、サーバーには送信されません。
    </p>
    <p class="note">
      アプリ側での取り込み機能は開発中です。今の編集ページは、仕様に沿ったレイアウトを作って書き出すためのものです。
    </p>
  </section>
</main>

<style>
  .page {
    max-width: 880px;
    margin: 0 auto;
    padding: 24px 16px 0;
    box-sizing: border-box;
  }
  .hero {
    padding: 32px 0 24px;
  }
  .eyebrow {
    color: var(--accent);
    font-weight: 600;
    margin: 0;
    font-size: 0.9375rem;
  }
  h1 {
    font-size: clamp(1.75rem, 6.5vw, 2.75rem);
    line-height: 1.3;
    margin: 8px 0 16px;
  }
  h2 {
    font-size: 1.5rem;
    margin: 48px 0 12px;
    line-height: 1.4;
  }
  h3 {
    font-size: 1.0625rem;
    margin: 0 0 6px;
  }
  .lead {
    font-size: 1.0625rem;
    max-width: 38em;
  }
  .actions {
    flex-wrap: wrap;
    margin: 20px 0 12px;
  }
  .note {
    color: var(--muted);
    font-size: 0.9375rem;
  }
  .cards {
    list-style: none;
    padding: 0;
    margin: 0;
    display: grid;
    gap: 12px;
    grid-template-columns: repeat(auto-fit, minmax(min(100%, 260px), 1fr));
  }
  .cards li {
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: 14px;
    padding: 16px;
  }
  .cards p {
    margin: 0;
    font-size: 0.9375rem;
  }
  .plain {
    padding-left: 1.3em;
  }
  .table-wrap {
    overflow-x: auto;
  }
  table {
    border-collapse: collapse;
    width: 100%;
    min-width: 480px;
  }
  th,
  td {
    border: 1px solid var(--line);
    padding: 10px 12px;
    text-align: left;
    vertical-align: top;
  }
  thead th {
    background: var(--surface-2);
  }
  tbody th {
    white-space: nowrap;
    background: var(--surface);
  }
  .callout {
    border: 1px dashed var(--muted);
    border-radius: 14px;
    padding: 4px 16px;
    background: var(--surface);
  }
</style>
