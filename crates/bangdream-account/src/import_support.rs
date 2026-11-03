//! Shared import admission, phase logs and redacted transport diagnostics.
use crate::ImportError;
use std::{
    io::Read,
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

static CN_IMPORT_BUSY: AtomicBool = AtomicBool::new(false);
static JP_IMPORT_BUSY: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ImportRegion {
    Cn,
    Jp,
}

impl ImportRegion {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cn => "cn",
            Self::Jp => "jp",
        }
    }

    fn busy(self) -> &'static AtomicBool {
        match self {
            Self::Cn => &CN_IMPORT_BUSY,
            Self::Jp => &JP_IMPORT_BUSY,
        }
    }

    /// Reserve one import for this region without waiting. CN and JP are independent.
    pub fn try_acquire(self) -> Result<ImportPermit, ImportError> {
        import_stage("import_lock", || {
            self.busy()
                .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                .map(|_| ImportPermit { region: self })
                .map_err(|_| fail("已有账号正在读取，请稍后再试"))
        })
    }
}

/// Owned admission permit: it can move into a blocking worker and releases on drop.
/// Its fields are private so callers cannot bypass admission by constructing one.
pub struct ImportPermit {
    region: ImportRegion,
}

impl Drop for ImportPermit {
    fn drop(&mut self) {
        self.region.busy().store(false, Ordering::Release);
    }
}

pub(crate) fn admit_import(
    region: ImportRegion,
    permit: Option<ImportPermit>,
) -> Result<ImportPermit, ImportError> {
    match permit {
        Some(permit) if permit.region == region => Ok(permit),
        Some(_) => import_stage("import_lock", || Err(fail("导入许可与服务器不一致"))),
        None => region.try_acquire(),
    }
}

pub(crate) fn fail(message: impl Into<String>) -> ImportError {
    ImportError::Account(message.into())
}

pub(crate) fn import_error_summary(error: &ImportError) -> String {
    match error {
        // These variants may carry URLs or arbitrary JSON values. Do not format them.
        ImportError::Http(_) => "upstream transport failure".into(),
        ImportError::ParsePersist(_) => "invalid client-version JSON".into(),
        ImportError::HttpStatus { status, .. } => format!("upstream HTTP {status}"),
        _ => error.to_string(),
    }
}

pub(crate) fn import_stage<T>(
    stage: &'static str,
    action: impl FnOnce() -> Result<T, ImportError>,
) -> Result<T, ImportError> {
    let started = Instant::now();
    tracing::info!(stage, outcome = "started", "Account import stage");
    let result = action();
    let elapsed_ms = started.elapsed().as_millis() as u64;
    match &result {
        Ok(_) => tracing::info!(
            stage,
            outcome = "completed",
            elapsed_ms,
            "Account import stage"
        ),
        Err(error) => tracing::warn!(stage, outcome = "failed", elapsed_ms,
            error = %import_error_summary(error), "Account import stage failed"),
    }
    result
}

pub(crate) fn logged_endpoint(endpoint: &str) -> &str {
    if endpoint.starts_with("/suite/user/") {
        "/suite/user/{self}"
    } else {
        endpoint
    }
}

pub(crate) fn log_transport_failure(operation: &str, error: reqwest::Error) {
    tracing::warn!(operation = logged_endpoint(operation), timeout = error.is_timeout(),
        connect = error.is_connect(), body = error.is_body(), decode = error.is_decode(),
        error = ?error.without_url(), "Account import upstream transport failed");
}

pub(crate) fn log_body_failure(error: std::io::Error) {
    tracing::warn!(error_kind = ?error.kind(), "Account import response body read failed");
    if let Some(source) = error.into_inner() {
        if let Ok(error) = source.downcast::<reqwest::Error>() {
            log_transport_failure("response_body", *error);
        }
    }
}

pub(crate) fn bounded_body(
    response: reqwest::blocking::Response,
    max: usize,
    service: &str,
) -> Result<Vec<u8>, ImportError> {
    if response.content_length().is_some_and(|n| n > max as u64) {
        tracing::warn!(limit_bytes = max, response_bytes = ?response.content_length(),
            "Account import response exceeds size limit");
        return Err(fail(format!("{service}响应超过大小限制")));
    }
    let mut body = Vec::new();
    response
        .take(max as u64 + 1)
        .read_to_end(&mut body)
        .map_err(|error| {
            log_body_failure(error);
            fail(format!("{service}响应读取失败，请稍后重试"))
        })?;
    if body.len() > max {
        tracing::warn!(
            limit_bytes = max,
            response_bytes = body.len(),
            "Account import response exceeds size limit"
        );
        return Err(fail(format!("{service}响应超过大小限制")));
    }
    Ok(body)
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::OnceLock;
    pub(crate) fn capture_logs<T>(action: impl FnOnce() -> T) -> (T, String) {
        use std::{
            io::Write,
            sync::{Arc, Mutex},
        };
        // Avoid tracing's single-dispatch shortcut caching disabled callsites on other test threads.
        static BASE_DISPATCH: OnceLock<tracing::Dispatch> = OnceLock::new();
        BASE_DISPATCH
            .get_or_init(|| tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default()));
        struct Writer(Arc<Mutex<Vec<u8>>>);
        impl Write for Writer {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().write(bytes)
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let output = bytes.clone();
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_max_level(tracing::Level::INFO)
            .with_writer(move || Writer(output.clone()))
            .finish();
        let result = tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!("account_import_test", import_id = "test-import");
            span.in_scope(action)
        });
        let logs = String::from_utf8(bytes.lock().unwrap().clone()).unwrap();
        (result, logs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn region_permits_reject_duplicates_allow_parallel_regions_and_release_across_threads() {
        let cn = ImportRegion::Cn.try_acquire().unwrap();
        let jp = ImportRegion::Jp.try_acquire().unwrap();
        for region in [ImportRegion::Cn, ImportRegion::Jp] {
            let error = std::thread::spawn(move || region.try_acquire().err().unwrap().to_string())
                .join()
                .unwrap();
            assert!(error.contains("已有账号正在读取"));
        }
        std::thread::spawn(move || drop(cn)).join().unwrap();
        let cn = ImportRegion::Cn.try_acquire().unwrap();
        assert!(ImportRegion::Jp.try_acquire().is_err());
        drop(jp);
        drop(cn);
        let wrong = ImportRegion::Cn.try_acquire().unwrap();
        assert!(admit_import(ImportRegion::Jp, Some(wrong)).is_err());
        assert!(ImportRegion::Cn.try_acquire().is_ok());
        assert!(ImportRegion::Jp.try_acquire().is_ok());
    }
}
