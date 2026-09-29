use super::{
    aes_decrypt_iso10126_with, field_bytes_value, field_u64, load_card_episode_ids, parse_fields,
    suite_user_to_player_config, ImportError, PlayerConfig,
};
use crate::credentials::{bounded_body, fail};
use reqwest::{blocking::Client, header::HeaderMap};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, time::Duration};

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
        request.validate()?;
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .build()
            .map_err(|_| fail("无法初始化日服连接"))?;
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
        headers.insert(
            "x-signature",
            request
                .uuid
                .parse()
                .map_err(|_| fail("日服 UUID 格式异常"))?,
        );

        let application = read(&client, "/application", headers.clone(), 1024 * 1024)?;
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

        let suite = read(
            &client,
            &format!("/suite/user/{}", request.player_id),
            headers,
            32 * 1024 * 1024,
        )?;
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
        let player = suite_user_to_player_config(request.player_id, &suite, |card_id| {
            self.cards_dir
                .as_deref()
                .and_then(|dir| load_card_episode_ids(dir, card_id))
        })?;
        Ok(JpUuidImportResult {
            player,
            game_uid: request.player_id,
            name: text_field(&registration, 3).unwrap_or_default(),
            rank: field_u64(&data, 2).unwrap_or_default(),
        })
    }
}

fn read(
    client: &Client,
    endpoint: &str,
    headers: HeaderMap,
    limit: usize,
) -> Result<Vec<u8>, ImportError> {
    let response = client
        .get(format!("{BASE_URL}{endpoint}"))
        .headers(headers)
        .send()
        .map_err(|_| fail("连接日服超时或失败，请稍后重试"))?;
    if !response.status().is_success() {
        return Err(fail(format!(
            "日服接口返回 HTTP {}",
            response.status().as_u16()
        )));
    }
    aes_decrypt_iso10126_with(&bounded_body(response, limit)?, KEY, IV)
        .map_err(|_| fail("日服返回的数据无法解密，请检查游戏版本"))
}

fn text_field(fields: &[super::ProtoField], number: u64) -> Option<String> {
    std::str::from_utf8(field_bytes_value(fields, number)?)
        .ok()
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

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
