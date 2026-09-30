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
/// @return バージョン文字列（例：0.0.1）。解放不要。
const char *nagi_version(void);

/// サーバー初期化時の診断情報を返す。
///
/// @param handle nagi_server_new から返されたハンドル
///
/// @return 診断情報（起動時の notes + プロファイル情報）を UTF-8 C 文字列で返す。
///         nagi_string_free で解放必須。
///         handle が NULL の場合は、空文字列を返す。
char *nagi_last_notes(NagiServerHandle handle);

#ifdef __cplusplus
}
#endif

#endif // NAGI_FFI_H
