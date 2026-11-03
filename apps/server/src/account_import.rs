use super::{ApiError, ApiResponse};
use axum::{
    extract::rejection::JsonRejection,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use bangdream_optimize_bangdream_account::{ImportError, ImportPermit, ImportRegion};
use bangdream_optimize_core::PlayerConfig;
use serde::Serialize;
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

static IMPORT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(super) struct ImportTrace {
    region: ImportRegion,
    import_id: String,
    started: Instant,
    pub(super) span: tracing::Span,
}

impl ImportTrace {
    pub fn new(region: ImportRegion) -> Self {
        let started = Instant::now();
        let import_id = format!(
            "{}-{:x}-{:x}",
            region.as_str(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
            IMPORT_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        );
        let span = tracing::info_span!("account_import", import_id = %import_id,
            server = region.as_str(), channel = tracing::field::Empty);
        tracing::info!(parent: &span, stage = "request_received", "Account import started");
        Self {
            region,
            import_id,
            started,
            span,
        }
    }

    pub fn request<T>(
        &self,
        body: Result<Json<T>, JsonRejection>,
        message: &'static str,
    ) -> Result<T, Response> {
        match body {
            Ok(Json(request)) => Ok(request),
            Err(rejection) => {
                // JsonRejection can contain submitted field values. Never format it.
                let reason = match &rejection {
                    JsonRejection::JsonSyntaxError(_) => "invalid_json",
                    JsonRejection::JsonDataError(_) => "invalid_fields",
                    JsonRejection::MissingJsonContentType(_) => "missing_content_type",
                    JsonRejection::BytesRejection(_) => "body_read_or_size_limit",
                    _ => "invalid_request",
                };
                tracing::warn!(parent: &self.span, import_id = %self.import_id,
                    stage = "request_validation", outcome = "failed", reason,
                    rejection_status = rejection.status().as_u16(), elapsed_ms = self.elapsed_ms(),
                    "Account import request rejected");
                Err(self.error(StatusCode::BAD_REQUEST, message))
            }
        }
    }

    pub fn validate(&self, result: Result<(), ImportError>) -> Result<(), Response> {
        result.map_err(|error| {
            tracing::warn!(parent: &self.span, import_id = %self.import_id,
                stage = "request_validation", outcome = "failed", error = %error,
                elapsed_ms = self.elapsed_ms(), "Account import request rejected");
            self.error(StatusCode::BAD_REQUEST, error.to_string())
        })
    }

    pub async fn run<T: Serialize + Send + 'static>(
        self,
        action: impl FnOnce(ImportPermit) -> Result<T, ImportError> + Send + 'static,
        profile: impl FnOnce(&T) -> &PlayerConfig + Send,
    ) -> Response {
        // Reserve before spawn_blocking: rejected requests never enter the worker queue.
        let permit = match self.span.in_scope(|| self.region.try_acquire()) {
            Ok(permit) => permit,
            Err(error) => return self.failed(error),
        };
        let blocking_span = self.span.clone();
        let dispatcher = tracing::dispatcher::get_default(Clone::clone);
        match tokio::task::spawn_blocking(move || {
            tracing::dispatcher::with_default(&dispatcher, || {
                blocking_span.in_scope(|| action(permit))
            })
        })
        .await
        {
            Ok(Ok(data)) => {
                let player = profile(&data);
                tracing::info!(parent: &self.span, outcome = "completed", elapsed_ms = self.elapsed_ms(),
                    cards = player.card_list.len(), area_items = player.area_item.len(),
                    characters = player.character_bouns.len(), "Account import completed");
                self.response(Json(ApiResponse { status: "ok", data }).into_response())
            }
            Ok(Err(error)) => self.failed(error),
            Err(error) => {
                // Panic payloads may contain private values; only record worker state.
                tracing::error!(parent: &self.span, import_id = %self.import_id,
                    outcome = "failed", panicked = error.is_panic(), cancelled = error.is_cancelled(),
                    elapsed_ms = self.elapsed_ms(), "Account import worker failed");
                self.error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "账号读取未完成，请稍后重试",
                )
            }
        }
    }

    fn failed(&self, error: ImportError) -> Response {
        tracing::error!(parent: &self.span, import_id = %self.import_id, outcome = "failed",
            error = %error, elapsed_ms = self.elapsed_ms(), "Account import failed");
        self.error(StatusCode::BAD_GATEWAY, error.to_string())
    }

    fn elapsed_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    fn error(&self, status: StatusCode, message: impl Into<String>) -> Response {
        self.response(
            (
                status,
                Json(ApiError {
                    status: "error",
                    message: message.into(),
                }),
            )
                .into_response(),
        )
    }

    fn response(&self, mut response: Response) -> Response {
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
        response.headers_mut().insert(
            "x-import-id",
            self.import_id.parse().expect("generated import ID"),
        );
        response
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::{
        io::Write,
        sync::{Arc, Mutex, OnceLock},
    };
    use tracing::instrument::WithSubscriber;

    pub(crate) struct CapturedLogs {
        bytes: Arc<Mutex<Vec<u8>>>,
        dispatcher: tracing::Dispatch,
    }

    struct Writer(Arc<Mutex<Vec<u8>>>);
    impl Write for Writer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().write(bytes)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl CapturedLogs {
        pub(crate) fn new(level: tracing::Level) -> Self {
            // Keep callsite registration correct while other test threads have no subscriber.
            static BASE: OnceLock<tracing::Dispatch> = OnceLock::new();
            BASE.get_or_init(|| {
                tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default())
            });
            let bytes = Arc::new(Mutex::new(Vec::new()));
            let output = bytes.clone();
            let subscriber = tracing_subscriber::fmt()
                .without_time()
                .with_ansi(false)
                .with_max_level(level)
                .with_writer(move || Writer(output.clone()))
                .finish();
            Self {
                bytes,
                dispatcher: tracing::Dispatch::new(subscriber),
            }
        }

        pub(crate) async fn capture<F: std::future::Future>(&self, future: F) -> F::Output {
            future.with_subscriber(self.dispatcher.clone()).await
        }

        pub(crate) fn text(&self) -> String {
            String::from_utf8(self.bytes.lock().unwrap().clone()).unwrap()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_support::CapturedLogs;

    #[tokio::test]
    async fn admission_is_per_region_precedes_workers_and_preserves_trace_in_workers() {
        let logs = CapturedLogs::new(tracing::Level::INFO);
        logs.capture(async {
            let cn = ImportRegion::Cn.try_acquire().unwrap();
            let jp = ImportRegion::Jp.try_acquire().unwrap();
            for region in [ImportRegion::Cn, ImportRegion::Jp] {
                let response = ImportTrace::new(region)
                    .run(
                        |_| -> Result<PlayerConfig, ImportError> {
                            panic!("rejected import must not dispatch a worker")
                        },
                        |player| player,
                    )
                    .await;
                assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
                assert!(response.headers()["x-import-id"]
                    .to_str()
                    .unwrap()
                    .starts_with(region.as_str()));
                assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
            }
            drop(jp);
            let response = ImportTrace::new(ImportRegion::Jp)
                .run(
                    |_permit| {
                        assert!(ImportRegion::Jp.try_acquire().is_err());
                        tracing::info!(stage = "fixture_worker", "fixture import worker");
                        Ok(serde_json::from_value::<PlayerConfig>(
                            serde_json::json!({"playerId":42}),
                        )
                        .unwrap())
                    },
                    |player| player,
                )
                .await;
            assert_eq!(response.status(), StatusCode::OK);
            let id = response.headers()["x-import-id"].to_str().unwrap();
            let text = logs.text();
            let worker = text
                .lines()
                .find(|line| line.contains("fixture_worker"))
                .unwrap();
            assert!(worker.contains(id), "{text}");
            assert!(worker.contains("server=\"jp\""), "{text}");
            assert!(ImportRegion::Jp.try_acquire().is_ok());
            assert!(ImportRegion::Cn.try_acquire().is_err());
            drop(cn);
            let response = ImportTrace::new(ImportRegion::Cn)
                .run(
                    |_| {
                        Err::<PlayerConfig, _>(ImportError::Account(
                            "fixture import failure".into(),
                        ))
                    },
                    |player| player,
                )
                .await;
            assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
            assert!(logs
                .text()
                .contains(response.headers()["x-import-id"].to_str().unwrap()));
            assert!(ImportRegion::Cn.try_acquire().is_ok());
        })
        .await;
    }
}
