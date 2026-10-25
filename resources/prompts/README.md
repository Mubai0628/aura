# Prompt resources

P00 只建立可校验的空 manifest，不包含产品 Prompt。后续阶段添加 Prompt 时，必须同时登记稳定 ID、版本、相对路径和内容哈希，并通过 `cargo xtask validate-manifests`。
