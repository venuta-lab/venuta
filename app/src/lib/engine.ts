// Typed wrappers over the Rust commands in app/src-tauri/src/lib.rs.
import { invoke } from '@tauri-apps/api/core'

export interface ScanReport {
  files_seen: number
  unchanged: number
  added: number
  changed: number
  removed: number
  read_errors: number
  audio_files: number
  elapsed_ms: number
}

export const engineVersion = () => invoke<string>('engine_version')
export const scanLibrary = (root: string) => invoke<ScanReport>('scan_library', { root })
