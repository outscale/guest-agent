# Signature Verification

All `guest-agent` release packages are signed using [Sigstore](https://www.sigstore.dev/)
and can be verified cryptographically. This ensures the packages you download are
authentic and have not been tampered with.

---

## Overview

Each [GitHub release](https://github.com/outscale/guest-agent/releases) ships, alongside
the `.deb` / `.rpm` packages:

- A **checksums file** (`guest-agent_<version>_checksums.txt`) with SHA-256 hashes of all release artifacts
- A **signature bundle** (`guest-agent_<version>_checksums.txt.sigstore.json`) containing the
  cryptographic signature and verification metadata for the checksums file

The signature uses the Sigstore infrastructure:

- **Fulcio** for certificate issuance (OIDC-based short-lived certificates)
- **Rekor** for transparency log entries
- **GitHub Actions OIDC** for identity verification

---

## Manual Verification

Verify signatures with the `cosign` CLI tool.

### Step 1: Download Required Files

Download the package plus the two signature files from the
[releases page](https://github.com/outscale/guest-agent/releases):

- `guest-agent_<version>_<os>_<arch>.deb` (or `.rpm`)
- `guest-agent_<version>_checksums.txt`
- `guest-agent_<version>_checksums.txt.sigstore.json`

Example for Debian/Ubuntu x86_64:

```sh
VERSION="0.1.0"
curl -LO "https://github.com/outscale/guest-agent/releases/download/${VERSION}/guest-agent_${VERSION}-1_amd64.deb"
curl -LO "https://github.com/outscale/guest-agent/releases/download/${VERSION}/guest-agent_${VERSION}_checksums.txt"
curl -LO "https://github.com/outscale/guest-agent/releases/download/${VERSION}/guest-agent_${VERSION}_checksums.txt.sigstore.json"
```

### Step 2: Verify the Checksums File Using Cosign

The signature bundle can be verified using Cosign v2.0 or later:

```sh
cosign verify-blob \
  --bundle "guest-agent_${VERSION}_checksums.txt.sigstore.json" \
  --certificate-oidc-issuer "https://token.actions.githubusercontent.com" \
  --certificate-identity-regexp "^https://github.com/outscale/guest-agent/" \
  "guest-agent_${VERSION}_checksums.txt"
```

Expected output:

```
Verified OK
```

### Step 3: Verify the Package Against the Checksums

Once the checksums file is verified, check your package against it:

```sh
sha256sum -c "guest-agent_${VERSION}_checksums.txt" --ignore-missing
```

Expected output:

```
guest-agent_<version>_<os>_<arch>.deb: OK
```

### Certificate Identity Details

| Field | Value |
|-------|-------|
| Issuer | `https://token.actions.githubusercontent.com` |
| Subject | Starts with `https://github.com/outscale/guest-agent/` |

This confirms the package was built by the official `outscale/guest-agent` GitHub
repository's release workflow.

---

## Installing Sigstore Tools

Cosign is the recommended tool for verifying Sigstore signatures:

```sh
# Debian / Ubuntu (download binary directly)
curl -O -L "https://github.com/sigstore/cosign/releases/latest/download/cosign-$(uname -m)-pc-linux-gnu"
chmod +x cosign-$(uname -m)-pc-linux-gnu
sudo mv cosign-$(uname -m)-pc-linux-gnu /usr/local/bin/cosign
```

See the [Cosign installation guide](https://docs.sigstore.dev/cosign/system_config/installation/)
for more options.

---

## Further Reading

- [Sigstore Documentation](https://docs.sigstore.dev/)
- [Cosign Documentation](https://docs.sigstore.dev/cosign/overview/)
- [octl Signature Verification](https://github.com/outscale/octl/blob/main/docs/signature-verification.md)
