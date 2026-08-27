# Pkiboo

## Overview

**Pkiboo** (pronounced “peek-a-boo”) is a Rust CLI for managing **offline and highly available PKI key material**.

Its job is to make a normally awkward security model practical:

- important CA private keys live on removable/offline media;
- multiple independent copies can provide availability;
- Shamir Secret Sharing provides disaster recovery;
- certificates and metadata are freely replicated because they are not secret;
- operations requiring a private key interactively locate and load appropriate media;
- external systems can submit CSRs for signing without ever receiving the signing key.

Pkiboo is **not an online certificate authority service** and is not tied to any particular infrastructure stack.

It is a generic tool for safely operating the offline portions of a PKI.

---

# Design Philosophy

Pkiboo separates three things that are often unnecessarily bundled together:

```text
Certificates
    public
    freely replicated

Private keys
    secret
    stored on controlled media

Recovery shares
    secret
    independently distributed
```

The central principle is:

> **Certificates are data. Private keys are capabilities.**

Pkiboo therefore concentrates its security model around private-key possession rather than trying to make all PKI state secret.

---

# PKI Model

Pkiboo manages certificate authorities and their associated keys.

A typical hierarchy might be:

```text
Root CA
   │
   ├── Intermediate A
   │       ├── ...
   │
   └── Intermediate B
           ├── ...
```

Pkiboo is particularly intended for CAs whose private keys should remain offline.

A root consists conceptually of:

```text
CA
├── private key
├── certificate
├── certificate policy
└── metadata
```

The certificate is public.

The private key is the valuable secret.

Pkiboo should therefore not require the private key to reside permanently on the workstation running Pkiboo.

---

# Keys and Certificates

Keys and certificates have independent lifecycles.

A certificate refers to a public key and describes the authority granted to it.

The corresponding private key may be stored elsewhere entirely.

For example:

```text
CA certificate
    │
    ├── workstation
    ├── USB backup
    ├── trust stores
    └── arbitrary public backups

CA private key
    │
    ├── removable medium A
    └── removable medium B
```

This distinction should be reflected throughout the Pkiboo object model.

Certificates can be copied aggressively.

Private keys cannot.

---

# Private-Key Storage

Pkiboo supports keeping private keys on removable media rather than in its ordinary local state.

An operation requiring a key can therefore look like:

```text
$ pkiboo ...

Private key required: production-root

Searching for media...

Insert media containing this key.

✓ Media detected
✓ Key found
✓ Key identity verified

Continuing...
```

The private key should be loaded only for the duration of the operation requiring it.

Sensitive values should be protected in memory where practical and should not casually pass through temporary files.

---

# Media

Pkiboo maintains an abstraction for physical or logical **media** capable of storing PKI material.

Removable USB storage is the initial important implementation, but the core PKI model should not depend specifically on USB.

A medium has:

- a stable Pkiboo identifier;
- descriptive metadata;
- information about the PKI objects stored on it;
- a manifest.

For example:

```text
Media: 01J...
Label: root-home

Contains:
  certificate: production-root
  private-key: production-root
```

Device names such as `/dev/sdb` are not stable identities and should not be treated as such.

Where possible, Pkiboo should use persistent hardware/filesystem metadata.

---

# Manifests

Pkiboo keeps a manifest describing known media and PKI objects.

The manifest is **metadata, not a secret**.

Copies of relevant metadata should be stored on the media themselves.

This gives Pkiboo an important recovery property:

> Loss of Pkiboo's local database should not imply loss of the PKI inventory.

Given surviving media, Pkiboo should be able to rediscover and reconstruct as much of its state as possible.

The manifest might conceptually describe:

```text
media
├── ID
├── label
├── device metadata
└── objects
     ├── certificates
     ├── keys
     └── recovery shares
```

It should never contain raw private-key material.

---

# Media Discovery and Mounting

On Linux, the initial implementation can use:

