<svelte:head>
  <title>プライバシーポリシー（草案） - Nagi</title>
  <meta name="description" content="Nagi のプライバシーポリシー（草案）。" />
</svelte:head>

<main class="page">
  <h1>プライバシーポリシー</h1>

  <p class="draft" role="note">
    <strong>この文面は草案（placeholder）です。</strong>
    正式な公開前に内容を確定し、この表示を取り除きます。
  </p>

  <!-- TODO: 公開前に確定するもの
    - 事業者名・連絡先（問い合わせ先メールアドレス）
    - 施行日・改定履歴
    - App Store Connect の「App のプライバシー」回答との整合
    - 辞書・モデルの配布元（取得先ドメイン）の列挙
    - アップデート確認で送る情報（有無・内容）の確定
    - 法務確認（個人情報保護法に基づく表記の要否）
  -->

  <h2>1. 基本方針</h2>
  <p>
    Nagi は、入力した文章を外部のサーバーへ送信しません。かな漢字変換・学習・辞書の処理は、すべてお使いの端末の中で行います。
  </p>

  <h2>2. 端末内に保存するもの</h2>
  <ul>
    <li>学習データ（変換結果の履歴などの学習用ファイル）</li>
    <li>ユーザー辞書、設定、作成したキーボードレイアウト</li>
    <li>変換に使う辞書とモデルのファイル</li>
  </ul>
  <p>これらは端末内にのみ保存されます。学習データは、ワンコマンドまたはアプリの操作で全削除できるようにする方針です。</p>

  <h2>3. 通信について</h2>
  <p>Nagi が通信するのは、次の場合に限ります。</p>
  <ul>
    <li>辞書・モデルのダウンロード</li>
    <li>アップデートの確認（無効にできます）</li>
  </ul>
  <p>入力内容を、これらの通信に含めることはありません。</p>
  <p class="todo">TODO: アップデート確認で送信する情報（バージョン等）の内容を確定して記載する。</p>

  <h2>4. iOS の「フルアクセス」</h2>
  <p>
    iOS のキーボード拡張が、本体アプリの取得した辞書・モデルを読むために、「フルアクセスを許可」が必要です。
    この許可は辞書・モデルの読み込みのためのもので、キーボードから入力内容を送信することはありません。
    許可しない場合は、ひらがな・カタカナへの変換のみで動作します。
  </p>

  <h2>5. 入力内容の取り扱い</h2>
  <ul>
    <li>ログに入力内容を残しません（開発用ビルドで明示的に有効にした場合を除く）。</li>
    <li>パスワード欄など、入力の秘匿が求められる場面では、学習・周辺テキストの取得・ログを停止する方針です。</li>
  </ul>

  <h2>6. このサイトについて</h2>
  <p>
    このサイトに、アクセス解析や広告は設置していません。キーボード編集ページ（/custom-keyboard）で作成した内容は、
    お使いのブラウザの localStorage にのみ保存され、サーバーへは送信されません。
  </p>
  <p class="todo">TODO: ホスティング先が決まったら、そのアクセスログの扱いを追記する。</p>

  <h2>7. 変更・お問い合わせ</h2>
  <p class="todo">TODO: 改定の告知方法と連絡先を記載する。</p>
</main>

<style>
  .page {
    max-width: 760px;
    margin: 0 auto;
    padding: 24px 16px 0;
    box-sizing: border-box;
  }
  h1 {
    font-size: 2rem;
    margin: 16px 0;
  }
  h2 {
    font-size: 1.25rem;
    margin: 32px 0 8px;
  }
  .draft {
    border: 2px solid var(--danger);
    background: var(--danger-soft);
    border-radius: 12px;
    padding: 12px 16px;
  }
  .todo {
    border-left: 4px solid var(--muted);
    padding-left: 12px;
    color: var(--muted);
  }
</style>
