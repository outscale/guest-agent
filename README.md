## **guest-agent**

[![Project Stage](https://docs.outscale.com/fr/userguide/_images/Project-Sandbox-yellow.svg)](https://docs.outscale.com/en/userguide/Open-Source-Projects.html) [![](https://dcbadge.limes.pink/api/server/HUVtY5gT6s?style=flat&theme=default-inverted)](https://discord.gg/HUVtY5gT6s)

---

## 🌐 Links

- Documentation: <https://docs.outscale.com/en/>
- Project website: <https://github.com/outscale/guest-agent>
- Join our community on [Discord](https://discord.gg/HUVtY5gT6s)

---

## 📄 Table of Contents

- [Overview](#-overview)
- [Requirements](#-requirements)
- [Installation](#-installation)
- [Usage](#-usage)
- [License](#-license)
- [Contributing](#-contributing)

---

## 🧭 Overview

**guest-agent** is the OUTSCALE guest agent: a small daemon that runs inside a
VM and speaks with host.

---

## ✅ Requirements

- Rust toolchain (edition 2024)
- CMake + C toolchain (bundled libzmq build via `zmq-src`)
- Libclang (`libclang-dev` on Debian-like, `clang-devel` on Fedora)
- libsodium headers and static archive (`libsodium.a`; `libsodium-dev` on Debian/Ubuntu)
- Linux guest with systemd (for packaged installs)
- `cargo-deb` / `cargo-generate-rpm` only if building packages

---

## ⚙ Installation

### Option 1: Download from Releases

Download the latest `.deb` / `.rpm` from the
[Releases page](https://github.com/outscale/guest-agent/releases).
Packages are Sigstore-signed; see
[docs/signature-verification.md](docs/signature-verification.md) to verify them.

### Option 2: Build from source

```bash
git clone https://github.com/outscale/guest-agent.git
cd guest-agent
cargo build --release -p guest-agent
```

Binary: `target/release/guest-agent`.

### Option 3: Package

```bash
# Debian / Ubuntu
cargo deb -p guest-agent

# RPM (no rpmbuild; needs a prior release build)
cargo build --release -p guest-agent
cargo generate-rpm -p guest-agent
```

---

## 🚀 Usage

```bash
# foreground
./target/release/guest-agent

# packaged
sudo systemctl enable --now guest-agent
```

Snapshot hooks live under `/etc/guest-agent/` and follow `run-parts` naming.
Examples ship in `contrib/snapshot-hooks/`.

---

## 📜 License

**guest-agent** is released under the BSD 3-Clause license.

© 2025 Outscale SAS

See [LICENSE](./LICENSE) for full details.

---

## 🤝 Contributing

We welcome contributions!

Please read our [Contributing Guidelines](CONTRIBUTING.md) and
[Code of Conduct](CODE_OF_CONDUCT.md) before submitting a pull request.
