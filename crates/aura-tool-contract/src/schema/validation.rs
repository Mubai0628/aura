use super::*;

pub struct CompiledToolSchemaV1 {
    pub(super) validator: Arc<Validator>,
    schema: Arc<Value>,
}

impl CompiledToolSchemaV1 {
    pub fn compile(schema: &CanonicalJsonSchemaDocumentV1) -> Result<Self, ToolSchemaError> {
        // Public documents can be reconstructed by callers. Verify the hash
        // before lookup, so a forged hash cannot borrow another validator.
        let hash =
            canonical_sha256(&schema.document).map_err(|_| ToolSchemaError::Canonicalization)?;
        if hash != schema.schema_hash
            || schema.schema_policy_version != CANONICAL_TOOL_SCHEMA_POLICY_VERSION
        {
            return Err(ToolSchemaError::HashMismatch);
        }
        type Validators = std::collections::BTreeMap<String, (Arc<Validator>, Arc<Value>)>;
        static CACHE: OnceLock<Mutex<Validators>> = OnceLock::new();
        let cache = CACHE.get_or_init(|| Mutex::new(Validators::new()));
        let key = hash.hex().to_owned();
        if let Some((validator, document)) =
            cache.lock().map_err(|_| ToolSchemaError::ValidatorCompilation)?.get(&key).cloned()
        {
            return Ok(Self { validator, schema: document });
        }
        schema.validate()?;
        let validator = jsonschema::options()
            .with_draft(Draft::Draft202012)
            .offline()
            .should_validate_formats(true)
            .should_ignore_unknown_formats(false)
            .with_pattern_options(
                PatternOptions::regex().size_limit(MAX_SCHEMA_PATTERN_TOTAL_BYTES),
            )
            .build(&schema.document)
            .map_err(|_| ToolSchemaError::ValidatorCompilation)?;
        let validator = Arc::new(validator);
        let mut cache = cache.lock().map_err(|_| ToolSchemaError::ValidatorCompilation)?;
        if cache.len() >= 64 {
            cache.clear();
        }
        let document = Arc::new(schema.document.clone());
        cache.insert(key, (validator.clone(), document.clone()));
        Ok(Self { validator, schema: document })
    }

    #[must_use]
    pub fn validate(&self, instance: &Value) -> ToolSchemaValidationReportV1 {
        let mut errors = self
            .validator
            .iter_errors(instance)
            .map(|error| ToolSchemaValidationIssueV1 {
                instance_path: error.instance_path().as_str().to_owned(),
                schema_path: error.schema_path().as_str().to_owned(),
                issue_kind: format!(
                    "{}: {}",
                    validation_message::code(error.kind()),
                    validation_message::message(&error, &self.schema)
                ),
            })
            .collect::<Vec<_>>();
        errors.sort();
        let total_error_count = u64::try_from(errors.len()).unwrap_or(u64::MAX);
        errors.truncate(MAX_PUBLIC_SCHEMA_ERRORS);
        ToolSchemaValidationReportV1 { valid: total_error_count == 0, errors, total_error_count }
    }
}
