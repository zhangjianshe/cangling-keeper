//! Validation for private stdio environment patches and public project scopes.
use crate::store::Store;
use serde_json::Value;

const ENVIRONMENT_VARS: usize = 32;
const ENVIRONMENT_VALUE_BYTES: usize = 8192;
const ENVIRONMENT_BYTES: usize = 32768;
const PROJECT_IDS: usize = 128;

fn valid_environment_key(key: &str) -> bool {
    let bytes = key.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 128
        && (bytes[0].is_ascii_alphabetic() || bytes[0] == b'_')
        && bytes[1..]
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'_')
}
fn validate_environment(value: &Value) -> Result<(), String> {
    let object = value.as_object().ok_or("MCP 环境变量配置必须是对象")?;
    if object.len() > ENVIRONMENT_VARS {
        return Err("MCP 环境变量最多 32 项".into());
    }
    for (key, value) in object {
        if !valid_environment_key(key) {
            return Err("MCP 环境变量名称格式无效".into());
        }
        let text = value.as_str().ok_or("MCP 环境变量的值必须是字符串")?;
        if text.len() > ENVIRONMENT_VALUE_BYTES || text.contains('\0') {
            return Err("MCP 环境变量的值过长或包含空字符".into());
        }
    }
    if value.to_string().len() > ENVIRONMENT_BYTES {
        return Err("MCP 环境变量配置超过 32 KiB".into());
    }
    Ok(())
}

/// Apply only explicitly supplied changes. No validation error includes stored values.
pub(super) fn environment(previous: &Value, changes: Option<&Value>) -> Result<Value, String> {
    validate_environment(previous)?;
    let Some(changes) = changes else {
        return Ok(previous.clone());
    };
    let changes = changes.as_object().ok_or("MCP 环境变量修改必须是对象")?;
    let mut result = previous.as_object().unwrap().clone();
    for (key, value) in changes {
        if !valid_environment_key(key) {
            return Err("MCP 环境变量名称格式无效".into());
        }
        if value.is_null() {
            result.remove(key);
        } else {
            let text = value
                .as_str()
                .ok_or("MCP 环境变量修改只允许字符串或 null")?;
            if text.len() > ENVIRONMENT_VALUE_BYTES || text.contains('\0') {
                return Err("MCP 环境变量的值过长或包含空字符".into());
            }
            result.insert(key.clone(), value.clone());
        }
    }
    let result = Value::Object(result);
    validate_environment(&result)?;
    Ok(result)
}

/// Null means all projects; explicit arrays must name real, distinct projects.
pub(super) fn project_ids(
    store: &Store,
    input: Option<&Value>,
    previous: &Value,
) -> Result<Value, String> {
    let value = input.unwrap_or(previous);
    if value.is_null() {
        return Ok(Value::Null);
    }
    let ids = value
        .as_array()
        .ok_or("MCP 项目范围必须是 null 或项目列表")?;
    if ids.is_empty() || ids.len() > PROJECT_IDS {
        return Err("请为 MCP 服务选择至少一个项目，最多 128 个".into());
    }
    let mut names = std::collections::HashSet::new();
    for id in ids {
        let id = id
            .as_str()
            .filter(|id| !id.is_empty() && id.len() <= 128 && !id.chars().any(char::is_control))
            .ok_or("MCP 项目标识无效")?;
        if !names.insert(id) {
            return Err("MCP 项目列表不能重复".into());
        }
        let exists: bool = store
            .conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM agent_projects WHERE id=?1)",
                [id],
                |r| r.get(0),
            )
            .map_err(|_| "无法检查 MCP 项目范围")?;
        if !exists {
            return Err("所选 MCP 项目不存在".into());
        }
    }
    Ok(value.clone())
}

