/// Nagi コアエンジンの C インタフェース（iOS キーボード拡張等での利用）
///
/// 所有権ルール（C 呼び出し側の責務）:
/// - `nagi_server_new()` が返すハンドルは `nagi_server_free()` で明示的に解放する必須。
/// - `nagi_handle_line()` と `nagi_last_notes()` が返すポインタ（C 文字列）は
///   `nagi_string_free()` で明示的に解放する必須。メモリリークを避けるため。
/// - ハンドルの使用中に `nagi_server_free()` を呼ぶと未定義動作。

#ifndef NAGI_FFI_H
#define NAGI_FFI_H

#ifdef __cplusplus
extern "C" {
#endif

#include <stdint.h>

/// サーバーハンドル（不透明型）
typedef struct NagiServerHandle_ *NagiServerHandle;

// ---- 初期化 ----

/// サーバーを初期化して返す。
///
/// @param data_dir データディレクトリのパス（UTF-8, NULL で未指定）
/// @param user_dir ユーザーデータディレクトリのパス（UTF-8, NULL で未指定）
/// @param no_llm 0 以外なら LLM を無効化
///
/// @return 成功時は不透明なハンドル（nagi_server_free で解放必須）、
///         失敗時（UTF-8 デコード失敗等）は NULL。
NagiServerHandle nagi_server_new(const char *data_dir, const char *user_dir, int no_llm);

/// サーバーを初期化して返す（プロセスのメモリ上限を指定する版）。
///
/// @param data_dir データディレクトリのパス（UTF-8, NULL で未指定）
/// @param user_dir ユーザーデータディレクトリのパス（UTF-8, NULL で未指定）
/// @param no_llm 0 以外なら LLM を無効化
/// @param memory_budget_mb このプロセスが OS から課されているメモリ上限（MB）。0 は指定なし。
///        iOS のキーボード拡張では phys_footprint + os_proc_available_memory() を渡す。
///        LLM に必要な量（150MB）未満なら、LLM を読み込まず辞書のみで動く。
///
/// @return 成功時は不透明なハンドル（nagi_server_free で解放必須）、失敗時は NULL。
NagiServerHandle nagi_server_new_with_budget(const char *data_dir, const char *user_dir, int no_llm, uint64_t memory_budget_mb);

// ---- JSON-RPC 処理 ----

/// JSON-RPC リクエスト行を処理し、応答行を返す。
///
/// @param handle nagi_server_new から返されたハンドル
/// @param line JSON-RPC リクエストの1行（UTF-8）
///
/// @return 応答行（改行区切り JSON-RPC 行を連結）を UTF-8 C 文字列で返す。
///         nagi_string_free で解放必須。
///         handle が NULL またはエラーの場合は、空文字列を返す。
char *nagi_handle_line(NagiServerHandle handle, const char *line);

// ---- 非同期通知 ----

/// 通知が取り出せるようになったときに呼ばれるコールバックの型。
typedef void (*NagiWakeCallback)(void *user_data);

/// 通知（update_preedit / update_candidates / notify）が取り出せるようになったときに呼ばれる
/// コールバックを登録する。LLM が同期の待ち時間（conversion.sync_budget_ms）を超えた分の完了や
/// ライブ変換の結果など、次のリクエストを待たずに届けるべき通知のために使う。
///
/// コールバックは任意のスレッドから、handle のロック内で呼ばれる。中で nagi_* を呼ばず、
/// 呼び出し側のキューに nagi_poll_notifications の呼び出しを積んですぐに戻ること。
/// user_data は nagi_server_free が返るまで有効であること（解放後は呼ばれない）。
///
/// @param handle nagi_server_new から返されたハンドル
/// @param callback コールバック。NULL なら登録を解除する
/// @param user_data コールバックにそのまま渡す値
void nagi_set_waker(NagiServerHandle handle, NagiWakeCallback callback, void *user_data);

/// 次のリクエストを待たずに配信すべき通知の行を取り出す。
///
/// @return 通知行（改行区切りの JSON-RPC 通知）。なければ空文字列。nagi_string_free で解放必須。
///         initialize の前は通知を取り出さず、空文字列を返す。
///         nagi_handle_line と同時に呼ばないこと（同一のキューで直列に呼ぶ）。
char *nagi_poll_notifications(NagiServerHandle handle);

// ---- 解放 ----

/// UTF-8 C 文字列を解放する。
///
/// @param s nagi_handle_line または nagi_last_notes から返された文字列ポインタ
void nagi_string_free(char *s);

/// サーバーハンドルを解放する。
///
/// @param handle nagi_server_new から返されたハンドル
void nagi_server_free(NagiServerHandle handle);

// ---- メタデータ ----

/// Nagi コアエンジンのバージョン文字列（静的）を返す。
///
/// @return バージョン文字列（ワークスペースの version。例：0.1.0）。解放不要。
const char *nagi_version(void);

/// サーバー初期化時の診断情報を返す。
///
/// @param handle nagi_server_new から返されたハンドル
///
/// @return 診断情報（起動時の notes + プロファイル情報）を UTF-8 C 文字列で返す。
///         nagi_string_free で解放必須。
///         handle が NULL の場合は、空文字列を返す。
char *nagi_last_notes(NagiServerHandle handle);

// ---- 任意データ（英語の語彙・カタカナ語の原語）----
//
// 取得（ネットワーク）は本体アプリが行い、ここは取得元の一覧と、取得済みアーカイブからの
// 索引の組み立てだけを提供する。

/// 任意データの取得元の一覧（JSON）を返す。
///
/// {"english": {"dir","file","url","sha256","index_file"}, "loanword": {...}}。
/// sha256 は取得したファイルの SHA-256（検証して一致しなければ使わない）。
///
/// @return UTF-8 C 文字列。nagi_string_free で解放必須。
char *nagi_optional_data_sources(void);

/// 取得済みのアーカイブから、任意データの索引を組み立てる。
///
/// @param kind "english"（SCOWL の tar.gz → words.fst）または "loanword"（JMdict の zip → loanword.fst）
/// @param archive_path 取得済みのアーカイブのパス（UTF-8）
/// @param out_path 出力する索引のパス（UTF-8。一時ファイル経由で置き換える）
///
/// @return 成功なら NULL。失敗なら理由（nagi_string_free で解放必須）。
char *nagi_build_optional_index(const char *kind, const char *archive_path, const char *out_path);

#ifdef __cplusplus
}
#endif

#endif // NAGI_FFI_H
