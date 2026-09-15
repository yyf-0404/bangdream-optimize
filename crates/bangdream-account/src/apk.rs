//! Read only the current official APK's manifest and Unity header using HTTP Range.
use crate::{
    credentials::{bounded_body, fail},
    ImportError,
};
use reqwest::{blocking::Client, Url};
use serde_json::Value;
use std::io::{self, Read, Seek, SeekFrom};
use zip::ZipArchive;

const CONFIG_URL: &str = "https://static.biligame.com/config/bangdream.config.js";
const MAX_MANIFEST: usize = 2 * 1024 * 1024;
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ClientVersion {
    #[serde(rename = "clientVersion")]
    pub client: String,
    #[serde(rename = "versionCode")]
    pub code: String,
    #[serde(rename = "unityVersion")]
    pub unity: String,
}
impl ClientVersion {
    pub(crate) fn validate(&self) -> Result<(), ImportError> {
        if !dotted_version(&self.client)
            || self.code.is_empty()
            || self.code.len() >= 16
            || !self.code.bytes().all(|b| b.is_ascii_digit())
            || self.unity.len() < 8
            || self.unity.len() > 64
            || !self.unity.starts_with("20")
            || !self
                .unity
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.')
        {
            return Err(fail("国服客户端版本配置格式异常"));
        }
        Ok(())
    }
}

pub(crate) fn dotted_version(s: &str) -> bool {
    let parts: Vec<_> = s.split('.').collect();
    parts.len() >= 3
        && parts.len() <= 8
        && s.len() < 64
        && parts
            .iter()
            .all(|s| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()))
}
fn allowed_apk(url: &Url) -> bool {
    url.scheme() == "https"
        && matches!(
            url.host_str(),
            Some("pkg.biligame.com" | "pkgdl.biligame.net")
        )
        && url.path().ends_with(".apk")
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
}
fn download(client: &Client, url: &str) -> reqwest::blocking::RequestBuilder {
    client.get(url).header("User-Agent","Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36")
        .header("Referer","https://game.bilibili.com/").header("Accept-Encoding","identity")
}
pub(crate) fn discover(client: &Client) -> Result<ClientVersion, ImportError> {
    let response = download(client, CONFIG_URL)
        .header("Cache-Control", "no-cache")
        .send()
        .map_err(|_| fail("无法读取国服官方版本信息，请稍后重试"))?;
    if !response.status().is_success() {
        return Err(fail("国服官方版本配置暂不可用"));
    }
    let data = bounded_body(response, 512 * 1024)?;
    let text = std::str::from_utf8(&data).map_err(|_| fail("国服官方配置格式异常"))?;
    let at = text
        .find("$mngConfig")
        .ok_or_else(|| fail("国服官方配置缺少版本入口"))?;
    let at = at
        + text[at..]
            .find('=')
            .ok_or_else(|| fail("国服官方配置格式异常"))?
        + 1;
    let config: Value = serde_json::Deserializer::from_str(&text[at..])
        .into_iter::<Value>()
        .next()
        .ok_or_else(|| fail("国服官方配置为空"))?
        .map_err(|_| fail("国服官方配置格式异常"))?;
    let mut urls = Vec::new();
    for section in ["pc", "h5"] {
        for key in ["android_link", "android_link2"] {
            if let Some(url) = config[section][key]
                .as_str()
                .and_then(|s| Url::parse(s).ok())
                .filter(allowed_apk)
            {
                if !urls.contains(&url) {
                    urls.push(url);
                }
            }
        }
    }
    for url in urls {
        match read_remote(client, url.as_str()) {
            Ok(version) => return Ok(version),
            Err(error) => {
                #[cfg(test)]
                eprintln!("Public APK {}: {error}", url.host_str().unwrap_or_default());
                let _ = error;
            }
        }
    }
    Err(fail(
        "无法从官方安装包读取当前版本，请稍后重试；未使用过期版本登录",
    ))
}