/// Fail closed if a persisted scope has an invalid shape.
pub(super) fn applies(project_ids: &Value, project: &str) -> bool {
    if project_ids.is_null() {
        return true;
    }
    let Some(ids) = project_ids.as_array() else {
        return false;
    };
    if ids.is_empty() || ids.len() > PROJECT_IDS {
        return false;
    }
    let mut names = std::collections::HashSet::new();
    for id in ids {
        let Some(id) = id
            .as_str()
            .filter(|id| !id.is_empty() && id.len() <= 128 && !id.chars().any(char::is_control))
        else {
            return false;
        };
        if !names.insert(id) {
            return false;
        }
    }
    names.contains(project)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn environment_patches_preserve_omit_delete_and_empty_strings() {
        let existing = json!({"SECRET":"private-value","UNCHANGED":"keep","EMPTY":"previous"});
        assert_eq!(environment(&existing, None).unwrap(), existing);
        let updated = environment(
            &existing,
            Some(&json!({"SECRET":null,"EMPTY":"","NEW":"new-value"})),
        )
        .unwrap();
        assert_eq!(
            updated,
            json!({"UNCHANGED":"keep","EMPTY":"","NEW":"new-value"})
        );
        assert_eq!(environment(&existing, Some(&json!({}))).unwrap(), existing);
        assert!(environment(&existing, Some(&Value::Null)).is_err());
        assert!(environment(&existing, Some(&json!({"SECRET":true}))).is_err());
    }

    #[test]
    fn environment_rejects_invalid_keys_and_redacts_all_errors() {
        for key in ["", "1NAME", "BAD-NAME", "BAD=NAME", "白名单", "SPACE NAME"] {
            let mut changes = serde_json::Map::new();
            changes.insert(key.into(), json!("private-value-sentinel"));
            let error = environment(&json!({}), Some(&Value::Object(changes))).unwrap_err();
            assert!(!error.contains("private-value-sentinel"));
        }
        let boundary = "A".repeat(128);
        assert!(environment(&json!({}), Some(&json!({boundary:"value","_OK_1":""}))).is_ok());
        assert!(environment(&json!({}), Some(&json!({"A".repeat(129):"secret"}))).is_err());
        for value in [
            "x".repeat(8193),
            "private-value-sentinel\0".to_owned(),
            "文".repeat(2731),
        ] {
            let error = environment(&json!({}), Some(&json!({"TOKEN":value}))).unwrap_err();
            assert!(!error.contains("private-value-sentinel"));
        }
    }

    #[test]
    fn environment_enforces_result_count_and_total_encoded_size() {
        let mut variables = serde_json::Map::new();
        for index in 0..32 {
            variables.insert(format!("V_{index}"), json!("x"));
        }
        let previous = Value::Object(variables.clone());
        assert!(environment(&previous, None).is_ok());
        assert!(environment(&previous, Some(&json!({"V_32":"x"}))).is_err());
        assert!(environment(&previous, Some(&json!({"V_0":null,"V_32":"x"}))).is_ok());
        variables.clear();
        for index in 0..4 {
            variables.insert(format!("V_{index}"), json!("x".repeat(8192)));
        }
        assert!(environment(&json!({}), Some(&Value::Object(variables))).is_err());
        assert!(environment(&json!({}), Some(&json!({"QUOTED":"\\".repeat(8192)}))).is_ok());
    }

    fn store() -> Store {
        let store = Store {
            conn: rusqlite::Connection::open_in_memory().unwrap(),
        };
        store.conn.execute_batch("CREATE TABLE agent_projects(id TEXT PRIMARY KEY); INSERT INTO agent_projects VALUES ('general'),('project-a'),('project-b');").unwrap();
        store
    }
    #[test]
    fn project_scope_preserves_omitted_and_accepts_explicit_all() {
        let store = store();
        let previous = json!(["project-a"]);
        assert_eq!(project_ids(&store, None, &previous).unwrap(), previous);
        assert_eq!(
            project_ids(&store, Some(&Value::Null), &previous).unwrap(),
            Value::Null
        );
        assert_eq!(
            project_ids(&store, Some(&json!(["general", "project-b"])), &previous).unwrap(),
            json!(["general", "project-b"])
        );
        for invalid in [
            json!([]),
            json!(["project-a", "project-a"]),
            json!(["missing"]),
            json!([true]),
            json!("general"),
        ] {
            assert!(project_ids(&store, Some(&invalid), &previous).is_err());
        }
        assert!(project_ids(&store, Some(&json!(["x".repeat(129)])), &previous).is_err());
        assert!(project_ids(&store, Some(&json!(vec!["general"; 129])), &previous).is_err());
    }
    #[test]
    fn project_matching_is_exact_and_invalid_shapes_fail_closed() {
        assert!(applies(&Value::Null, "any-project"));
        assert!(applies(&json!(["project-a"]), "project-a"));
        assert!(!applies(&json!(["project-a"]), "project"));
        assert!(!applies(&json!(["project-a"]), "project-b"));
        for invalid in [
            json!([]),
            json!({"project-a":true}),
            json!(["project-a", false]),
            json!([""]),
            json!(["project-a", "project-a"]),
            json!("project-a"),
        ] {
            assert!(!applies(&invalid, "project-a"));
        }
    }
}
