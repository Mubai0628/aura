//! Local built-in input types have invariants beyond the portable JSON sent to providers.
//! Apply them to the frozen input description, including definitions saved by older sessions.
use super::ToolSchemaValidationIssueV1;
use crate::{ToolSearchCursorV1, ToolSearchQueryV1};
use aura_domain_common::{
    COMMON_ID_TYPES, ContentHash, Revision, TOOL_ID_TYPES, UtcTimestamp, parse_typed_ulid,
};
use jsonschema::ValidationOptions;
use jsonschema::error::{ValidationError, ValidationErrorKind};
use serde_json::Value;
use std::str::FromStr;

const FILE_HASH: &str = "aura-file-content-hash";
const MEMORY_HASH: &str = "aura-memory-content-hash";
const CANONICAL_HASH: &str = "aura-registry-digest";
const ID_FORMAT: &str = "aura-id-";
const DECIMAL: &str = "aura-unsigned-decimal";
const TIMESTAMP: &str = "aura-rfc3339-time";
const SEARCH_QUERY: &str = "aura-tool-search-query";
const SEARCH_CURSOR: &str = "aura-tool-search-cursor";

fn parses<T: FromStr>(value: &str) -> bool {
    value.parse::<T>().is_ok()
}

pub(super) fn configure<'i>(
    mut options: ValidationOptions<'i>,
    document: &mut Value,
) -> ValidationOptions<'i> {
    let Some(definitions) = document.get_mut("$defs").and_then(Value::as_object_mut) else {
        return options;
    };
    for (name, field, format) in [
        ("ContentHash", "hex", FILE_HASH),
        ("MemoryUpdateItem", "expected_hash", MEMORY_HASH),
        ("MemoryDeleteItem", "expected_hash", MEMORY_HASH),
        ("CanonicalSha256HashV1", "hex", CANONICAL_HASH),
        ("CanonicalBlake3HashV1", "hex", CANONICAL_HASH),
    ] {
        if let Some(property) = definitions
            .get_mut(name)
            .and_then(|definition| definition.get_mut("properties"))
            .and_then(|properties| properties.get_mut(field))
            && set_string_format(property, format)
        {
            options = options.with_format(format, ContentHash::is_valid_hex);
        }
    }
    type StringRule = (&'static str, &'static str, fn(&str) -> bool);
    let rules: &[StringRule] = &[
        ("Revision", DECIMAL, parses::<Revision>),
        ("SessionSequence", DECIMAL, parses::<aura_domain_common::SessionSequence>),
        ("UtcTimestamp", TIMESTAMP, parses::<UtcTimestamp>),
        ("ToolSearchQueryV1", SEARCH_QUERY, parses::<ToolSearchQueryV1>),
        ("ToolSearchCursorV1", SEARCH_CURSOR, |value| ToolSearchCursorV1::new(value).is_ok()),
    ];
    for &(name, format, validate) in rules {
        if let Some(definition) = definitions.get_mut(name)
            && set_string_format(definition, format)
        {
            options = options.with_format(format, validate);
        }
    }
    for &(name, prefix) in COMMON_ID_TYPES.iter().chain(TOOL_ID_TYPES) {
        if let Some(definition) = definitions.get_mut(name) {
            let format = format!("{ID_FORMAT}{prefix}");
            if set_string_format(definition, &format) {
                options = options.with_format(format, move |value: &str| {
                    parse_typed_ulid(value, prefix).is_ok()
                });
            }
        }
    }
    options
}

fn set_string_format(schema: &mut Value, format: &str) -> bool {
    if schema["type"] != "string" || schema.get("format").is_some() {
        return false;
    }
    schema["format"] = Value::String(format.to_owned());
    true
}

// An untagged union may contain several unrelated errors from branches the user did not select.
// Prefer the concrete field in the submitted shape over a generic union description.
pub(super) fn correction(error: &ValidationError<'_>) -> Option<ToolSchemaValidationIssueV1> {
    match error.kind() {
        ValidationErrorKind::AnyOf { context } | ValidationErrorKind::OneOfNotValid { context } => {
            context.iter().find_map(|branch| {
                let mut issues = branch.iter();
                let correction_for_first = correction(issues.next()?)?;
                issues.all(|other| correction(other).is_some()).then_some(correction_for_first)
            })
        }
        ValidationErrorKind::Format { format } => {
            let hint = match format.as_str() {
                FILE_HASH => "内容哈希必须为 64 个小写十六进制字符（0-9、a-f）。请原样复制读取结果的哈希对象，文件使用 fs.read 返回的 logical_hash。".to_owned(),
                MEMORY_HASH => "记忆哈希必须为 64 个小写十六进制字符（0-9、a-f）。请用 memory.read 重新读取并原样复制返回的哈希。".to_owned(),
                CANONICAL_HASH => "摘要必须为 64 个小写十六进制字符（0-9、a-f）。请原样复制工具目录或快照返回的完整引用。".to_owned(),
                DECIMAL => "请使用规范的无符号十进制字符串，范围为 0 到 18446744073709551615，仅 0 可用 0 开头。修订号与事件序号请从相应读取结果原样复制。".to_owned(),
                TIMESTAMP => "时间请用含时区的 RFC 3339 字符串，例如 2026-01-01T00:00:00Z。".to_owned(),
                SEARCH_QUERY => format!("请提供有效关键词，最多 {} 个 UTF-8 字节，不能全为空白或含 NUL、DEL。", crate::MAX_TOOL_SEARCH_QUERY_BYTES),
                SEARCH_CURSOR => "请复制上一页结果的游标；首次搜索可省略 cursor。".to_owned(),
                name => {
                    let prefix = name.strip_prefix(ID_FORMAT)?;
                    format!("此标识须以 {prefix}_ 开头，后接 26 位规范 ULID。请原样使用对应查询或创建结果返回的标识。")
                }
            };
            Some(ToolSchemaValidationIssueV1 {
                instance_path: error.instance_path().as_str().to_owned(),
                schema_path: error.schema_path().as_str().trim_end_matches("/format").to_owned(),
                issue_kind: format!("format: {hint}"),
            })
        }
        _ => None,
    }
}