struct RemoteApk {
    client: Client,
    url: String,
    len: u64,
    pos: u64,
    start: u64,
    cache: Vec<u8>,
}
impl RemoteApk {
    fn new(client: &Client, url: &str) -> Result<Self, ImportError> {
        let response = download(client, url)
            .header("Range", "bytes=-65557")
            .send()
            .map_err(|_| fail("官方安装包连接失败"))?;
        let (start, end, len) = range_info(&response)?;
        if len == 0 || len > 1024 * 1024 * 1024 || end + 1 != len || end - start + 1 > 65557 {
            return Err(fail("官方安装包长度异常"));
        }
        let cache = bounded_body(response, 65557)?;
        if cache.len() as u64 != end - start + 1 {
            return Err(fail("官方安装包分段不完整"));
        }
        Ok(Self {
            client: client.clone(),
            url: url.into(),
            len,
            pos: 0,
            start,
            cache,
        })
    }
    fn fill(&mut self) -> Result<(), ImportError> {
        let end = (self.pos + 65535).min(self.len - 1);
        let response = download(&self.client, &self.url)
            .header("Range", format!("bytes={}-{end}", self.pos))
            .send()
            .map_err(|_| fail("官方安装包分段读取失败"))?;
        let (start, actual_end, len) = range_info(&response)?;
        if start != self.pos || end != actual_end || len != self.len {
            return Err(fail("官方安装包分段发生变化"));
        }
        let cache = bounded_body(response, 65536)?;
        if cache.len() as u64 != end - start + 1 {
            return Err(fail("官方安装包分段不完整"));
        }
        self.start = start;
        self.cache = cache;
        Ok(())
    }
}
fn range_info(response: &reqwest::blocking::Response) -> Result<(u64, u64, u64), ImportError> {
    if response.status().as_u16() != 206 {
        return Err(fail(format!(
            "官方安装包分段读取失败（HTTP {}）",
            response.status().as_u16()
        )));
    }
    let raw = response
        .headers()
        .get("content-range")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("bytes "))
        .ok_or_else(|| fail("安装包缺少分段信息"))?;
    let (range, total) = raw
        .split_once('/')
        .ok_or_else(|| fail("安装包分段格式异常"))?;
    let (start, end) = range
        .split_once('-')
        .ok_or_else(|| fail("安装包分段格式异常"))?;
    let parse = |s: &str| s.parse::<u64>().map_err(|_| fail("安装包分段格式异常"));
    let (start, end, len) = (parse(start)?, parse(end)?, parse(total)?);
    if start > end || end >= len {
        return Err(fail("安装包分段超出范围"));
    }
    Ok((start, end, len))
}
impl Read for RemoteApk {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() || self.pos >= self.len {
            return Ok(0);
        }
        if self.pos < self.start || self.pos >= self.start + self.cache.len() as u64 {
            self.fill()
                .map_err(|_| io::Error::other("APK range unavailable"))?;
        }
        let offset = (self.pos - self.start) as usize;
        let count = buf.len().min(self.cache.len() - offset);
        buf[..count].copy_from_slice(&self.cache[offset..offset + count]);
        self.pos += count as u64;
        Ok(count)
    }
}
impl Seek for RemoteApk {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let position = match from {
            SeekFrom::Start(n) => n as i128,
            SeekFrom::End(n) => self.len as i128 + n as i128,
            SeekFrom::Current(n) => self.pos as i128 + n as i128,
        };
        if position < 0 || position > self.len as i128 {
            return Err(io::Error::other("APK seek outside file"));
        }
        self.pos = position as u64;
        Ok(self.pos)
    }
}
fn read_remote(client: &Client, url: &str) -> Result<ClientVersion, ImportError> {
    let remote = RemoteApk::new(client, url)?;
    let mut zip = ZipArchive::new(remote).map_err(|_| fail("官方安装包目录读取失败"))?;
    if zip.len() > 100000 {
        return Err(fail("官方安装包目录异常"));
    }
    let manifest = {
        let file = zip
            .by_name("AndroidManifest.xml")
            .map_err(|_| fail("官方安装包缺少清单"))?;
        if file.size() > MAX_MANIFEST as u64 {
            return Err(fail("安装包清单过大"));
        }
        let mut bytes = Vec::new();
        file.take(MAX_MANIFEST as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| fail("安装包清单读取失败"))?;
        if bytes.len() > MAX_MANIFEST {
            return Err(fail("安装包清单过大"));
        }
        bytes
    };
    let (client, code) = manifest_version(&manifest)?;
    let mut header = [0u8; 128];
    let mut file = zip
        .by_name("assets/bin/Data/globalgamemanagers")
        .map_err(|_| fail("官方安装包缺少引擎元数据"))?;
    file.read_exact(&mut header)
        .map_err(|_| fail("引擎元数据读取失败"))?;
    let fmt = u32::from_be_bytes(header[8..12].try_into().unwrap());
    if !(9..=22).contains(&fmt) {
        return Err(fail("不支持的引擎元数据格式"));
    }
    let at = if fmt >= 22 { 48 } else { 20 };
    let unity = std::str::from_utf8(header[at..].split(|b| *b == 0).next().unwrap())
        .map_err(|_| fail("引擎版本编码错误"))?;
    if unity.len() < 8
        || unity.len() > 64
        || !unity
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.')
        || !unity.starts_with("20")
    {
        return Err(fail("引擎版本格式异常"));
    }
    Ok(ClientVersion {
        client,
        code,
        unity: unity.into(),
    })
}

