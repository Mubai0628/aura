use super::*;

pub struct CompiledToolSchemaV1 {
    pub(super) validator: Arc<Validator>,
    schema: Arc<Value>,
    builtin_input: bool,
}

impl CompiledToolSchemaV1 {
    pub fn compile(schema: &CanonicalJsonSchemaDocumentV1) -> Result<Self, ToolSchemaError> {
        Self::compile_with(schema, false)
    }

    /// Validate a frozen tool input using the source of the registry binding. External tools
    /// own their types, even when their local definition names coincide with Aura types.
    pub fn for_tool_input(
        input: &ToolInputSchemaV1,
        source: crate::ToolSourceKindV1,
    ) -> Result<Self, ToolSchemaError> {
        Self::compile_with(input.canonical_document(), source == crate::ToolSourceKindV1::BuiltIn)
    }

    fn compile_with(
        schema: &CanonicalJsonSchemaDocumentV1,
        builtin_input: bool,
    ) -> Result<Self, ToolSchemaError> {
        // Public documents can be reconstructed by callers. Verify the hash
        // before lookup, so a forged hash cannot borrow another validator.
        let hash =
            canonical_sha256(&schema.document).map_err(|_| ToolSchemaError::Canonicalization)?;
        if hash != schema.schema_hash
            || schema.schema_policy_version != CANONICAL_TOOL_SCHEMA_POLICY_VERSION
        {
            return Err(ToolSchemaError::HashMismatch);
        }
        type Validators = std::collections::BTreeMap<(String, bool), (Arc<Validator>, Arc<Value>)>;
        static CACHE: OnceLock<Mutex<Validators>> = OnceLock::new();
        let cache = CACHE.get_or_init(|| Mutex::new(Validators::new()));
        let key = (hash.hex().to_owned(), builtin_input);
        if let Some((validator, document)) =
            cache.lock().map_err(|_| ToolSchemaError::ValidatorCompilation)?.get(&key).cloned()
        {
            return Ok(Self { validator, schema: document, builtin_input });
        }
        schema.validate()?;
        let options = jsonschema::options()
            .with_draft(Draft::Draft202012)
            .offline()
            .should_validate_formats(true)
            .should_ignore_unknown_formats(false)
            .with_pattern_options(
                PatternOptions::regex().size_limit(MAX_SCHEMA_PATTERN_TOTAL_BYTES),
            );
        let mut document = schema.document.clone();
        let options = if builtin_input {
            builtin_formats::configure(options, &mut document)
        } else {
            options
        };
        let validator =
            options.build(&document).map_err(|_| ToolSchemaError::ValidatorCompilation)?;
        let validator = Arc::new(validator);
        let mut cache = cache.lock().map_err(|_| ToolSchemaError::ValidatorCompilation)?;
        if cache.len() >= 64 {
            cache.clear();
        }
        let document = Arc::new(document);
        cache.insert(key, (validator.clone(), document.clone()));
        Ok(Self { validator, schema: document, builtin_input })
    }

    #[must_use]
    pub fn validate(&self, instance: &Value) -> ToolSchemaValidationReportV1 {
        let mut errors = self
            .validator
            .iter_errors(instance)
            .map(|error| {
                if let Some(issue_kind) =
                    validation_message::closed_object_fields(&error, &self.schema, instance)
                {
                    return ToolSchemaValidationIssueV1 {
                        instance_path: error.instance_path().as_str().to_owned(),
                        schema_path: error.schema_path().as_str().to_owned(),
                        issue_kind,
                    };
                }
                if self.builtin_input
                    && let Some(issue) = builtin_formats::correction(&error)
                {
                    return issue;
                }
                ToolSchemaValidationIssueV1 {
                    instance_path: error.instance_path().as_str().to_owned(),
                    schema_path: error.schema_path().as_str().to_owned(),
                    issue_kind: format!(
                        "{}: {}",
                        validation_message::code(error.kind()),
                        validation_message::message(&error, &self.schema)
                    ),
                }
            })
            .collect::<Vec<_>>();
        errors.sort();
        let total_error_count = u64::try_from(errors.len()).unwrap_or(u64::MAX);
        errors.dedup();
        errors.truncate(MAX_PUBLIC_SCHEMA_ERRORS);
        ToolSchemaValidationReportV1 { valid: total_error_count == 0, errors, total_error_count }
    }
}
