//! Provider 在线更新（方案 §9 第二阶段 / §26 / P4）。
//!
//! 流程：拉取 index（签名信封）-> 版本比较 -> 下载 -> SHA-256 校验
//!       -> 可选 ed25519 验签 -> 原子写入用户 providers 目录 -> 立即生效。
//!
//! 安全边界（方案 §25）：Manifest 只含 HTTP/JSON/映射数据，无代码执行；
//! 配置了公钥则强制验签（payload 必须为 serde_json 紧凑序列化字节）。

use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::{info, warn};

use super::manifest::{user_providers_dir, ProviderManifest};
use super::{echo_untrusted, is_safe_id};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoEntry {
    pub id: String,
    pub version: String,
    /// Manifest 文件的 SHA-256（hex，小写）
    pub sha256: String,
    /// 相对仓库根的文件名（如 "bing.json"）
    pub file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoPayload {
    pub providers: Vec<RepoEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedEnvelope {
    pub payload: RepoPayload,
    /// ed25519 签名（hex），签名对象为 payload 的 serde_json 紧凑序列化字节
    #[serde(default)]
    pub signature: Option<String>,
}

#[derive(Debug, Default)]
pub struct UpdateReport {
    /// (id, 本地版本, 新版本)
    pub updated: Vec<(String, String, String)>,
    pub checked: usize,
    pub errors: Vec<String>,
}

impl std::fmt::Display for UpdateReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.updated.is_empty() {
            write!(f, "checked {}", self.checked)?;
        } else {
            let items: Vec<String> = self
                .updated
                .iter()
                .map(|(id, old, new)| format!("{id}: {old} -> {new}"))
                .collect();
            write!(f, "{}", items.join("; "))?;
        }
        for err in &self.errors {
            write!(f, "; {err}")?;
        }
        Ok(())
    }
}

fn index_url(repo_base: &str) -> String {
    format!("{}/index.json", repo_base.trim_end_matches('/'))
}

fn file_url(repo_base: &str, file: &str) -> String {
    format!("{}/{}", repo_base.trim_end_matches('/'), file)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn hex_decode(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 {
        return None;
    }
    (0..text.len() / 2)
        .map(|i| u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).ok())
        .collect()
}

/// ed25519 验签（ring，复用 rustls 依赖树）；payload 序列化方式必须与服务端一致（serde_json 紧凑格式）
fn verify_signature(
    public_key_hex: &str,
    payload: &RepoPayload,
    signature_hex: &str,
) -> Result<(), String> {
    let key_bytes = hex_decode(public_key_hex).ok_or("公钥 hex 无效")?;
    let sig_bytes = hex_decode(signature_hex).ok_or("签名 hex 无效")?;
    let payload_bytes = serde_json::to_vec(payload).map_err(|e| e.to_string())?;
    let key = ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, key_bytes);
    key.verify(&payload_bytes, &sig_bytes)
        .map_err(|e| format!("签名验证失败: {e:?}"))
}

/// 读取本地 Manifest 版本（不存在返回 None）
fn local_version(dir: &Path, id: &str) -> Option<String> {
    let text = std::fs::read_to_string(dir.join(format!("{id}.json"))).ok()?;
    let manifest: ProviderManifest = serde_json::from_str(&text).ok()?;
    Some(manifest.version)
}

/// 检查并安装 Provider 更新。返回人类可读报告；部分失败不阻塞其余条目。
pub async fn check_for_updates(
    http: &reqwest::Client,
    repo_base: &str,
    public_key: Option<&str>,
    data_dir: &Path,
) -> Result<UpdateReport, String> {
    if repo_base.trim().is_empty() {
        return Err("未配置 Provider 更新源".into());
    }

    let envelope: SignedEnvelope = http
        .get(index_url(repo_base))
        .send()
        .await
        .map_err(|e| format!("拉取索引失败: {e}"))?
        .error_for_status()
        .map_err(|e| format!("索引响应错误: {e}"))?
        .json()
        .await
        .map_err(|e| format!("索引解析失败: {e}"))?;

    // 验签策略：配置了公钥 -> 强制验签；未配置 -> 仅 SHA-256 完整性（并提示）
    if let Some(key) = public_key.filter(|k| !k.trim().is_empty()) {
        let signature = envelope
            .signature
            .as_deref()
            .ok_or("仓库要求验签，但索引缺少签名")?;
        verify_signature(key, &envelope.payload, signature)?;
        info!("Provider 索引签名验证通过");
    } else if envelope.signature.is_some() {
        warn!("索引带签名但未配置公钥，本次仅做 SHA-256 校验；建议在设置中配置公钥");
    }

    let dir = user_providers_dir(data_dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建 providers 目录失败: {e}"))?;

    let mut report = UpdateReport {
        checked: envelope.payload.providers.len(),
        ..Default::default()
    };

    // 逐条：版本比较 -> 下载 -> SHA-256 校验 -> 原子安装
    let mut updated = Vec::new();
    for entry in &envelope.payload.providers {
        // id 直接拼入本地安装路径（<id>.json），必须先过白名单再碰文件系统
        if !is_safe_id(&entry.id) {
            report.errors.push(format!(
                "条目 id 非法，已拒绝安装: {}",
                echo_untrusted(&entry.id)
            ));
            continue;
        }
        let current = local_version(&dir, &entry.id);
        if let Some(local) = &current {
            if *local >= entry.version {
                continue;
            }
        }
        let downloaded = http
            .get(file_url(repo_base, &entry.file))
            .send()
            .await
            .and_then(|resp| resp.error_for_status())
            .map_err(|e| format!("{}: 下载失败: {e}", entry.id))?
            .bytes()
            .await
            .map_err(|e| format!("{}: 读取失败: {e}", entry.id))?;

        let actual = sha256_hex(&downloaded);
        if actual != entry.sha256.to_lowercase() {
            report.errors.push(format!(
                "{}: SHA-256 不匹配（预期 {}，实际 {actual}）",
                entry.id, entry.sha256
            ));
            continue;
        }
        let dest = dir.join(format!("{}.json", entry.id));
        let tmp = dir.join(format!("{}.json.part", entry.id));
        if let Err(e) = std::fs::write(&tmp, &downloaded) {
            report.errors.push(format!("{}: 写入失败: {e}", entry.id));
            continue;
        }
        if let Err(e) = std::fs::rename(&tmp, &dest) {
            report.errors.push(format!("{}: 安装失败: {e}", entry.id));
            continue;
        }
        info!("Provider 已更新: {} -> {}", entry.id, entry.version);
        updated.push((
            entry.id.clone(),
            current.unwrap_or_else(|| "-".into()),
            entry.version.clone(),
        ));
    }
    report.updated = updated;
    Ok(report)
}
