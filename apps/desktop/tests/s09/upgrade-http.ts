import { createHash } from 'node:crypto';
import { readFile, readdir } from 'node:fs/promises';
import { join } from 'node:path';
import { DatabaseSync } from 'node:sqlite';

type ObjectValue = Record<string, unknown>;
function canonical(value: unknown): string {
  if (Array.isArray(value)) return `[${value.map(canonical).join(',')}]`;
  if (value !== null && typeof value === 'object') {
    const record = value as ObjectValue;
    return `{${Object.keys(record)
      .sort()
      .map((key) => `${JSON.stringify(key)}:${canonical(record[key])}`)
      .join(',')}}`;
  }
  return JSON.stringify(value);
}
function fingerprint(value: unknown) {
  return {
    algorithm: 'sha256',
    canonicalization_version: 'rfc8785_jcs_v1',
    hex: createHash('sha256').update(canonical(value)).digest('hex'),
  };
}
type Row = { record_json: string };

/** Replace only this test's stopped isolated HTTP instance with the published v1 fixture. */
export async function seedPreviousHttp(root: string, currentId: string) {
  const files = await readdir(root, { recursive: true });
  const candidates: string[] = [];
  for (const file of files.filter((name) => name.endsWith('.db'))) {
    const db = new DatabaseSync(join(root, file), { readOnly: true });
    try {
      if (db.prepare("SELECT 1 FROM sqlite_master WHERE name='signed_builtin_release_state'").get())
        candidates.push(file);
    } finally {
      db.close();
    }
  }
  if (candidates.length !== 1) throw new Error('Expected exactly one isolated configuration DB');
  const old = JSON.parse(
    await readFile(new URL('./http-request-v1.json', import.meta.url), 'utf8'),
  ) as { definition: ObjectValue; instance: ObjectValue };
  const db = new DatabaseSync(join(root, candidates[0]!));
  try {
    db.exec('BEGIN IMMEDIATE');
    const current = JSON.parse(
      (
        db
          .prepare('SELECT record_json FROM tool_instances WHERE tool_instance_id=?')
          .get(currentId) as Row
      ).record_json,
    ) as ObjectValue;
    const binding = JSON.parse(
      (
        db
          .prepare(
            'SELECT record_json FROM tool_instance_credential_bindings WHERE tool_instance_id=?',
          )
          .get(currentId) as Row
      ).record_json,
    ) as ObjectValue;
    const instance: ObjectValue = {
      ...old.instance,
      enabled: current.enabled,
      overrides: current.overrides,
      entity_revision: current.entity_revision,
      updated_at: current.updated_at,
    };
    const definition = old.definition;
    const version = definition.tool_version;
    db.prepare(
      'INSERT INTO tool_definitions(tool_id,tool_version_json,tool_package_id,tool_key,canonical_name,definition_revision,definition_fingerprint_json,entity_revision,record_json,record_hash_json,retired_at) VALUES(?,?,NULL,NULL,?,1,?,1,?,?,NULL)',
    ).run(
      String(definition.tool_id),
      canonical(version),
      String(definition.canonical_name),
      canonical(definition.definition_fingerprint),
      canonical(definition),
      canonical(fingerprint(definition)),
    );
    const oldId = String(instance.tool_instance_id);
    db.prepare(
      'INSERT INTO tool_instances(tool_instance_id,tool_id,tool_version_json,source_kind,enabled,runtime_health,entity_revision,record_json,record_hash_json,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?,?)',
    ).run(
      oldId,
      String(instance.tool_id),
      canonical(version),
      'built_in',
      instance.enabled ? 1 : 0,
      'healthy',
      Number(instance.entity_revision),
      canonical(instance),
      canonical(fingerprint(instance)),
      String(instance.created_at),
      String(instance.updated_at),
    );
    const moved = { ...binding, tool_instance_id: oldId };
    const hashMaterial = Object.fromEntries(
      [
        'tool_instance_id',
        'credential_slot_key',
        'credential_id',
        'credential_revision',
        'placement',
        'binding_revision',
      ].map((key) => [key, moved[key as keyof typeof moved]]),
    );
    const updated = { ...moved, binding_hash: fingerprint(hashMaterial) };
    db.prepare(
      'UPDATE tool_instance_credential_bindings SET tool_instance_id=?,binding_hash_json=?,record_json=?,record_hash_json=? WHERE tool_instance_id=?',
    ).run(
      oldId,
      canonical(updated.binding_hash),
      canonical(updated),
      canonical(fingerprint(updated)),
      currentId,
    );
    db.prepare('DELETE FROM tool_instances WHERE tool_instance_id=?').run(currentId);
    if (db.prepare('PRAGMA foreign_key_check').all().length)
      throw new Error('Isolated upgrade fixture has an invalid relationship');
    db.exec('COMMIT');
    return oldId;
  } catch (error) {
    db.exec('ROLLBACK');
    throw error;
  } finally {
    db.close();
  }
}
