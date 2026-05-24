# WorkMirror Security White Paper

> **Version:** 1.0 · **Last Updated:** May 2026

This document describes the security architecture, encryption mechanisms, and privacy guarantees of WorkMirror. It is intended for security-conscious users, enterprise administrators, and open-source auditors.

---

## 1. Data Storage Location

All WorkMirror data is stored **exclusively on your local machine**. No data is ever transmitted to external servers.

| Data | Location | Format |
|------|----------|--------|
| Activity records | `{app_data_dir}/workmirror/data.db` | Encrypted SQLite ([see §2](#2-encryption)) |
| Category rules | `{app_data_dir}/workmirror/category_rules.json` | Plaintext JSON |
| Generated reports | `{app_data_dir}/workmirror/reports/` | HTML / PDF |

The `app_data_dir` is platform-specific:

| Platform | Path |
|----------|------|
| Windows | `%APPDATA%/workmirror/` |
| macOS | `~/Library/Application Support/workmirror/` |
| Linux | `~/.local/share/workmirror/` |

**Sensitive fields** (`window_title`, `process_name`) in the SQLite database are encrypted (see below). Non-sensitive fields (`timestamp`, `duration_seconds`, `category`) are stored in cleartext to enable efficient SQL queries.

---

## 2. Encryption

### Algorithm

| Property | Value |
|----------|-------|
| Algorithm | **AES-256-GCM** |
| Key size | 256 bits (32 bytes) |
| Nonce size | 96 bits (12 bytes) |
| Authentication tag | 128 bits (16 bytes) |
| Mode | Authenticated encryption (AEAD) |
| Nonce source | OS CSPRNG |
| Implementation | `aes-gcm` crate (pure Rust, no C dependencies) |

### Data Format

Every encrypted field is stored as:

```
[12-byte nonce ‖ AES-GCM ciphertext]
```

The nonce is generated fresh from the operating system's cryptographically secure pseudo-random number generator (CSPRNG) for each encryption operation. No nonce is ever reused.

### What Is Encrypted

- `activities.window_title` — **Encrypted** (BLOB)
- `activities.process_name` — **Encrypted** (BLOB)
- `activities.timestamp` — **Plaintext** (for SQL querying)
- `activities.duration_seconds` — **Plaintext**
- `activities.category` — **Plaintext**
- `config` table — **Plaintext** (only non-sensitive preferences)

---

## 3. Key Management

### Key Storage

WorkMirror uses the [keyring](https://crates.io/crates/keyring) crate to store the master encryption key in the platform's native credential service:

| Platform | Backend |
|----------|---------|
| Windows | **DPAPI** (Credential Manager) |
| macOS | **Keychain Services** |
| Linux | **Secret Service** (D-Bus / libsecret) |

### Key Lifecycle

1. **First run:** A new 256-bit key is generated from `OsRng` and persisted to the platform keychain as a hex-encoded string.
2. **Subsequent runs:** The key is loaded from the keychain on first call to `encrypt()` or `decrypt()`.
3. **In-memory caching:** The AES-GCM cipher is cached in a `OnceLock<Mutex<Option<Aes256Gcm>>>` after initialisation. The raw key bytes in memory are cleared with `zeroize` after the cipher is constructed.
4. **Key rotation:** Not yet implemented. If you need to rotate the key, clear all data via the Settings page — a new key will be generated automatically.

### Memory Safety

- Zero `unsafe` blocks in the security module.
- All temporary plaintext and key buffers are cleared with `zeroize` after use.
- The `zeroize` trait is implemented for all key material types.

---

## 4. Network Behavior

**WorkMirror does not make any network connections by default.**

The only exception is the optional **AI analysis feature**, which communicates with a **local** Ollama instance:

| Component | Network | Default | Configurable |
|-----------|---------|---------|-------------|
| Tracker | ❌ None | — | — |
| Database | ❌ None | — | — |
| Report generation | ❌ None | — | — |
| AI analysis (Ollama) | ✅ Localhost only | `http://localhost:11434` | Yes, via Settings |

The AI client only connects to `localhost` by default. You can configure a different Ollama URL in Settings — but this is intended for advanced setups (e.g., running Ollama in a Docker container on the same machine). **WorkMirror will never connect to external AI providers.**

If Ollama is not running, all AI features degrade gracefully by returning statistics-only results without error.

---

## 5. Third-Party Dependency Audit

WorkMirror's dependencies are continuously monitored via:

| Channel | Method |
|---------|--------|
| `cargo audit` | Automated Rust dependency vulnerability scanning |
| `pnpm audit` | Automated NPM dependency vulnerability scanning |
| Manual review | Critical dependencies are reviewed on each major release |

### Rust Dependency Policy

- All Rust crates are sourced from **crates.io**.
- The project pins exact versions in `Cargo.lock`.
- Zero `unsafe` blocks in our code (warnings from dependencies are monitored).
- The `aes-gcm` crate is a pure Rust implementation with no C dependencies, reducing supply-chain risk.

### Frontend Dependency Policy

- All NPM packages are pinned in `pnpm-lock.yaml`.
- Minimal runtime dependencies: only Solid.js + Chart.js + Tauri API.
- No analytics, telemetry, or tracking scripts are included.

---

## 6. Vulnerability Reporting

If you discover a security vulnerability in WorkMirror, please report it responsibly:

1. **Do not** open a public GitHub issue.
2. Send a private email to the maintainer (see project profile).
3. Include a detailed description of the vulnerability.
4. Allow up to 14 days for a fix before public disclosure.

We aim to respond to all security reports within 48 hours.

---

## 7. User Data Export Format

Users can export their activity data from the Settings page. The export format is **JSON**:

```json
{
  "version": 1,
  "exported_at": "2026-05-24T12:00:00+08:00",
  "activities": [
    {
      "timestamp": "2026-05-24T09:00:00",
      "window_title": "Visual Studio Code",
      "process_name": "code.exe",
      "duration_seconds": 3600,
      "category": "IDE"
    }
  ],
  "stats": {
    "total_active_hours": 8.5,
    "total_days": 7,
    "avg_daily_hours": 1.2
  }
}
```

**Note:** The exported data is in plaintext — the encryption is transparently applied at rest only.

---

## 8. Data Clearance

All data can be permanently deleted from the Settings page ("Clear All Data").

The operation:
1. Drops all rows from the `activities` and `config` tables.
2. The SQLite database file is **not** deleted (to avoid breaking file handles); its size can be reclaimed with `VACUUM` (run automatically on next `init`).
3. The encryption key in the platform keychain is preserved (new data will reuse it).

**To completely remove all traces:**

```bash
# Stop WorkMirror first, then:
rm -rf ~/.local/share/workmirror      # Linux
rm -rf ~/Library/Application\ Support/workmirror  # macOS
rm -rf %APPDATA%\workmirror           # Windows
```

---

## 9. Platform-Specific Security Notes

### Windows

- **Idle detection** uses `GetLastInputInfo` via the `winapi` crate. This function queries the number of milliseconds since the last user input (keyboard or mouse). No accessibility APIs are used.
- **Keychain** uses the built-in Credential Manager (DPAPI-encrypted).
- The Tauri webview uses **WebView2** (Microsoft Edge Chromium). No additional sandboxing is applied; the webview operates with the same permissions as the host process.

### macOS

- **Idle detection** uses CoreGraphics `CGEventSourceSecondsSinceLastEvent`. This is a standard macOS API that does not require accessibility permissions.
- **Keychain** uses the system Keychain Services.
- Gatekeeper and notarization are recommended for distribution builds.

### Linux (X11 / Wayland)

- **Idle detection** uses the X11 ScreenSaver extension (`XScreenSaverQueryInfo`). On Wayland, idle detection may not work — WorkMirror will fall back to "always active" mode.
- **Keychain** uses Secret Service (D-Bus). Requires `libsecret` and a compatible keychain daemon (GNOME Keyring, KDE Wallet, etc.).
- **AppData location** follows the [XDG Base Directory Specification](https://specifications.freedesktop.org/basedir-spec/latest/).

---

## Compliance Checklist

- [x] No telemetry or analytics
- [x] No user accounts or registration
- [x] No external network requests by default
- [x] Data encrypted at rest
- [x] Platform-native key storage
- [x] Memory buffer clearing (`zeroize`)
- [x] Zero `unsafe` Rust blocks
- [x] Configurable AI provider (default: localhost only)
- [x] Full data export and deletion
- [x] Open-source (MIT license)

---

*Questions or concerns? Open an issue or contact the maintainer.*
