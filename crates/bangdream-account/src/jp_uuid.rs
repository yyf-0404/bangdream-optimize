use super::{
    aes_decrypt_iso10126_with, field_bytes_value, field_u64, load_card_episode_ids, parse_fields,
    suite_user_to_player_config, ImportError, PlayerConfig,
};
use crate::import_support::{
    admit_import, bounded_body, fail, import_error_summary, import_stage, log_transport_failure,
    logged_endpoint, ImportPermit, ImportRegion,
};
use reqwest::{blocking::Client, header::HeaderMap};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

const BASE_URL: &str = "https://api.garupa.jp/api";
const CLIENT_VERSION: &str = "10.2.0";
const DATA_VERSION: &str = "10.2.0.130";
const UNITY_VERSION: &str = "2022.3.62f1";
const KEY: &[u8; 16] = b"mikumikulukaluka";
const IV: &[u8; 16] = b"lukalukamikumiku";

// UUID is a session credential. Keep it out of serialized results and diagnostics.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JpUuidImportRequest {
    pub player_id: u64,
    pub uuid: String,
}

impl JpUuidImportRequest {
    pub fn validate(&self) -> Result<(), ImportError> {
        if self.player_id == 0 || self.player_id > i64::MAX as u64 {
            return Err(fail("请输入有效的日服玩家 ID"));
        }
        let bytes = self.uuid.as_bytes();
        if bytes.len() != 36
            || bytes.iter().enumerate().any(|(i, b)| {
                if matches!(i, 8 | 13 | 18 | 23) {
                    *b != b'-'
                } else {
                    !b.is_ascii_hexdigit()
                }
            })
        {
            return Err(fail("请输入有效的日服 UUID"));
        }
        Ok(())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JpUuidImportResult {
    pub player: PlayerConfig,
    pub game_uid: u64,
    pub name: String,
    pub rank: u64,
}

pub struct JpUuidImporter {
    cards_dir: Option<PathBuf>,
}

impl JpUuidImporter {
    pub fn new(cards_dir: Option<PathBuf>) -> Self {
        Self { cards_dir }
    }

    pub fn import(&self, request: JpUuidImportRequest) -> Result<JpUuidImportResult, ImportError> {
        self.import_inner(request, None)
    }

    /// Use a JP permit reserved before dispatching a blocking task.
    pub fn import_with_permit(
        &self,
        request: JpUuidImportRequest,
        permit: ImportPermit,
    ) -> Result<JpUuidImportResult, ImportError> {
        self.import_inner(request, Some(permit))
    }

    #[tracing::instrument(name = "account_import_worker", skip_all, fields(server = "jp"))]
    fn import_inner(
        &self,
        request: JpUuidImportRequest,
        permit: Option<ImportPermit>,
    ) -> Result<JpUuidImportResult, ImportError> {
        import_stage("request_validation", || request.validate())?;
        let _permit = admit_import(ImportRegion::Jp, permit)?;
        let client = import_stage("http_client_init", || {
            Client::builder()
                .timeout(Duration::from_secs(30))
                .redirect(reqwest::redirect::Policy::none())
                .retry(reqwest::retry::never())
                .build()
                .map_err(|error| {
                    log_transport_failure("http_client_init", error);
                    fail("无法初始化日服连接")
                })
        })?;
        import_with_transport(
            |endpoint, headers, limit| read(&client, endpoint, headers, limit),
            request,
            |card_id| {
                self.cards_dir
                    .as_deref()
                    .and_then(|dir| load_card_episode_ids(dir, card_id))
            },
        )
    }
}

fn import_with_transport(
    mut read: impl FnMut(&str, HeaderMap, usize) -> Result<Vec<u8>, ImportError>,
    request: JpUuidImportRequest,
    episodes: impl FnMut(u64) -> Option<[u64; 2]>,
) -> Result<JpUuidImportResult, ImportError> {
    let mut headers = import_stage("request_headers", || {
        let mut headers = HeaderMap::new();
        for (key, value) in [
            (
                "user-agent",
                "UnityPlayer/2022.3.62f1 (UnityWebRequest/1.0, libcurl/8.10.1-DEV)",
            ),
            ("accept", "application/octet-stream"),
            ("content-type", "application/octet-stream"),
            ("x-clientversion", CLIENT_VERSION),
            ("x-dataversion", DATA_VERSION),
            ("x-masterdataversion", DATA_VERSION),
            ("x-clientplatform", "Android"),
            ("x-unity-version", UNITY_VERSION),
        ] {
            headers.insert(key, value.parse().map_err(|_| fail("日服请求头格式异常"))?);
        }
        let mut signature = request
            .uuid
            .parse::<reqwest::header::HeaderValue>()
            .map_err(|_| fail("日服 UUID 格式异常"))?;
        signature.set_sensitive(true);
        headers.insert("x-signature", signature);
        Ok(headers)
    })?;
    import_stage("application_preflight", || {
        let application = read("/application", headers.clone(), 1024 * 1024)?;
        let application = parse_fields(&application)?;
        let version = text_field(&application, 1).ok_or_else(|| fail("日服版本信息缺失"))?;
        if version != CLIENT_VERSION {
            return Err(fail("日服客户端版本已更新，暂时无法读取"));
        }
        let data = text_field(&application, 2).ok_or_else(|| fail("日服数据版本缺失"))?;
        let master = text_field(&application, 10).ok_or_else(|| fail("日服主数据版本缺失"))?;
        headers.insert(
            "x-dataversion",
            data.parse().map_err(|_| fail("日服数据版本格式异常"))?,
        );
        headers.insert(
            "x-masterdataversion",
            master.parse().map_err(|_| fail("日服主数据版本格式异常"))?,
        );
        tracing::info!(client_version = %version, data_version_bytes = data.len(), master_version_bytes = master.len(),
            "JP import application versions accepted");
        Ok(())
    })?;
    let suite = import_stage("suite_fetch", || {
        read(
            &format!("/suite/user/{}", request.player_id),
            headers,
            32 * 1024 * 1024,
        )
    })?;
    let (name, rank) = import_stage("suite_identity", || {
        let root = parse_fields(&suite)?;
        let user =
            parse_fields(field_bytes_value(&root, 1).ok_or_else(|| fail("日服没有返回用户资料"))?)?;
        let registration =
            parse_fields(field_bytes_value(&user, 1).ok_or_else(|| fail("日服没有返回用户身份"))?)?;
        let data =
            parse_fields(field_bytes_value(&user, 2).ok_or_else(|| fail("日服没有返回用户等级"))?)?;
        if field_u64(&registration, 1) != Some(request.player_id)
            || field_u64(&data, 1) != Some(request.player_id)
        {
            return Err(fail("日服返回的玩家 ID 与输入不一致，已停止导入"));
        }
        Ok((
            text_field(&registration, 3).unwrap_or_default(),
            field_u64(&data, 2).unwrap_or_default(),
        ))
    })?;
    let player = import_stage("profile_parse", || {
        suite_user_to_player_config(request.player_id, &suite, episodes)
    })?;
    tracing::info!(
        cards = player.card_list.len(),
        area_items = player.area_item.len(),
        characters = player.character_bouns.len(),
        "JP account profile parsed"
    );
    Ok(JpUuidImportResult {
        player,
        game_uid: request.player_id,
        name,
        rank,
    })
}

fn read(
    client: &Client,
    endpoint: &str,
    headers: HeaderMap,
    limit: usize,
) -> Result<Vec<u8>, ImportError> {
    let span = tracing::info_span!(
        "account_import_http",
        api = "game",
        endpoint = logged_endpoint(endpoint),
        method = "GET"
    );
    let _entered = span.enter();
    let started = Instant::now();
    tracing::info!(outcome = "started", "JP import upstream request");
    let response = client
        .get(format!("{BASE_URL}{endpoint}"))
        .headers(headers)
        .send()
        .map_err(|error| {
            log_transport_failure(endpoint, error);
            fail("连接日服超时或失败，请稍后重试")
        })?;
    let status = response.status().as_u16();
    if !response.status().is_success() {
        tracing::warn!(
            status,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "JP import HTTP request rejected"
        );
        return Err(fail(format!("日服接口返回 HTTP {status}")));
    }
    let encrypted = bounded_body(response, limit, "日服")?;
    let plain = aes_decrypt_iso10126_with(&encrypted, KEY, IV).map_err(|error| {
        tracing::warn!(error = %import_error_summary(&error), encrypted_bytes = encrypted.len(), "JP import response decrypt failed");
        fail("日服返回的数据无法解密，请检查游戏版本")
    })?;
    tracing::info!(
        outcome = "completed",
        status,
        elapsed_ms = started.elapsed().as_millis() as u64,
        response_bytes = encrypted.len(),
        "JP import upstream request"
    );
    Ok(plain)
}

fn text_field(fields: &[super::ProtoField], number: u64) -> Option<String> {
    std::str::from_utf8(field_bytes_value(fields, number)?)
        .ok()
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{field_bytes, field_string, import_support::test_support::capture_logs};

    const FIXTURE_UUID: &str = "00000000-0000-4000-8000-000000000001";

    fn integer(out: &mut Vec<u8>, field: u64, value: u64) {
        for mut value in [field << 3, value] {
            while value >= 128 {
                out.push((value as u8 & 127) | 128);
                value >>= 7;
            }
            out.push(value as u8);
        }
    }

    fn fixture(failure: &str) -> (Result<JpUuidImportResult, ImportError>, String, Vec<String>) {
        let mut calls = Vec::new();
        let (result, logs) = capture_logs(|| {
            import_with_transport(
                |endpoint, headers, _| {
                    calls.push(endpoint.to_owned());
                    assert_eq!(headers["x-signature"], FIXTURE_UUID);
                    assert!(headers["x-signature"].is_sensitive());
                    let mut result = Vec::new();
                    if endpoint == "/application" {
                        field_string(
                            &mut result,
                            1,
                            if failure == "application_preflight" {
                                "0.0.0"
                            } else {
                                CLIENT_VERSION
                            },
                        );
                        field_string(&mut result, 2, "10.2.0.180");
                        field_string(&mut result, 10, "10.2.0.180");
                    } else {
                        assert_eq!(endpoint, "/suite/user/42");
                        assert_eq!(headers["x-dataversion"], "10.2.0.180");
                        assert_eq!(headers["x-masterdataversion"], "10.2.0.180");
                        if failure == "suite_fetch" {
                            return Err(fail("fixture suite unavailable"));
                        }
                        let mut registration = Vec::new();
                        integer(
                            &mut registration,
                            1,
                            if failure == "suite_identity" { 43 } else { 42 },
                        );
                        field_string(&mut registration, 3, "JP fixture player");
                        let mut data = Vec::new();
                        integer(&mut data, 1, 42);
                        integer(&mut data, 2, 7);
                        let mut user = Vec::new();
                        field_bytes(&mut user, 1, &registration);
                        field_bytes(&mut user, 2, &data);
                        field_bytes(&mut result, 1, &user);
                        let mut situation = Vec::new();
                        integer(&mut situation, 1, 42);
                        integer(&mut situation, 2, 3);
                        integer(&mut situation, 3, 40);
                        field_string(&mut situation, 7, "done");
                        field_string(&mut situation, 9, "after_training");
                        integer(&mut situation, 11, 2);
                        integer(&mut situation, 13, 1);
                        let mut entry = Vec::new();
                        integer(&mut entry, 1, 3);
                        field_bytes(&mut entry, 2, &situation);
                        let mut situations = Vec::new();
                        field_bytes(&mut situations, 1, &entry);
                        field_bytes(&mut result, 3, &situations);
                        if failure == "profile_parse" {
                            integer(&mut result, 3, 1);
                        }
                    }
                    Ok(result)
                },
                JpUuidImportRequest {
                    player_id: 42,
                    uuid: FIXTURE_UUID.into(),
                },
                |_| None,
            )
        });
        for secret in [FIXTURE_UUID, "JP fixture player", "/suite/user/42"] {
            assert!(!logs.contains(secret), "{logs}");
        }
        (result, logs, calls)
    }

    #[test]
    fn imports_own_profile_once_and_logs_only_stages_and_counts() {
        let (result, logs, calls) = fixture("");
        let result = result.unwrap();
        assert_eq!(result.game_uid, 42);
        assert_eq!(result.player.player_id, 42);
        assert_eq!(result.name, "JP fixture player");
        assert_eq!(result.rank, 7);
        let card = &result.player.card_list["3"];
        assert_eq!(card.level, 40);
        assert!(card.training);
        assert_eq!(card.skill_level, 2);
        assert_eq!(card.limit_break_rank, 1);
        assert_eq!(calls, ["/application", "/suite/user/42"]);
        assert!(!serde_json::to_string(&result)
            .unwrap()
            .contains(FIXTURE_UUID));
        for stage in [
            "request_headers",
            "application_preflight",
            "suite_fetch",
            "suite_identity",
            "profile_parse",
        ] {
            assert!(logs.contains(stage), "{logs}");
        }
        assert!(logs.contains("cards=1"), "{logs}");
        assert!(logs.contains("elapsed_ms="), "{logs}");
        assert!(logs.contains("import_id=\"test-import\""), "{logs}");
    }

    #[test]
    fn failures_record_exact_stage_and_stop_without_retry_or_uuid_leaks() {
        for stage in [
            "application_preflight",
            "suite_fetch",
            "suite_identity",
            "profile_parse",
        ] {
            let (result, logs, calls) = fixture(stage);
            assert!(result.is_err());
            let failure = logs
                .lines()
                .find(|line| line.contains("outcome=\"failed\""))
                .unwrap();
            assert!(failure.contains(stage), "{logs}");
            assert!(failure.contains("elapsed_ms="), "{logs}");
            assert!(failure.contains("import_id=\"test-import\""), "{logs}");
            assert_eq!(
                calls.len(),
                if stage == "application_preflight" {
                    1
                } else {
                    2
                }
            );
        }
    }

    #[test]
    fn requires_valid_player_id_and_uuid() {
        let mut request = JpUuidImportRequest {
            player_id: 42,
            uuid: "00000000-0000-4000-8000-000000000001".into(),
        };
        assert!(request.validate().is_ok());
        request.player_id = 0;
        assert!(request.validate().is_err());
        request.player_id = 42;
        request.uuid = "not-a-uuid".into();
        assert!(request.validate().is_err());
    }
}
