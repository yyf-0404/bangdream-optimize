use super::{game_data_root_for_calculation, ApiError, ApiResponse};
use axum::{
    extract::rejection::JsonRejection,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use bangdream_optimize_bangdream_account::{JpUuidImportRequest, JpUuidImporter};
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

pub async fn import_jp_uuid(body: Result<Json<JpUuidImportRequest>, JsonRejection>) -> Response {
    let Json(request) = match body {
        Ok(body) => body,
        Err(_) => return error(StatusCode::BAD_REQUEST, "请填写日服玩家 ID 和 UUID"),
    };
    if let Err(err) = request.validate() {
        return error(StatusCode::BAD_REQUEST, err.to_string());
    }
    let cards = game_data_root_for_calculation()
        .and_then(|root| BestdoriFilesystemConfig::from_root(root).cards_dir);
    match tokio::task::spawn_blocking(move || JpUuidImporter::new(cards).import(request)).await {
        Ok(Ok(data)) => (
            [(header::CACHE_CONTROL, "no-store")],
            Json(ApiResponse { status: "ok", data }),
        )
            .into_response(),
        Ok(Err(err)) => error(StatusCode::BAD_GATEWAY, err.to_string()),
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "日服资料读取未完成，请稍后重试",
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
    async fn jp_uuid_endpoint_rejects_bad_input_without_echoing_uuid() {
        let app = crate::build_app(crate::AppState::default(), None, None, false);
        let uuid = "00000000-0000-4000-8000-000000000001";
        let request = Request::post("/api/import/jp-uuid")
            .header("content-type", "application/json")
            .body(Body::from(format!(r#"{{"playerId":0,"uuid":"{uuid}"}}"#)))
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        let bytes = to_bytes(response.into_body(), 2000).await.unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains(uuid));
    }
}