- udev/sysfs for block-device discovery and metadata;
- UDisks2 over D-Bus for mounting and unmounting filesystems.

Pkiboo must coexist cleanly with desktop automounters.

If a filesystem is already mounted, Pkiboo can use that mount.

If Pkiboo itself mounts a filesystem, it may unmount it after the operation.

It should **not unmount a filesystem mounted by somebody else**.

Writes to removable media should be explicitly synchronized before Pkiboo reports that the medium can safely be removed.

---

# Creating PKI Media

Pkiboo can prepare media for storing PKI objects.

Conceptually:

```text
pkiboo media create ...
```

The workflow is approximately:

```text
identify device
      │
      ▼
confirm destructive operation if necessary
      │
      ▼
prepare storage
      │
      ▼
assign media identity
      │
      ▼
write manifest
      │
      ▼
register locally
```

Interactive destructive operations should be extremely difficult to perform accidentally.

---

# Creating a Root CA

Pkiboo can create a new root CA.

Conceptually:

```text
pkiboo root create
```

The operation generates:

```text
private key
     │
     ├──────────► public key
     │
     ▼
self-signed root certificate
```

The user chooses the CA's relevant X.509 properties and key parameters.

Pkiboo then stores the resulting artifacts according to the requested storage configuration.

The root certificate may be written anywhere.

The root private key should only be written to explicitly selected secure destinations.

A successful root creation should not accidentally leave an ordinary private-key file behind on the machine running Pkiboo.

---

# Multiple Complete Copies

Pkiboo distinguishes **availability backups** from **disaster recovery**.

The simplest availability mechanism is multiple complete copies of a private key:

```text
             private key
             /         \
            /           \
       Medium A       Medium B
```

These copies should normally exist in different physical failure domains.

Either one can be used independently for a signing operation.

This protects against ordinary failures such as:

- failed flash storage;
- lost media;
- physical destruction of one storage location.

Pkiboo should keep track of which registered media are expected to contain which keys.

---

# Shamir Secret Sharing

Pkiboo additionally supports Shamir Secret Sharing for catastrophic recovery.

For example:

```text
                private key
                    │
                  split
                    │
       ┌────┬────┬──┴─┬────┐
       ▼    ▼    ▼    ▼    ▼
      S1   S2   S3   S4   S5

             threshold = 3
```

Any three shares can reconstruct the private key.

This is distinct from ordinary complete-key backups.

A critical invariant is:

> **Pkiboo must never create a convenient bundle containing all of the recovery shares.**

Doing so would defeat the security property provided by secret sharing.

---

# Creating Recovery Shares

The split procedure should ensure shares are delivered independently.

An interactive workflow could look like:

```text
$ pkiboo key split ...

Creating 3-of-5 recovery set.

Share 1/5
Insert destination media...
✓ written

Remove media to continue.

Share 2/5
Insert destination media...
✓ written

...
```

Batch operation may also be possible, but it must still require independent destinations rather than creating five files together in an ordinary directory by default.

Potential share destinations may include:

- removable storage;
- printable/paper representations;
- encrypted remote storage.

Remote storage is reasonable for individual shares provided the overall recovery design maintains independent security domains.

---

# Recovery

Pkiboo should make reconstruction ergonomic.

Instead of requiring the operator to manually locate filenames and concatenate shares, Pkiboo can discover available shares.

For example:

```text
$ pkiboo key recover ...

Required: 3 of 5 shares

[✓] share 1
[ ] share 2
[✓] share 3
[ ] share 4
[ ] share 5

Insert another recovery medium...

[✓] share 4

Threshold reached.
Reconstructing key...
```

If enough media are already available, reconstruction can proceed immediately.

The reconstructed key should normally exist only temporarily.

For example:

```text
shares
  │
  ▼
reconstructed key in memory
  │
  ├── sign something
  │
  └── recreate lost backup media
```

Reconstruction should not automatically result in a permanent plaintext key file.

