use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub const PACK_CAPABILITIES_FILENAME: &str = "engine-capabilities.json";
const REQUIRED_CAPABILITIES_JSON: &str =
    include_str!("../../scripts/pdf2zh-engine-capabilities.json");

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pdf2zhEngineCapabilities {
    pub schema_version: u32,
    pub engine_contract_version: u32,
    pub engine_revision: u32,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkerEngineCapabilities {
    contract_version: u32,
    engine_revision: u32,
    #[serde(default)]
    capabilities: Vec<String>,
}

const TRUSTED_LEGACY_ENGINE_VERSION: &str = "rosetta-pdf-engine-v2.1";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TrustedLegacyWorkerEngineCapabilities {
    engine_version: String,
    contract_version: u32,
    supports_prepare_window: bool,
    supports_unit_collection: bool,
    supports_page_rendering: bool,
    supports_single_page_artifacts: bool,
    timings_ms: HashMap<String, u64>,
}

pub fn required_engine_capabilities() -> Pdf2zhEngineCapabilities {
    serde_json::from_str(REQUIRED_CAPABILITIES_JSON)
        .expect("bundled PDF engine capability manifest must be valid")
}

pub fn read_pack_engine_capabilities(pack_dir: &Path) -> Result<Pdf2zhEngineCapabilities, String> {
    let path = pack_dir.join(PACK_CAPABILITIES_FILENAME);
    let contents = std::fs::read_to_string(&path)
        .map_err(|_| format!("组件缺少 {PACK_CAPABILITIES_FILENAME}，它可能是旧版组件"))?;
    let capabilities = serde_json::from_str::<Pdf2zhEngineCapabilities>(&contents)
        .map_err(|error| format!("组件能力清单无法解析: {error}"))?;
    validate_engine_capabilities(&capabilities)?;
    Ok(capabilities)
}

pub fn validate_engine_capabilities(actual: &Pdf2zhEngineCapabilities) -> Result<(), String> {
    let required = required_engine_capabilities();
    validate_claim(
        actual.schema_version,
        actual.engine_contract_version,
        actual.engine_revision,
        &actual.capabilities,
        &required,
    )
}

pub fn validate_installed_capabilities(
    schema_version: Option<u32>,
    contract_version: Option<u32>,
    engine_revision: Option<u32>,
    capabilities: &[String],
) -> Result<(), String> {
    let required = required_engine_capabilities();
    validate_claim(
        schema_version.unwrap_or_default(),
        contract_version.unwrap_or_default(),
        engine_revision.unwrap_or_default(),
        capabilities,
        &required,
    )
}

pub fn validate_worker_capabilities(
    value: &serde_json::Value,
    allow_trusted_legacy: bool,
) -> Result<(), String> {
    let required = required_engine_capabilities();
    match serde_json::from_value::<WorkerEngineCapabilities>(value.clone()) {
        Ok(actual) => validate_claim(
            required.schema_version,
            actual.contract_version,
            actual.engine_revision,
            &actual.capabilities,
            &required,
        ),
        Err(_) if allow_trusted_legacy => validate_trusted_legacy_worker(value, &required),
        Err(_) => Err("运行中的 PDF engine 未报告兼容能力".to_string()),
    }
}

fn validate_trusted_legacy_worker(
    value: &serde_json::Value,
    required: &Pdf2zhEngineCapabilities,
) -> Result<(), String> {
    let actual = serde_json::from_value::<TrustedLegacyWorkerEngineCapabilities>(value.clone())
        .map_err(|_| "运行中的 PDF engine 未报告兼容能力".to_string())?;
    if actual.engine_version != TRUSTED_LEGACY_ENGINE_VERSION {
        return Err(format!(
            "PDF engine 版本不兼容（需要 {TRUSTED_LEGACY_ENGINE_VERSION}，当前为 {}）",
            actual.engine_version
        ));
    }
    if actual.contract_version != required.engine_contract_version {
        return Err(format!(
            "PDF engine contract 不兼容（需要 {}，当前为 {}）",
            required.engine_contract_version, actual.contract_version
        ));
    }
    if !actual.supports_prepare_window
        || !actual.supports_unit_collection
        || !actual.supports_page_rendering
        || !actual.supports_single_page_artifacts
    {
        return Err("旧版 PDF engine 未报告完整的 v2 功能支持".to_string());
    }
    if actual.timings_ms.is_empty() {
        return Err("旧版 PDF engine 未报告预热计时".to_string());
    }
    Ok(())
}

fn validate_claim(
    schema_version: u32,
    contract_version: u32,
    engine_revision: u32,
    capabilities: &[String],
    required: &Pdf2zhEngineCapabilities,
) -> Result<(), String> {
    if schema_version != required.schema_version {
        return Err(format!(
            "组件能力清单版本不受支持（需要 {}，当前为 {schema_version}）",
            required.schema_version
        ));
    }
    if contract_version != required.engine_contract_version {
        return Err(format!(
            "PDF engine contract 不兼容（需要 {}，当前为 {contract_version}）",
            required.engine_contract_version
        ));
    }
    if engine_revision < required.engine_revision {
        return Err(format!(
            "PDF engine revision 过旧（需要至少 {}，当前为 {engine_revision}）",
            required.engine_revision
        ));
    }
    let missing = required
        .capabilities
        .iter()
        .filter(|capability| !capabilities.contains(capability))
        .cloned()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(format!("组件缺少必要能力: {}", missing.join(", ")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_component_without_revision_or_capabilities_fails_closed() {
        let error = validate_installed_capabilities(None, Some(2), None, &[])
            .expect_err("old component must require an upgrade");

        assert!(error.contains("能力清单版本"));
    }

    #[test]
    fn newer_revision_may_add_capabilities() {
        let required = required_engine_capabilities();
        let mut actual = required.clone();
        actual.engine_revision += 1;
        actual.capabilities.push("future-capability".to_string());

        assert!(validate_engine_capabilities(&actual).is_ok());
    }

    #[test]
    fn worker_handshake_uses_the_same_minimum_capabilities() {
        let required = required_engine_capabilities();
        let value = serde_json::json!({
            "contractVersion": required.engine_contract_version,
            "engineRevision": required.engine_revision,
            "capabilities": required.capabilities,
        });

        assert!(validate_worker_capabilities(&value, false).is_ok());
    }

    #[test]
    fn trusted_legacy_worker_requires_explicit_immutable_pack_authorization() {
        let value = serde_json::json!({
            "engineVersion": "rosetta-pdf-engine-v2.1",
            "contractVersion": 2,
            "supportsPrepareWindow": true,
            "supportsUnitCollection": true,
            "supportsPageRendering": true,
            "supportsSinglePageArtifacts": true,
            "timingsMs": { "total": 1 },
        });

        assert!(validate_worker_capabilities(&value, true).is_ok());
        assert!(validate_worker_capabilities(&value, false).is_err());
    }

    #[test]
    fn trusted_legacy_worker_rejects_partial_or_ambiguous_claims() {
        let partial = serde_json::json!({
            "engineVersion": "rosetta-pdf-engine-v2.1",
            "contractVersion": 2,
            "supportsPrepareWindow": true,
            "supportsUnitCollection": true,
            "supportsPageRendering": true,
            "supportsSinglePageArtifacts": false,
            "timingsMs": { "total": 1 },
        });
        let current_fields_with_invalid_revision = serde_json::json!({
            "engineVersion": "rosetta-pdf-engine-v2.1",
            "contractVersion": 2,
            "engineRevision": "invalid",
            "capabilities": [],
            "supportsPrepareWindow": true,
            "supportsUnitCollection": true,
            "supportsPageRendering": true,
            "supportsSinglePageArtifacts": true,
            "timingsMs": { "total": 1 },
        });

        assert!(validate_worker_capabilities(&partial, true).is_err());
        assert!(validate_worker_capabilities(&current_fields_with_invalid_revision, true).is_err());
    }
}
