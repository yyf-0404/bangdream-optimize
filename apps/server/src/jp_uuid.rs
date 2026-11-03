use super::{account_import::ImportTrace, game_data_root_for_calculation};
use axum::{extract::rejection::JsonRejection, response::Response, Json};
use bangdream_optimize_bangdream_account::{ImportRegion, JpUuidImportRequest, JpUuidImporter};
use bangdream_optimize_data::BestdoriFilesystemConfig;

pub async fn import_jp_uuid(body: Result<Json<JpUuidImportRequest>, JsonRejection>) -> Response {
    let trace = ImportTrace::new(ImportRegion::Jp);
    let request = match trace.request(body, "请填写日服玩家 ID 和 UUID") {
        Ok(request) => request,
        Err(response) => return response,
    };
    if let Err(response) = trace.validate(request.validate()) {
        return response;
    }
    let cards = game_data_root_for_calculation()
        .and_then(|root| BestdoriFilesystemConfig::from_root(root).cards_dir);
    trace
        .run(
            move |permit| JpUuidImporter::new(cards).import_with_permit(request, permit),
            |data| &data.player,
        )
        .await
}

#[cfg(test)]
mod tests {
    use crate::account_import::test_support::CapturedLogs;
    use axum::{
        body::{to_bytes, Body},
        http::{header, Request, StatusCode},
    };
    use tower::ServiceExt;

    #[tokio::test]
    async fn jp_uuid_endpoint_rejects_bad_input_without_echoing_uuid() {
        let captured = CapturedLogs::new(tracing::Level::WARN);
        let app = crate::build_app(crate::AppState::default(), None, None, false);
        let uuid = "00000000-0000-4000-8000-000000000001";
        let request = Request::post("/api/import/jp-uuid")
            .header("content-type", "application/json")
            .body(Body::from(format!(r#"{{"playerId":0,"uuid":"{uuid}"}}"#)))
            .unwrap();
        let response = captured.capture(app.oneshot(request)).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        let import_id = response.headers()["x-import-id"]
            .to_str()
            .unwrap()
            .to_owned();
        assert!(import_id.starts_with("jp-"));
        let bytes = to_bytes(response.into_body(), 2000).await.unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains(uuid));
        let logs = captured.text();
        assert!(logs.contains(&import_id));
        assert!(logs.contains("request_validation"));
        assert!(logs.contains("elapsed_ms="));
        assert!(!logs.contains(uuid), "{logs}");
    }
}