---

# Signing

A central Pkiboo operation is signing a CSR using an offline CA key.

Conceptually:

```text
CSR
 │
 ▼
Pkiboo
 │
 ├── validate CSR
 ├── load CA policy
 ├── request required key media
 ├── verify private key
 └── issue certificate
 │
 ▼
certificate
```

The requester never receives the CA private key.

This allows Pkiboo to serve as the human-operated bridge between offline signing authority and systems capable of generating their own keys and CSRs.

---

# CSR Validation

A CSR is an **untrusted request**, not a specification Pkiboo must blindly obey.

Pkiboo should validate relevant properties including:

- CSR cryptographic signature;
- public-key algorithm;
- key strength;
- subject;
- requested CA status;
- Basic Constraints;
- path-length constraints;
- Key Usage;
- Extended Key Usage where relevant;
- requested extensions;
- requested lifetime.

The certificate ultimately issued should be determined by:

```text
CSR request
     +
Pkiboo CA policy
     =
issued certificate
```

The CA policy is authoritative.

A requester must not be able to gain additional authority merely by requesting dangerous extensions.

---

# Integrations

Pkiboo should support integrations with external certificate systems.

An integration is responsible for moving requests and resulting certificates across the offline/online boundary.

Conceptually:

```text
External system
      │
      │ CSR
      ▼
Integration
      │
      ▼
Pkiboo signing workflow
      │
      │ certificate
      ▼
Integration
      │
      ▼
External system
```

The integration should reuse Pkiboo's normal signing and policy machinery rather than implement a separate signing path.

---

# OpenBao Integration

**OpenBao is one supported integration**, not part of Pkiboo's core architecture.

An OpenBao instance may generate an intermediate CA private key and corresponding CSR.

Pkiboo can retrieve that CSR, sign it with an appropriate offline CA, and return the resulting certificate.

Conceptually:

```text
OpenBao
   │
   │ generate private key
   │ generate CSR
   ▼
pending CSR
   │
   ▼
Pkiboo OpenBao integration
   │
   ▼
normal Pkiboo signing workflow
   │
   ▼
signed certificate
   │
   ▼
OpenBao
```

A command might eventually look like:

```text
pkiboo bao update
```

It could:

1. connect to configured OpenBao instances;
2. discover outstanding CA CSRs;
3. determine the appropriate signing CA;
4. validate each CSR;
5. locate the required offline signing key;
6. ask the operator to insert appropriate media when necessary;
7. sign the CSR;
8. return the certificate to OpenBao.

The OpenBao integration should contain **OpenBao-specific transport and API logic only**.

It should not contain special PKI semantics that bypass the generic Pkiboo model.

Other integrations should be possible later.

---

# Local State

Pkiboo's ordinary local state should contain metadata rather than private secrets.

For example:

```text
Pkiboo state
│
├── CAs
│   ├── certificates
│   ├── policies
│   └── metadata
│
├── keys
│   ├── public information
│   ├── fingerprints
│   └── storage locations
│
├── media
│   ├── identities
│   └── expected contents
│
├── recovery sets
│   └── share metadata
│
└── integrations
    └── configuration
```

Storing a cryptographic fingerprint identifying a private key is acceptable and useful.

The fingerprint is not the private key and allows Pkiboo to verify that inserted media contains the expected key.

---

# CLI Design

Pkiboo is primarily a **CLI application**.

It should be pleasant for interactive use without becoming a full-screen TUI.

Interactive operation may use:

- colors;
- bold text;
- warnings;
- progress bars;
- Unicode status indicators;
- prompts;
- automatic removable-media discovery.

For example:

```text
$ pkiboo sign request.csr

Signing CA: production-root
Key: unavailable

Waiting for media...

✓ root-backup-2 detected
✓ private key verified

Validating CSR...
✓ signature
✓ public key
✓ requested extensions

Signing...
✓ certificate issued
```

