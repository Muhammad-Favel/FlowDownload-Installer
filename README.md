# FlowDownload Installer (Phase 1 CLI Spike)

Cross-platform **Windows + macOS** CLI that fetches the latest `FlowDownload.ipa` from `source.json`, verifies SHA-256, inspects embedded `.appex` extensions, and prepares/sideloads over USB.

## Commands

```bash
# Check latest release from source.json
cargo run -- check

# View 3-App-ID / 2-App-ID / 1-App-ID trade-offs
cargo run -- trade-offs

# Download, verify SHA-256, and prepare IPA for a profile (full | share-only | core)
cargo run -- install --profile core

# List connected USB devices (requires --features sideload)
cargo run --features sideload -- devices
```
