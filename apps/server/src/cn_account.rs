use super::{game_data_root_for_calculation, ApiError, ApiResponse};
use axum::{
    extract::rejection::JsonRejection,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use bangdream_optimize_bangdream_account::{CredentialImportRequest, CredentialImporter};
use bangdream_optimize_data::BestdoriFilesystemConfig;

fn error(status: StatusCode, message: impl Into<String>) -> Response {
    (
        status,
        [(header::CACHE_CONTROL, "no-store")],
        Json(ApiError {
            status: "error",
            message: message.into(),
        }),
    )
        .into_response()
}

pub async fn import_account(
    body: Result<Json<CredentialImportRequest>, JsonRejection>,
) -> Response {
    let Json(request) = match body {
        Ok(body) => body,
        Err(_) => {
            return error(
                StatusCode::BAD_REQUEST,
                "请填写账号、密码并选择 bili安卓或 iOS 渠道",
            )
        }
    };
    if let Err(err) = request.validate() {
        return error(StatusCode::BAD_REQUEST, err.to_string());
    }
    let cards = game_data_root_for_calculation()
        .and_then(|root| BestdoriFilesystemConfig::from_root(root).cards_dir);
    let versions = super::non_empty_env("BANGDREAM_OPTIMIZE_CN_VERSION_CONFIG")
        .unwrap_or_else(|| "var/bangdream-account/client-version.json".to_owned());
    // Never trace the request, upstream body, password or session headers.
    match tokio::task::spawn_blocking(move || {
        CredentialImporter::new(cards)
            .with_version_config(versions)
            .import(request)
    })
    .await
    {
        Ok(Ok(data)) => (
            [(header::CACHE_CONTROL, "no-store")],
            Json(ApiResponse { status: "ok", data }),
        )
            .into_response(),
        Ok(Err(err)) => error(StatusCode::BAD_GATEWAY, err.to_string()),
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "账号读取未完成，请稍后重试",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{to_bytes, Body},
        http::Request,
    };
    use tower::ServiceExt;
    #[tokio::test]
    async fn account_endpoint_rejects_bad_input_without_echoing_credentials_or_using_spa_fallback()
    {
        let app = crate::build_app(crate::AppState::default(), None, None, false);
        for body in [
            r#"{"account":"","password":"fixture-secret","channel":"android"}"#.to_owned(),
            r#"{"account":"a","password":"fixture-secret"}"#.to_owned(),
            r#"{"account":"a","password":"fixture-secret","channel":"ios","user_id":123}"#
                .to_owned(),
            "x".repeat(9000),
        ] {
            let request = Request::post("/api/import/cn-account")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap();
            let response = app.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
            let bytes = to_bytes(response.into_body(), 2000).await.unwrap();
            let text = String::from_utf8(bytes.to_vec()).unwrap();
            assert!(!text.contains("fixture-secret"));
            assert!(serde_json::from_str::<serde_json::Value>(&text).is_ok());
        }
    }
}
