# CLI structure

Pkiboo's primary objects are certificates, keys, and media. A certificate is not
classified as a root or intermediate by its CLI object type: its issuer
relationship determines its place in the certificate hierarchy.

Recovery splits belong to keys and are managed beneath `key split`. Remote
storage is a media backend, not a separate object type. Locations and other
operator-defined classifications are ordinary metadata.

```text
pkiboo
├── status
│   Show a concise health summary: certificates, key recoverability, media
│   status, stale verifications, and warnings.
│
├── cert
│   ├── create --name <name> [--csr <csr-file>] [--key <key>] [--by <issuer-cert>]
│   │   Create a certificate. With --by, validate and sign the request using
│   │   the named issuer certificate. Without --by, create a self-signed root
│   │   certificate using the identified key.
│   │
│   ├── list
│   │   List managed certificates, including their issuer relationships and
│   │   current status.
│   │
│   ├── show --cert <cert> [--pem]
│   │   Show a certificate's subject, issuer, serial, fingerprint, validity,
│   │   associated key, policy, and status.
│   │   With --pem, print only the PEM-encoded public certificate.
│   │
│   ├── export
│   │   Export a public certificate in PEM or DER form.
│   │
│   ├── verify
│   │   Verify the certificate and, when its key is available, confirm that
│   │   the certificate and key correspond.
│   │
│   ├── meta
│   │   Show, set, or remove metadata on a certificate.
│   │
│   └── retire
│       Mark a certificate as unavailable for new issuance while retaining
│       its certificate, relationships, and history.
│
├── key
│   ├── create
│   │   Generate a new key and write it directly to explicitly selected media.
│   │   Retain only public information and storage metadata locally.
│   │
│   ├── list
│   │   List managed keys and their availability and recoverability state.
│   │
│   ├── show --key <key> [--pem]
│   │   Show a key's fingerprint, public information, complete copies,
│   │   recovery splits, and health.
│   │   With --pem, print only the PEM-encoded public key.
│   │
│   ├── backup --key <key> --media <destination-media>
│   │   Make another complete copy of a key directly from its current media
│   │   to explicitly selected destination media.
│   │
│   ├── split
│   │   ├── create
│   │   │   Create a threshold recovery set and place each share onto an
│   │   │   independent destination. Never collect all shares in one ordinary
│   │   │   local directory.
│   │   │
│   │   ├── list
│   │   │   List recovery splits, optionally restricted to one key.
│   │   │
│   │   ├── show
│   │   │   Show a split's threshold, share placements, metadata, and whether
│   │   │   quorum is achievable.
│   │   │
│   │   ├── verify
│   │   │   Verify split metadata and optionally exercise reconstruction
│   │   │   without retaining the reconstructed key.
│   │   │
│   │   ├── meta
│   │   │   Show, set, or remove metadata on a recovery split.
│   │   │
│   │   └── retire
│   │       Stop counting a recovery split as an active recovery mechanism.
│   │
│   ├── verify
│   │   Verify a complete key copy or recovery path against the expected
│   │   public-key fingerprint.
│   │
│   └── meta
│       Show, set, or remove metadata on a key.
│
├── media
│   ├── create --name <name> (--path <mount> | --device <block-device>)
│   │   Register and initialize physical media. --path uses an existing mount;
│   │   --device identifies the filesystem block device and lets Pkiboo mount
│   │   it through UDisks after safety checks pass. Storage attached over USB,
│   │   Thunderbolt, or FireWire whose kernel removable bit is unset requires
│   │   --allow-external-bus. SD cards are accepted when the kernel marks them
│   │   removable; ambiguous SDIO/MMC devices remain fixed because they may be
│   │   internal eMMC storage.
│   │
│   ├── list
│   │   List all registered media, regardless of backend.
│   │
│   ├── show [--contents]
│   │   Show media identity, backend-specific details, metadata, contents,
│   │   and last verification. With --contents, display only its contents.
│   │
│   ├── inspect
│   │   Inspect discoverable media without registering or modifying it.
│   │
│   ├── sync
│   │   Synchronize public metadata and certificates onto the medium.
│   │
│   ├── verify
│   │   Read back and validate all pkiboo material expected on the medium.
│   │
│   ├── repair
│   │   Restore a damaged medium's structure and public metadata where
│   │   possible without silently replacing missing secret material.
│   │
│   ├── meta
│   │   Show, set, or remove metadata on a medium. Physical location,
│   │   custodian, and failure domain are examples rather than fixed fields.
│   │
│   ├── rename
│   │   Change the friendly name of registered media.
│   │
│   ├── retire
│   │   Mark media as intentionally no longer in service.
│   │
│   └── forget
│       Remove media from inventory after explicit acknowledgement that
│       pkiboo will no longer count its contents toward recoverability.
│
└── paper
    ├── list
    │   List registered paper artifacts.
    │
    ├── show
    │   Show what a paper artifact contains, along with its metadata and
    │   verification state.
    │
    ├── scan
    │   Scan or read a printed pkiboo artifact and contribute it to recovery.
    │
    ├── import
    │   Import a generated PDF or scanned file instead of using a live scanner.
    │
    ├── verify
    │   Verify that a paper artifact is readable and internally valid.
    │
    ├── meta
    │   Show, set, or remove metadata on a paper artifact. Its physical storage
    │   location can be recorded here.
    │
    └── forget
        Remove a lost or destroyed paper artifact from inventory.
```

## Certificate creation

`cert create` uses one certificate path for both roots and certificates issued
by another CA:

```text
pkiboo cert create --csr <csr-file> --by <issuer-cert>
```

- `--by` identifies the certificate whose private key signs the new
  certificate. If it is omitted, the new certificate is self-signed and is
  therefore a root certificate.
- `--name` is the stable name used to identify the certificate in Pkiboo. It
  is independent of subject fields such as the common name.
- `--key` explicitly identifies the subject key.
- When `--csr` is supplied without `--key`, Pkiboo identifies the managed key
  from the CSR's requested public key.
- When both `--csr` and `--key` are supplied, the CSR public key must match the
  named key.
- A self-signed certificate still requires an identified subject key, either
  explicitly through `--key` or by looking it up from the CSR.
- The issuer key and subject key are separate. For an issued certificate,
  `--by` selects the issuer certificate and therefore the signing key; `--key`
  or the CSR selects the key certified by the new certificate.
- CSR contents remain untrusted requests. Certificate policy determines the
  extensions and authority actually granted.

Private-key material needed by this command is loaded from media only for the
operation. It is never relocated into ordinary local storage.

## Generic metadata

Certificates, keys, recovery splits, media, and paper artifacts expose the same
metadata interface:

```text
pkiboo <object> meta <object-name> show [<key>...]
pkiboo <object> meta <object-name> set <key> <value>
pkiboo <object> meta <object-name> remove <key>
```

This replaces dedicated location and custody commands. Pkiboo may recommend
well-known metadata keys later, but they do not need dedicated object types.

## Deferred command groups

`audit` and general-purpose `db` administration are intentionally not included
in the initial command surface yet. Their useful behavior is partly covered by
`status` and object-specific `verify` commands. They can be added once concrete
workflows establish which additional operations users need.

Configuration commands are also deferred until Pkiboo has user-editable
configuration that cannot be expressed as object metadata or ordinary command
options.
