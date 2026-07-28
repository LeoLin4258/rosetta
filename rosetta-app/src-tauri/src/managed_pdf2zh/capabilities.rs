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

pub fn validate_worker_capabilities(value: &serde_json::Value) -> Result<(), String> {
    let actual = serde_json::from_value::<WorkerEngineCapabilities>(value.clone())
        .map_err(|_| "运行中的 PDF engine 未报告兼容能力".to_string())?;
    let required = required_engine_capabilities();
    validate_claim(
        required.schema_version,
        actual.contract_version,
        actual.engine_revision,
        &actual.capabilities,
        &required,
    )
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

        assert!(validate_worker_capabilities(&value).is_ok());
    }
}
