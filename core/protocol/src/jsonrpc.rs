//! JSON-RPC 2.0 のエンベロープ・エラーコード（typeshare 対象外）
//!
//! このモジュールは、JSON-RPC 2.0 の標準メッセージ形式とエラーコードを定義します。
//! Swift 側では手作業で対応する型を定義してください。

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// JSON-RPC リクエスト ID（数値または文字列）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RequestId {
    /// 数値 ID
    Number(i64),
    /// 文字列 ID
    String(String),
}

/// JSON-RPC 2.0 リクエスト
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    /// JSON-RPC バージョン（常に "2.0"）
    pub jsonrpc: String,
    /// リクエスト ID（None の場合は通知）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<RequestId>,
    /// メソッド名
    pub method: String,
    /// パラメータ（オプション）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl Request {
    /// リクエストが通知（ID がない）かどうか
    pub fn is_notification(&self) -> bool {
        self.id.is_none()
    }
}

/// JSON-RPC エラーオブジェクト
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorObject {
    /// エラーコード
    pub code: i32,
    /// エラーメッセージ
    pub message: String,
    /// 追加データ（オプション）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl ErrorObject {
    /// 新しいエラーオブジェクトを作成
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    /// データ付きでエラーオブジェクトを作成
    pub fn with_data(code: i32, message: impl Into<String>, data: Value) -> Self {
        Self {
            code,
            message: message.into(),
            data: Some(data),
        }
    }
}

/// JSON-RPC 2.0 応答
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    /// JSON-RPC バージョン（常に "2.0"）
    pub jsonrpc: String,
    /// リクエスト ID
    pub id: Option<RequestId>,
    /// 成功時の結果
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    /// 失敗時のエラー
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorObject>,
}

impl Response {
    /// 成功応答を作成
    pub fn success(id: RequestId, result: impl Serialize) -> Result<Self, serde_json::Error> {
        Ok(Self {
            jsonrpc: "2.0".to_string(),
            id: Some(id),
            result: Some(serde_json::to_value(result)?),
            error: None,
        })
    }

    /// 失敗応答を作成
    pub fn error(id: Option<RequestId>, error: ErrorObject) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(error),
        }
    }
}

/// JSON-RPC 2.0 通知（送信用）
#[derive(Debug, Clone, Serialize)]
pub struct Notification {
    /// JSON-RPC バージョン（常に "2.0"）
    pub jsonrpc: String,
    /// メソッド名
    pub method: String,
    /// パラメータ
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl Notification {
    /// 新しい通知を作成
    pub fn new(
        method: impl Into<String>,
        params: impl Serialize,
    ) -> Result<Self, serde_json::Error> {
        Ok(Self {
            jsonrpc: "2.0".to_string(),
            method: method.into(),
            params: Some(serde_json::to_value(params)?),
        })
    }

    /// パラメータなしの通知を作成
    pub fn without_params(method: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            method: method.into(),
            params: None,
        }
    }
}

pub mod error_code {
    //! JSON-RPC エラーコード定数
    //!
    //! JSON-RPC 2.0 標準エラーコード + Nagi アプリ固有コード

    /// Parse error: -32700
    pub const PARSE_ERROR: i32 = -32700;

    /// Invalid Request: -32600
    pub const INVALID_REQUEST: i32 = -32600;

    /// Method not found: -32601
    pub const METHOD_NOT_FOUND: i32 = -32601;

    /// Invalid params: -32602
    pub const INVALID_PARAMS: i32 = -32602;

    /// Internal error: -32603
    pub const INTERNAL_ERROR: i32 = -32603;

    // Nagi アプリ固有エラーコード（-32000 ～ -32099）

    /// Session not found: -32001
    pub const SESSION_NOT_FOUND: i32 = -32001;

    /// Not initialized: -32002
    pub const NOT_INITIALIZED: i32 = -32002;

    /// Not implemented: -32003
    pub const NOT_IMPLEMENTED: i32 = -32003;

    /// Engine failure: -32004
    pub const ENGINE_FAILURE: i32 = -32004;

    /// Unsupported protocol version: -32005
    pub const UNSUPPORTED_PROTOCOL_VERSION: i32 = -32005;
}

