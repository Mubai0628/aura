use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use aura_v2_importer::{ImportInput, apply, backup_legacy, preview, resume};
use std::path::PathBuf;

fn main() {
    if let Err(error) = run() {
        // Only stable categories/static validation reasons may reach stderr.
        let reason = match error {
            Error::Invalid(reason) => reason,
            Error::Conflict => "persistent revision, source identity or target conflict",
            Error::Json(_) => "invalid document encoding",
            Error::Sqlite(_) => "database schema, integrity or transaction failed",
            Error::Create(_) => "filesystem access or new target creation failed",
        };
        eprintln!("V2 persistence command failed: {reason}");
        std::process::exit(1);
    }
}
fn run() -> StoreResult<()> {
    let mut args = std::env::args().skip(1);
    let command = args.next().unwrap_or_default();
    let rest: Vec<String> = args.collect();
    match command.as_str() {
        "init" if rest.len() == 1 => {
            aura_persistence_v2::V2Store::create(&PathBuf::from(&rest[0]))?;
            println!("Created isolated V2 development database");
        }
        "verify" if rest.len() == 1 => {
            aura_persistence_v2::V2Store::open(&PathBuf::from(&rest[0]))?;
            println!("V2 baseline integrity verified");
        }
        "backup" if rest.len() == 2 => {
            backup_legacy(&PathBuf::from(&rest[0]), &PathBuf::from(&rest[1]))?
        }
        "resume" if rest.len() == 2 => println!(
            "{}",
            serde_json::to_string_pretty(&resume(
                &PathBuf::from(&rest[0]),
                &PathBuf::from(&rest[1])
            )?)?
        ),
        "preview" | "import" => {
            let mut input = ImportInput {
                database: PathBuf::new(),
                include_sessions: false,
                models: None,
                settings: None,
                memory_vault: None,
                config_root: None,
            };
            let mut target = None;
            let mut args = rest.iter();
            while let Some(flag) = args.next() {
                if flag == "--sessions" {
                    input.include_sessions = true;
                    continue;
                }
                let value = args.next().ok_or(Error::Invalid("missing CLI value"))?;
                match flag.as_str() {
                    "--source" => input.database = PathBuf::from(value),
                    "--target" => target = Some(PathBuf::from(value)),
                    "--models" => input.models = Some(PathBuf::from(value)),
                    "--settings" => input.settings = Some(PathBuf::from(value)),
                    "--memory-vault" => input.memory_vault = Some(PathBuf::from(value)),
                    "--config-dir" => input.config_root = Some(PathBuf::from(value)),
                    _ => return Err(Error::Invalid("unknown CLI argument")),
                }
            }
            let plan = preview(input)?;
            let report = if command == "import" {
                apply(&plan, &target.ok_or(Error::Invalid("explicit new import target"))?, None)?
            } else {
                plan.report().clone()
            };
            println!("{}", serde_json::to_string_pretty(&report)?);
            if report.items.iter().any(|i| {
                i.disposition == aura_persistence_core::import_report::ImportDisposition::Conflict
            }) {
                return Err(Error::Conflict);
            }
        }
        _ => return Err(Error::Invalid("command: init/verify/backup/preview/import/resume")),
    }
    Ok(())
}
