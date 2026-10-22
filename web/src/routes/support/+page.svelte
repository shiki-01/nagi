<script lang="ts">
  import Meta from '$lib/components/Meta.svelte'
  import { ISSUES_URL, REPO_URL } from '$lib/site'
</script>

<Meta
  title="サポート"
  path="/support"
  description="Nagi のサポートページ。キーボードの追加方法、フルアクセスの設定、辞書・モデルの取り込み、変換が遅い・出ないときの対処、学習データの削除、レイアウトの取り込みについて。"
/>

<main class="doc">
  <header class="doc-head">
    <p class="eyebrow">サポート</p>
    <h1>使い方と、困ったとき</h1>
    <p class="lead">
      Nagi（iOS 版）の使い方とよくある質問です。現在は TestFlight で検証中の開発版で、画面や手順が変わることがあります。
    </p>
  </header>

  <div class="layout">
    <nav class="toc" aria-label="このページの目次">
      <p class="toc-title">目次</p>
      <ul>
        <li><a href="#add">キーボードを追加するには</a></li>
        <li><a href="#fullaccess">フルアクセスを許可するには</a></li>
        <li><a href="#data">辞書・モデルを取り込むには</a></li>
        <li><a href="#slow">変換が遅い・候補が出ないとき</a></li>
        <li><a href="#delete">学習データを削除するには</a></li>
        <li><a href="#layout">キーボードのレイアウトを取り込むには</a></li>
        <li><a href="#contact">お問い合わせ</a></li>
      </ul>
    </nav>

    <div class="prose">

      <section aria-labelledby="add">
        <h2 id="add">キーボードを追加するには</h2>
        <ol>
          <li>iPhone の「設定」アプリで、一般 &gt; キーボード &gt; キーボード &gt; 新しいキーボードを追加 を開きます。</li>
          <li>一覧から「Nagi」を選びます。</li>
          <li>文字を入力できる画面で 🌐 キーを押し、「Nagi」に切り替えます。</li>
        </ol>
        <p>
          Nagi アプリのホームにも、同じ手順の案内（初回起動時に表示。ホームの「セットアップの案内を表示」からいつでも開けます）と、
          追加できているかの確認表示があります。追加後に一度キーボードを表示してからアプリに戻ると、確認が「OK」になります。
        </p>
      </section>

      <section aria-labelledby="fullaccess">
        <h2 id="fullaccess">フルアクセスを許可するには（なぜ必要ですか）</h2>
        <ol>
          <li>設定 &gt; 一般 &gt; キーボード &gt; キーボード &gt; Nagi を開きます。</li>
          <li>「フルアクセスを許可」をオンにします。</li>
        </ol>
        <p>
          フルアクセスは、キーボードが<strong>本体アプリの保存した辞書・モデルを読むために必要</strong>です。
          Nagi のキーボードは通信を行わず、入力内容を外部へ送信しません（<a href="/privacy">プライバシーポリシー</a>）。
        </p>
        <p>
          許可しない場合に使える機能は、端末や iOS のバージョンにより異なる場合があります。辞書とモデルの利用に許可が必要になることがあります。入力した文字が失われることはありません。
        </p>
      </section>

      <section aria-labelledby="data">
        <h2 id="data">辞書・モデルを取り込むには</h2>
        <p>
          Nagi は辞書とモデルを同梱せず、別途用意する方式です。現在の TestFlight 版では、本体アプリの取り込み機能を使います。
        </p>
        <ul>
          <li>
            <strong>辞書</strong>: アプリ内のファイル（バンドル）から、ホームタブの「データ」にある「辞書をバンドルからコピー」で配置します。
          </li>
          <li>
            <strong>モデル</strong>: 「ファイル」アプリなどに置いた <code>.gguf</code> ファイルを、「モデルをインポート」で選びます。
            コピーに数十秒かかることがあります。
          </li>
          <li>
            「データ」タブで、辞書・モデルの有無、サイズ、使用中のモデルを確認できます。
            辞書がない場合は、読みをそのまま返します（モデルも読み込まれません）。
          </li>
        </ul>
        <p>
          辞書とモデルの出典とライセンスは、アプリの About タブで確認できます。取得元のサイトからの自動取得機能は、準備中です。
        </p>
      </section>

      <section aria-labelledby="slow">
        <h2 id="slow">変換が遅い・候補が出ないとき</h2>
        <ul>
          <li>
            <strong>漢字に変換されない</strong>: フルアクセスが許可されているか、辞書が取り込まれているかを確認してください
            （上の 2 項目）。ホームの案内は、フルアクセスが「未設定」のときにその旨を表示します。
          </li>
          <li>
            <strong>候補が出るのに時間がかかる</strong>:
            モデル（LLM）による変換は、端末のメモリに余裕がないと使われず、辞書だけの変換になります。
            「データ」タブで「LLM が使える見込み」を確認できます。
            設定タブで、LLM の使用やプロファイルを軽い方へ変えると改善することがあります。
          </li>
          <li>
            <strong>キーボードが突然閉じる・切り替わる</strong>:
            iOS のキーボード拡張にはメモリの上限があり、超えると OS が終了させます。
            その場合は、モデルを小さいもの（xsmall）に替えるか、LLM を無効にしてください。
          </li>
          <li>
            <strong>部分変換（先頭の文節だけを選ぶ）で、残りの文字が二重に出る</strong>:
            既知の不具合で、調査中です（再現する場合は、下のお問い合わせでお知らせください）。
          </li>
          <li>外付けキーボードでの入力は、キーボード拡張の仕組み上、対象外です（OS 標準の入力を使ってください）。</li>
        </ul>
      </section>

      <section aria-labelledby="delete">
        <h2 id="delete">学習データを削除するには</h2>
        <ol>
          <li>Nagi アプリの「設定」タブを開きます。</li>
          <li>「学習データをすべて削除」を選び、確認ダイアログで実行します。</li>
        </ol>
        <p>
          削除すると、以前の変換候補の順位は戻りません。学習データは端末内にのみあり、削除後は元に戻せません。
          パスワード欄などでは、もともと学習しません。アプリを削除すると、端末内の Nagi のデータはすべて消えます。
        </p>
      </section>

      <section aria-labelledby="layout">
        <h2 id="layout">キーボードのレイアウトを取り込むには</h2>
        <p>
          キーボードの配列は JSON で定義できます。<a href="/custom-keyboard">編集ページ</a>で作成して書き出したものを、
          本体アプリの「レイアウト」タブで取り込みます。
        </p>
        <ul>
          <li>JSON の貼り付け、またはファイルから取り込みます。不正な JSON のときは、理由が日本語で表示されます。</li>
          <li>
            <code>nagi://layout?data=…</code> 形式のリンクを Safari で開いても取り込めます。
          </li>
          <li>
            取り込んだレイアウトは、設定タブの「かな」「英字」などの割り当てで選びます。反映は、キーボードを開き直したときです。
            不正なレイアウトは既定のものに戻ります。
          </li>
          <li>レイアウトは一覧から削除できます（割り当て中のものを消すと、既定に戻ります）。</li>
        </ul>
        <p class="note">この機能は開発中で、仕様や画面は変わることがあります。</p>
      </section>

      <section aria-labelledby="contact">
        <h2 id="contact">お問い合わせ</h2>
        <p>
          不具合のご報告やご質問は、GitHub の <a href={ISSUES_URL} rel="noopener">Issues</a>
          （<a href={REPO_URL} rel="noopener">{REPO_URL}</a>）にお寄せください。GitHub のアカウントが必要です。
        </p>
        <p>報告のときは、次の情報があると原因を調べやすくなります。</p>
        <ul>
          <li>端末の機種と iOS のバージョン、Nagi のバージョン（About タブ）</li>
          <li>何をしたら、どうなったか（再現手順）</li>
          <li>必要に応じて診断ログ（入力テストタブからコピーできます）</li>
        </ul>
        <p class="note">
          診断ログには、再現のために入力した文字列が含まれることがあります。公開の場に貼る前に内容を確認し、
          個人情報やパスワードに当たるものは削除してください。
        </p>
      </section>
    </div>
  </div>
</main>

<style>
  .layout {
    display: grid;
    gap: 8px 72px;
  }
  @media (min-width: 960px) {
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
  .prose :global(section:first-child h2) {
    margin-top: 40px;
  }
</style>
