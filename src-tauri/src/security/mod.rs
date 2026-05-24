// WorkMirror — Security module
//
// Provides AES-256-GCM encryption backed by platform-native keychain storage.
//
// ## Platform keychain backends
//
// |  Platform | Backend                         |
// |-----------|---------------------------------|
// |  Windows  | DPAPI via `keyring` crate       |
// |  macOS    | Keychain Services via `keyring` |
// |  Linux    | Secret Service (D-Bus)          |

pub mod crypto;
