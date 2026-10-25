// Development and test builds link with rust-lld
// (.cargo/config.toml). Release builds keep MSVC link.exe until their output (PE security
// flags, embedded manifests, signing) has been checked with rust-lld; this runs cargo with the
// arguments it is given and that one linker setting changed back.
import { spawnSync } from 'node:child_process';

const result = spawnSync('cargo', process.argv.slice(2), {
  env: { ...process.env, CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER: 'link.exe' },
  stdio: 'inherit',
  windowsHide: true,
});
if (result.error) {
  console.error(result.error.message);
  process.exit(1);
}
process.exit(result.status ?? 1);
