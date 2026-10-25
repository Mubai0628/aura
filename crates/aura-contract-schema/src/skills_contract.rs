//! Skills: Settings → Skill.
use aura_skills::*;
pub(super) fn register(generator: &mut schemars::SchemaGenerator) {
    generator.subschema_for::<SkillSource>();
    generator.subschema_for::<SkillState>();
    generator.subschema_for::<SkillSetting>();
    generator.subschema_for::<SkillFile>();
    generator.subschema_for::<SkillView>();
    generator.subschema_for::<SkillRootView>();
    generator.subschema_for::<SkillRegistryView>();
    generator.subschema_for::<SkillPreview>();
    generator.subschema_for::<SkillListRequest>();
    generator.subschema_for::<SkillRequest>();
    generator.subschema_for::<SkillTrustRequest>();
    generator.subschema_for::<SkillChangeRequest>();
    generator.subschema_for::<SkillSetEnabledRequest>();
    generator.subschema_for::<SkillImportRequest>();
    generator.subschema_for::<SkillSelectDirectoryRequest>();
    generator.subschema_for::<SkillSelectedDirectory>();
    generator.subschema_for::<SkillDirectoryOpened>();
    generator.subschema_for::<SkillSelectArchiveRequest>();
    generator.subschema_for::<SkillSelectedArchive>();
}
