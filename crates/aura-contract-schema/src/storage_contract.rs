//! 设置 → 系统 → 存储 (`storage.*`): where the data and the installation are, moving the data, and
//! the old data older versions and other installations left.
pub(super) fn register(generator: &mut schemars::SchemaGenerator) {
    generator.subschema_for::<aura_old_data::OldDataListing>();
    generator.subschema_for::<aura_old_data::OldDataEntry>();
    generator.subschema_for::<aura_old_data::OldDataKind>();
    generator.subschema_for::<aura_old_data::OldDataRemoveRequest>();
    generator.subschema_for::<aura_old_data::OldDataRemoval>();
    generator.subschema_for::<aura_old_data::OldDataFailure>();
    generator.subschema_for::<aura_data_move::StorageOverview>();
    generator.subschema_for::<aura_data_move::StoragePlace>();
    generator.subschema_for::<aura_data_move::StorageMoveRequest>();
    generator.subschema_for::<aura_data_move::StorageMoveFailure>();
    generator.subschema_for::<aura_data_move::StorageChoice>();
    generator.subschema_for::<aura_data_move::StorageCheck>();
    generator.subschema_for::<aura_data_move::StorageMovePart>();
    generator.subschema_for::<aura_data_move::StorageMovedPart>();
    generator.subschema_for::<aura_data_move::StorageSelectedFolder>();
    generator.subschema_for::<aura_data_move::StoragePart>();
    generator.subschema_for::<aura_data_move::StorageOpenFolderRequest>();
    generator.subschema_for::<aura_data_move::StorageFolderOpened>();
    generator.subschema_for::<aura_data_move::StorageRestart>();
}
