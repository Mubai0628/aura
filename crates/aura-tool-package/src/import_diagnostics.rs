//! Bounded UI diagnostics, separate from internal chained errors and OS paths.
use crate::{ToolPackageManifestParseErrorV1 as Manifest, ToolPackageStagingErrorV1 as Staging};
use aura_domain_common::jcs::CanonicalJsonError;
use aura_tool_domain::{
    PackageImportDiagnosticCodeV1 as Code, PackageImportDiagnosticLocationV1 as Location,
    PackageImportDiagnosticV1 as Diagnostic,
};

fn diagnostic(code: Code, location: Location, message: &str) -> Diagnostic {
    Diagnostic { code, location, message: message.to_owned() }
}

fn manifest_location(line: usize, column: usize, field: Option<String>) -> Location {
    Location::Manifest {
        line: u32::try_from(line).ok().filter(|line| *line > 0),
        column: u32::try_from(column).ok().filter(|column| *column > 0),
        field,
    }
}

/// Field names are bounded identifiers, never arbitrary authored text or values.
fn safe_field(value: &str) -> Option<String> {
    (!value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte)))
    .then(|| value.to_owned())
}

pub fn manifest_import_diagnostic_v1(error: &Manifest) -> Diagnostic {
    let unknown = || manifest_location(0, 0, None);
    match error {
        Manifest::SourceRead(_) => diagnostic(
            Code::StorageUnavailable,
            Location::ImportSource,
            "无法读取包清单，请确认导入源仍存在且可读。",
        ),
        Manifest::ManifestTooLarge => {
            diagnostic(Code::LimitExceeded, unknown(), "包清单超过 1 MiB 上限。")
        }
        Manifest::StrictJson(failure) => {
            let (reason, field) = match &failure.error {
                CanonicalJsonError::DuplicateObjectKey { key } => {
                    ("同一个对象中有重复字段。", safe_field(key))
                }
                CanonicalJsonError::UnsafeInteger => ("整数超出 JSON 安全整数范围。", None),
                CanonicalJsonError::NonFiniteNumber => ("数字必须是有限数值。", None),
                _ => ("JSON 格式不合法，请检查此处的括号、引号和逗号。", None),
            };
            diagnostic(
                Code::InvalidManifest,
                manifest_location(failure.line, failure.column, field),
                reason,
            )
        }
        Manifest::RawContract(error) => {
            let text = error.to_string();
            let (reason, field) = if let Some(field) =
                text.strip_prefix("unknown field `").and_then(|text| text.split('`').next())
            {
                ("此字段不受 Manifest V1 支持。", safe_field(field))
            } else if let Some(field) =
                text.strip_prefix("missing field `").and_then(|text| text.split('`').next())
            {
                ("缺少必填字段。", safe_field(field))
            } else {
                ("字段类型、取值或版本不受支持，请按 Manifest V1 修正。", None)
            };
            diagnostic(
                Code::InvalidManifest,
                manifest_location(error.line(), error.column(), field),
                reason,
            )
        }
        Manifest::RawSemantic(error) | Manifest::CanonicalConversion(error) => {
            use aura_tool_domain::ToolPackageManifestContractError as E;
            let (field, reason) = match error {
                E::InvalidFilePolicy => ("files", "文件列表超过数量、大小或路径限制。"),
                E::ManifestListedAsFile => ("files", "文件列表不能把包清单本身列为载荷。"),
                E::WindowsPathCollision => ("files", "文件路径在 Windows 下忽略大小写后重名。"),
                E::DuplicateToolCanonicalName => ("tools", "工具的模型名称重复。"),
                E::InvalidSchemaContract => {
                    ("tools.model", "输入或结果 JSON Schema 不符合工具契约。")
                }
                E::DuplicateEnvironmentName | E::CredentialEnvironmentConflict => {
                    ("tools.runtime.environment", "环境变量重名，或与凭据变量冲突。")
                }
                E::DuplicateHttpHeader | E::CredentialHttpHeaderConflict => {
                    ("tools.runtime.request.headers", "HTTP 标头重名，或与凭据标头冲突。")
                }
                E::InvalidInstanceConfigurationReference => {
                    ("tools.instance_configuration", "运行参数引用了未声明的实例配置。")
                }
                E::InvalidRuntimeContract => {
                    ("tools.runtime", "运行方式、参数映射或资源声明不合法。")
                }
                E::DuplicateCollectionItem | E::InvalidCollectionOrder => {
                    ("", "声明中有重复项或不合法的顺序。")
                }
                E::InvalidText | E::InvalidCanonicalSuffix => {
                    ("", "文字、名称或模型名称后缀不符合长度与字符规则。")
                }
                _ => ("tools", "包清单与规范化内容不一致，请重新导入。"),
            };
            diagnostic(
                Code::InvalidManifest,
                manifest_location(0, 0, (!field.is_empty()).then(|| field.to_owned())),
                reason,
            )
        }
        _ => diagnostic(
            Code::IntegrityMismatch,
            unknown(),
            "包清单无法形成一致的规范化内容，请重新导入。",
        ),
    }
}

pub fn package_import_diagnostic_v1(error: &Staging) -> Diagnostic {
    match error {
        Staging::ManifestInvalid(error) => manifest_import_diagnostic_v1(error),
        Staging::ManifestMissing => diagnostic(
            Code::InvalidManifest,
            manifest_location(0, 0, None),
            "导入源根目录缺少 aura-tool-package.json。",
        ),
        Staging::ManifestTooLarge => diagnostic(
            Code::LimitExceeded,
            manifest_location(0, 0, None),
            "包清单超过 1 MiB 上限。",
        ),
        Staging::InvalidPath
        | Staging::CaseCollision
        | Staging::SpecialFileDenied
        | Staging::InvalidDeclaredFiles(_) => diagnostic(
            Code::PathDenied,
            Location::PackageFiles,
            "包文件路径不合法、忽略大小写后重名，或含链接/重解析点。请检查 files 和导入源。",
        ),
        Staging::InvalidSourceRoot | Staging::InvalidZipSource => diagnostic(
            Code::InvalidSource,
            Location::ImportSource,
            "导入源必须是普通目录或 ZIP 文件，不能是链接或重解析点。",
        ),
        Staging::InvalidStagingParent => {
            diagnostic(Code::StorageUnavailable, Location::Staging, "暂存目录不可用，请重试导入。")
        }
        Staging::Io(_) => diagnostic(
            Code::StorageUnavailable,
            Location::ImportSource,
            "无法读取导入源或写入暂存目录，请确认路径与可用空间。",
        ),
        Staging::InvalidZipArchive(_)
        | Staging::EncryptedZipEntryDenied
        | Staging::ZipCompressionMethodDenied => diagnostic(
            Code::InvalidSource,
            Location::ImportSource,
            "ZIP 损坏、已加密，或使用了不支持的压缩方式；仅支持 Stored 和 Deflate。",
        ),
        Staging::ZipInputTooLarge
        | Staging::ZipCompressionRatioExceeded
        | Staging::EntryCountExceeded
        | Staging::ExpandedSizeExceeded
        | Staging::FileSizeExceeded
        | Staging::PathDepthExceeded => diagnostic(
            Code::LimitExceeded,
            Location::PackageFiles,
            "包超过导入限制：ZIP 64 MiB、展开 256 MiB、单文件 64 MiB、4096 项、深度 32、压缩比 100。",
        ),
        _ => diagnostic(
            Code::IntegrityMismatch,
            Location::PackageFiles,
            "实际文件集合、长度或 SHA-256 与清单不一致；请确认导入时包未被修改。",
        ),
    }
}
