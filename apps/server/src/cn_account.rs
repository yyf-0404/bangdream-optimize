use super::{account_import::ImportTrace, game_data_root_for_calculation};
use axum::{extract::rejection::JsonRejection, response::Response, Json};
use bangdream_optimize_bangdream_account::{
    CredentialImportRequest, CredentialImporter, ImportRegion,
};
use bangdream_optimize_data::BestdoriFilesystemConfig;

pub async fn import_account(
    body: Result<Json<CredentialImportRequest>, JsonRejection>,
) -> Response {
    let trace = ImportTrace::new(ImportRegion::Cn);
    let request = match trace.request(body, "请填写账号、密码并选择 bili安卓或 iOS 渠道")
    {
        Ok(request) => request,
        Err(response) => return response,
    };
    trace
        .span
        .record("channel", tracing::field::debug(request.channel));
    if let Err(response) = trace.validate(request.validate()) {
        return response;
    }
    let cards = game_data_root_for_calculation()
        .and_then(|root| BestdoriFilesystemConfig::from_root(root).cards_dir);
    let versions = super::non_empty_env("BANGDREAM_OPTIMIZE_CN_VERSION_CONFIG")
        .unwrap_or_else(|| "var/bangdream-account/client-version.json".to_owned());
    trace
        .run(
            move |permit| {
                CredentialImporter::new(cards)
                    .with_version_config(versions)
                    .import_with_permit(request, permit)
            },
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
    use std::collections::HashSet;
    use tower::ServiceExt;

    #[tokio::test]
    async fn account_endpoint_rejects_bad_input_without_echoing_credentials_or_using_spa_fallback()
    {
        let captured = CapturedLogs::new(tracing::Level::WARN);
        let app = crate::build_app(crate::AppState::default(), None, None, false);
        let mut import_ids = HashSet::new();
        captured
            .capture(async {
                for (content_type, body) in [
                (
                    true,
                    r#"{"account":"","password":"fixture-secret","channel":"android"}"#.to_owned(),
                ),
                (
                    true,
                    r#"{"account":"a","password":"fixture-secret"}"#.to_owned(),
                ),
                (
                    true,
                    r#"{"account":"a","password":"fixture-secret","channel":"ios","user_id":123}"#
                        .to_owned(),
                ),
                (
                    true,
                    r#"{"account":"a","password":"fixture-secret","channel":"fixture-secret"}"#
                        .to_owned(),
                ),
                (true, r#"{"password":"fixture-secret""#.to_owned()),
                (true, "x".repeat(9000)),
                (false, r#"{"password":"fixture-secret"}"#.to_owned()),
            ] {
                let mut request = Request::post("/api/import/cn-account");
                if content_type {
                    request = request.header("content-type", "application/json");
                }
                let response = app
                    .clone()
                    .oneshot(request.body(Body::from(body)).unwrap())
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::BAD_REQUEST);
                assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
                let import_id = response.headers()["x-import-id"]
                    .to_str()
                    .unwrap()
                    .to_owned();
                assert!(import_id.starts_with("cn-"));
                assert!(import_ids.insert(import_id));
                let body = to_bytes(response.into_body(), 2000).await.unwrap();
                let text = String::from_utf8(body.to_vec()).unwrap();
                assert!(!text.contains("fixture-secret"));
                assert!(serde_json::from_str::<serde_json::Value>(&text).is_ok());
            }
            })
            .await;
        let logs = captured.text();
        assert!(!logs.contains("fixture-secret"), "{logs}");
        for import_id in import_ids {
            assert!(logs.contains(&import_id), "{logs}");
        }
        for field in [
            "request_validation",
            "elapsed_ms=",
            "outcome=\"failed\"",
            "invalid_fields",
            "invalid_json",
            "missing_content_type",
            "body_read_or_size_limit",
        ] {
            assert!(logs.contains(field), "{logs}");
        }
    }
}