/// 1 行の JSON-RPC メッセージをパースする
///
/// # Arguments
///
/// * `line` - JSON テキスト（改行で区切られた 1 つのメッセージ）
///
/// # Returns
///
/// パースに成功した Request、または パースエラー/不正なリクエスト
///
/// # Errors
///
/// - JSON が不正 → `PARSE_ERROR`
/// - `jsonrpc: "2.0"` でない → `INVALID_REQUEST`
/// - 必須フィールド（method）がない → `INVALID_REQUEST`
pub fn parse_message(line: &str) -> Result<Request, ErrorObject> {
    let value: Value = serde_json::from_str(line)
        .map_err(|e| ErrorObject::new(error_code::PARSE_ERROR, format!("Parse error: {}", e)))?;

    let obj = value.as_object().ok_or_else(|| {
        ErrorObject::new(error_code::INVALID_REQUEST, "Request must be a JSON object")
    })?;

    // jsonrpc フィールドが "2.0" か確認
    let jsonrpc = obj.get("jsonrpc").and_then(|v| v.as_str()).ok_or_else(|| {
        ErrorObject::new(
            error_code::INVALID_REQUEST,
            "Missing or invalid jsonrpc field",
        )
    })?;

    if jsonrpc != "2.0" {
        return Err(ErrorObject::new(
            error_code::INVALID_REQUEST,
            format!("jsonrpc must be \"2.0\", got {:?}", jsonrpc),
        ));
    }

    // method フィールドが必須
    let method = obj
        .get("method")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            ErrorObject::new(
                error_code::INVALID_REQUEST,
                "Missing or invalid method field",
            )
        })?
        .to_string();

    // id フィールドはオプション
    let id = obj.get("id").map(|v| {
        if v.is_i64() {
            RequestId::Number(v.as_i64().unwrap())
        } else if let Some(s) = v.as_str() {
            RequestId::String(s.to_string())
        } else {
            // id は数値または文字列でなければならない
            // ここでエラーにしてもいいが、とりあえず string に変換してみる
            RequestId::String(v.to_string())
        }
    });

    // params フィールドはオプション
    let params = obj.get("params").cloned();

    Ok(Request {
        jsonrpc: jsonrpc.to_string(),
        id,
        method,
        params,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_message_valid_request() {
        let json = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"version":1}}"#;
        let req = parse_message(json).unwrap();
        assert_eq!(req.jsonrpc, "2.0");
        assert_eq!(req.method, "initialize");
        assert_eq!(req.id, Some(RequestId::Number(1)));
        assert!(req.params.is_some());
    }

    #[test]
    fn test_parse_message_valid_notification() {
        let json = r#"{"jsonrpc":"2.0","method":"set_context","params":{"session_id":"s1"}}"#;
        let req = parse_message(json).unwrap();
        assert!(req.is_notification());
    }

    #[test]
    fn test_parse_message_string_id() {
        let json = r#"{"jsonrpc":"2.0","id":"abc","method":"ping"}"#;
        let req = parse_message(json).unwrap();
        assert_eq!(req.id, Some(RequestId::String("abc".to_string())));
    }

    #[test]
    fn test_parse_message_invalid_json() {
        let json = r#"{"jsonrpc":"2.0""#;
        let err = parse_message(json).unwrap_err();
        assert_eq!(err.code, error_code::PARSE_ERROR);
    }

    #[test]
    fn test_parse_message_invalid_jsonrpc_version() {
        let json = r#"{"jsonrpc":"1.0","id":1,"method":"initialize"}"#;
        let err = parse_message(json).unwrap_err();
        assert_eq!(err.code, error_code::INVALID_REQUEST);
    }

    #[test]
    fn test_parse_message_missing_method() {
        let json = r#"{"jsonrpc":"2.0","id":1}"#;
        let err = parse_message(json).unwrap_err();
        assert_eq!(err.code, error_code::INVALID_REQUEST);
    }

    #[test]
    fn test_parse_message_not_object() {
        let json = r#"[1,2,3]"#;
        let err = parse_message(json).unwrap_err();
        assert_eq!(err.code, error_code::INVALID_REQUEST);
    }

    #[test]
    fn test_response_success() -> Result<(), Box<dyn std::error::Error>> {
        let result = serde_json::json!({"status": "ok"});
        let resp = Response::success(RequestId::Number(1), result)?;
        assert_eq!(resp.jsonrpc, "2.0");
        assert!(resp.result.is_some());
        assert!(resp.error.is_none());
        Ok(())
    }

    #[test]
    fn test_response_error() {
        let err = ErrorObject::new(error_code::METHOD_NOT_FOUND, "Unknown method");
        let resp = Response::error(Some(RequestId::Number(1)), err);
        assert_eq!(resp.jsonrpc, "2.0");
        assert!(resp.result.is_none());
        assert!(resp.error.is_some());
    }

    #[test]
    fn test_notification_serialization() -> Result<(), Box<dyn std::error::Error>> {
        let notif = Notification::new("update_preedit", serde_json::json!({"session_id": "s1"}))?;
        let json = serde_json::to_string(&notif)?;
        assert!(json.contains("update_preedit"));
        assert!(json.contains("s1"));
        Ok(())
    }
}