fn u16at(b: &[u8], at: usize) -> Result<u16, ImportError> {
    Ok(u16::from_le_bytes(
        b.get(at..at + 2)
            .ok_or_else(|| fail("安装包清单截断"))?
            .try_into()
            .unwrap(),
    ))
}
fn u32at(b: &[u8], at: usize) -> Result<u32, ImportError> {
    Ok(u32::from_le_bytes(
        b.get(at..at + 4)
            .ok_or_else(|| fail("安装包清单截断"))?
            .try_into()
            .unwrap(),
    ))
}
fn string_at(pool: &[String], index: u32) -> Result<&str, ImportError> {
    pool.get(index as usize)
        .map(String::as_str)
        .ok_or_else(|| fail("安装包清单字符串索引异常"))
}
fn len8(b: &[u8], at: &mut usize) -> Result<usize, ImportError> {
    let x = *b.get(*at).ok_or_else(|| fail("清单字符串截断"))?;
    *at += 1;
    if x & 128 == 0 {
        Ok(x as usize)
    } else {
        let y = *b.get(*at).ok_or_else(|| fail("清单字符串截断"))?;
        *at += 1;
        Ok(((x as usize & 127) << 8) | y as usize)
    }
}
fn len16(b: &[u8], at: &mut usize) -> Result<usize, ImportError> {
    let x = u16at(b, *at)?;
    *at += 2;
    if x & 0x8000 == 0 {
        Ok(x as usize)
    } else {
        let y = u16at(b, *at)?;
        *at += 2;
        Ok(((x as usize & 0x7fff) << 16) | y as usize)
    }
}
fn strings(chunk: &[u8]) -> Result<Vec<String>, ImportError> {
    let count = u32at(chunk, 8)? as usize;
    let head = u16at(chunk, 2)? as usize;
    let start = u32at(chunk, 20)? as usize;
    let utf8 = u32at(chunk, 16)? & 0x100 != 0;
    if head < 28 || count > 20000 || head + count * 4 > chunk.len() || start > chunk.len() {
        return Err(fail("清单字符串池异常"));
    }
    let mut result = Vec::with_capacity(count);
    let mut total = 0;
    for n in 0..count {
        let mut at = start
            .checked_add(u32at(chunk, head + n * 4)? as usize)
            .ok_or_else(|| fail("清单字符串偏移溢出"))?;
        let value = if utf8 {
            len8(chunk, &mut at)?;
            let len = len8(chunk, &mut at)?;
            let bytes = chunk
                .get(at..at + len)
                .ok_or_else(|| fail("清单字符串截断"))?;
            std::str::from_utf8(bytes)
                .map_err(|_| fail("清单字符串编码错误"))?
                .to_owned()
        } else {
            let len = len16(chunk, &mut at)?;
            if len > 32768 {
                return Err(fail("清单字符串过长"));
            }
            let units = (0..len)
                .map(|n| u16at(chunk, at + n * 2))
                .collect::<Result<Vec<_>, _>>()?;
            String::from_utf16(&units).map_err(|_| fail("清单字符串编码错误"))?
        };
        total += value.len();
        if total > 8 * 1024 * 1024 {
            return Err(fail("清单字符串池过大"));
        }
        result.push(value);
    }
    Ok(result)
}
fn manifest_version(bytes: &[u8]) -> Result<(String, String), ImportError> {
    if bytes.len() > MAX_MANIFEST || u16at(bytes, 0)? != 3 {
        return Err(fail("安装包清单格式异常"));
    }
    let total = u32at(bytes, 4)? as usize;
    let mut at = u16at(bytes, 2)? as usize;
    let mut pool = Vec::new();
    if total > bytes.len() || at < 8 {
        return Err(fail("安装包清单截断"));
    }
    while at < total {
        let kind = u16at(bytes, at)?;
        let size = u32at(bytes, at + 4)? as usize;
        if size < 8 || size > total - at {
            return Err(fail("安装包清单块异常"));
        }
        let chunk = &bytes[at..at + size];
        if kind == 1 {
            pool = strings(chunk)?;
        }
        if kind == 0x102 && string_at(&pool, u32at(chunk, 20)?)? == "manifest" {
            let start = 16 + u16at(chunk, 24)? as usize;
            let stride = u16at(chunk, 26)? as usize;
            let count = u16at(chunk, 28)? as usize;
            if stride < 20 || start + count * stride > chunk.len() {
                return Err(fail("安装包清单属性异常"));
            }
            let mut package = None;
            let mut version = None;
            let mut code = None;
            for n in 0..count {
                let pos = start + n * stride;
                let name = string_at(&pool, u32at(chunk, pos + 4)?)?;
                let raw = u32at(chunk, pos + 8)?;
                let namespace = u32at(chunk, pos)?;
                let android = namespace != u32::MAX
                    && string_at(&pool, namespace)? == "http://schemas.android.com/apk/res/android";
                let data = u32at(chunk, pos + 16)?;
                let value = if raw != u32::MAX {
                    string_at(&pool, raw)?.to_owned()
                } else if chunk[pos + 15] == 3 {
                    string_at(&pool, data)?.to_owned()
                } else {
                    data.to_string()
                };
                match name {
                    "package" if namespace == u32::MAX => package = Some(value),
                    "versionName" if android => version = Some(value),
                    "versionCode" if android => code = Some(value),
                    _ => {}
                }
            }
            if package.as_deref() != Some("com.bilibili.star.bili") {
                return Err(fail("官方安装包包名不匹配"));
            }
            let version = version
                .filter(|s| dotted_version(s))
                .ok_or_else(|| fail("安装包客户端版本异常"))?;
            let code = code
                .filter(|s| !s.is_empty() && s.len() < 16 && s.bytes().all(|c| c.is_ascii_digit()))
                .ok_or_else(|| fail("安装包版本编号异常"))?;
            return Ok((version, code));
        }
        at += size;
    }
    Err(fail("安装包没有提供版本信息"))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn u16p(b: &mut Vec<u8>, n: u16) {
        b.extend(n.to_le_bytes());
    }
    fn u32p(b: &mut Vec<u8>, n: u32) {
        b.extend(n.to_le_bytes());
    }
    fn fixture(utf8: bool) -> Vec<u8> {
        let words = [
            "manifest",
            "package",
            "versionName",
            "versionCode",
            "http://schemas.android.com/apk/res/android",
            "com.bilibili.star.bili",
            "9.4.4",
        ];
        let mut content = Vec::new();
        let mut offsets = Vec::new();
        for word in words {
            offsets.push(content.len() as u32);
            if utf8 {
                content.push(word.len() as u8);
                content.push(word.len() as u8);
                content.extend(word.as_bytes());
                content.push(0);
            } else {
                u16p(&mut content, word.len() as u16);
                for c in word.encode_utf16() {
                    u16p(&mut content, c);
                }
                u16p(&mut content, 0);
            }
        }
        let start = 28 + words.len() * 4;
        let mut pool = Vec::new();
        u16p(&mut pool, 1);
        u16p(&mut pool, 28);
        u32p(&mut pool, (start + content.len()) as u32);
        u32p(&mut pool, words.len() as u32);
        u32p(&mut pool, 0);
        u32p(&mut pool, if utf8 { 0x100 } else { 0 });
        u32p(&mut pool, start as u32);
        u32p(&mut pool, 0);
        for n in offsets {
            u32p(&mut pool, n);
        }
        pool.extend(content);
        let mut node = Vec::new();
        u16p(&mut node, 0x102);
        u16p(&mut node, 16);
        u32p(&mut node, 96);
        u32p(&mut node, 1);
        u32p(&mut node, u32::MAX);
        u32p(&mut node, u32::MAX);
        u32p(&mut node, 0);
        u16p(&mut node, 20);
        u16p(&mut node, 20);
        u16p(&mut node, 3);
        u16p(&mut node, 0);
        u16p(&mut node, 0);
        u16p(&mut node, 0);
        for (ns, name, raw, kind, data) in [
            (u32::MAX, 1, 5, 3, 5),
            (4, 2, u32::MAX, 3, 6),
            (4, 3, u32::MAX, 16, 940400),
        ] {
            u32p(&mut node, ns);
            u32p(&mut node, name);
            u32p(&mut node, raw);
            u16p(&mut node, 8);
            node.push(0);
            node.push(kind);
            u32p(&mut node, data);
        }
        let mut xml = Vec::new();
        u16p(&mut xml, 3);
        u16p(&mut xml, 8);
        u32p(&mut xml, (8 + pool.len() + node.len()) as u32);
        xml.extend(pool);
        xml.extend(node);
        xml
    }
    #[test]
    fn manifest_reads_raw_and_typed_attributes_in_both_string_encodings() {
        for utf8 in [true, false] {
            assert_eq!(
                manifest_version(&fixture(utf8)).unwrap(),
                ("9.4.4".into(), "940400".into())
            );
        }
    }
    #[test]
    fn truncated_manifest_is_rejected_without_panics() {
        let xml = fixture(true);
        for end in 0..xml.len() {
            assert!(manifest_version(&xml[..end]).is_err());
        }
    }
    #[test]
    fn apk_download_accepts_only_official_https_hosts() {
        for url in [
            "https://pkg.biligame.com/games/a.apk",
            "https://pkgdl.biligame.net/a.apk",
        ] {
            assert!(allowed_apk(&Url::parse(url).unwrap()));
        }
        for url in [
            "http://pkg.biligame.com/a.apk",
            "https://evil.test/a.apk",
            "https://pkg.biligame.com.evil.test/a.apk",
            "https://user@pkg.biligame.com/a.apk",
            "https://pkg.biligame.com/a.zip",
        ] {
            assert!(!allowed_apk(&Url::parse(url).unwrap()));
        }
    }
}