When stdout is not a terminal, output should remain suitable for scripting.

Interactive status/progress information should generally go to stderr so stdout can be reserved for requested machine-readable or artifact output.

---

# Rust Architecture

Pkiboo is implemented in Rust.

Likely major components are:

```text
CLI
 │
 ▼
workflows
 │
 ├──────────────┬──────────────┐
 ▼              ▼              ▼
PKI           Media       Integrations
 │              │              │
 ▼              ▼              ▼
OpenSSL      OS/backend      OpenBao
```

Important implementation choices currently include:

- `clap` for CLI parsing;
- Rust `openssl` bindings for cryptographic/X.509 operations;
- `secrecy` wrappers for sensitive in-memory data where useful;
- serde for persistent metadata;
- TOML for human-readable manifests/configuration;
- async Rust for media discovery and integrations;
- D-Bus/UDisks2 for Linux filesystem operations;
- udev/sysfs for device discovery.

Pkiboo should **not implement cryptographic primitives itself**.

OpenSSL performs the cryptographic operations; Pkiboo provides policy, lifecycle management, storage orchestration, and UX.

---

# Backend Abstractions

Media access should be abstracted from workflows.

For example, a `Media` trait may provide operations for:

```text
identify
open manifest
read object
write object
sync
mount/unmount where applicable
```

This is important both architecturally and for testing.

The signing code should not care whether a key came from:

```text
USB filesystem
test backend
future remote backend
other secure storage
```

Likewise, integrations should be adapters around the generic signing workflow.

---

# Testing Philosophy

Pkiboo should be highly testable without requiring actual removable drives or valuable keys.

A fake media backend should support scenarios such as:

```text
media absent
    │
media inserted
    │
manifest discovered
    │
key loaded
    │
operation performed
    │
media removed
```

Integration tests can use deliberately insecure test-only CA keys and certificates.

Those artifacts may be committed to a test environment when they have no authority outside that environment.

Important tests include:

- root creation;
- certificate generation;
- CSR validation;
- signing;
- wrong-key detection;
- media discovery;
- media removal;
- multiple backup copies;
- Shamir splitting;
- threshold reconstruction;
- insufficient-share handling;
- manifest reconstruction;
- integration CSR retrieval and certificate return.

---

# Security Invariants

An implementation should preserve these invariants:

1. **Certificates are not treated as secrets.**
2. **Pkiboo local state does not need to contain CA private keys.**
3. **Private keys are only written to explicitly selected destinations.**
4. **Operations requiring a private key verify that the supplied key is the expected one.**
5. **Recovery shares are distributed independently.**
6. **Pkiboo never casually materializes all Shamir shares together.**
7. **A reconstructed key is temporary unless explicitly restored.**
8. **CSRs are untrusted input.**
9. **CA policy, not CSR contents, determines granted authority.**
10. **Integrations cannot bypass the normal signing and policy path.**
11. **Pkiboo does not implement its own cryptographic primitives.**
12. **Loss of local Pkiboo metadata should be recoverable as far as practical from surviving media and public PKI artifacts.**

---

# Scope Boundary

Pkiboo owns:

```text
CA/key lifecycle
offline private-key storage
removable-media management
backup copies
Shamir recovery
CSR validation
certificate signing
certificate policy
integration adapters
```

Pkiboo does **not** own:

```text
application deployment
cluster orchestration
service certificate rotation
machine provisioning
operating-system configuration
general secret management
network architecture
```

Those systems may use certificates ultimately signed through Pkiboo, but they are consumers of the PKI rather than part of Pkiboo itself.

---

# Short Definition

**Pkiboo is a Rust CLI for operating offline PKI: it manages CA keys across independent storage and recovery media, validates and signs certificate requests under explicit policy, and provides adapters that let online certificate systems interact safely with offline signing authorities.**

The goal is to make a strong offline-key architecture **operationally pleasant without weakening the offline boundary**.