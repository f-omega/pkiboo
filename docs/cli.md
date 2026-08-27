pkiboo
├── init
│   Initialize a new pkiboo repository/database and local configuration.
│
├── status
│   Show a concise health summary: roots, key recoverability, media status,
│   stale verifications, and warnings.
│
├── root
│   ├── create
│   │   Create a new self-signed root CA and its private key.
│   │
│   ├── list
│   │   List managed root CAs.
│   │
│   ├── show
│   │   Show one root's certificate, fingerprint, validity, key custody,
│   │   recovery options, and issued intermediates.
│   │
│   ├── export
│   │   Export the public root certificate in PEM/DER form.
│   │
│   ├── verify
│   │   Verify the root certificate and, when available, key/cert correspondence.
│   │
│   └── retire
│       Mark a root as no longer used for new intermediate issuance while
│       retaining it for trust/recovery/history.
│
├── key
│   ├── list
│   │   List managed private keys and their recoverability state.
│   │
│   ├── show
│   │   Show key fingerprint, owner/root, complete backups, splits, and health.
│   │
│   ├── backup
│   │   Create a complete encrypted backup of a private key directly onto
│   │   removable media, paper, or an approved remote destination.
│   │
│   ├── split
│   │   Create a threshold recovery split, e.g. 3-of-5, and interactively
│   │   place each share onto a distinct destination.
│   │
│   ├── recover
│   │   Recover a private key from a complete backup or by collecting enough
│   │   split shares. Watches for inserted media and tracks quorum progress.
│   │
│   ├── verify
│   │   Verify that known recovery paths can still reconstruct the expected key.
│   │
│   └── destroy-local
│       Securely remove any temporary/local plaintext copy after a ceremony.
│
├── media
│   ├── create
│   │   Register and initialize removable media as a pkiboo destination.
│   │   Verifies that the backing device is actually removable.
│   │
│   ├── list
│   │   List all registered removable media and known locations.
│   │
│   ├── show
│   │   Show media identity, device metadata, location, contents, and last verify.
│   │
│   ├── inspect
│   │   Inspect currently attached media without modifying it.
│   │
│   ├── contents
│   │   Show which pkiboo artifacts are known to live on the medium.
│   │
│   ├── sync
│   │   Synchronize public metadata and root certificates onto the medium.
│   │
│   ├── verify
│   │   Read back and validate all pkiboo material expected on the medium.
│   │
│   ├── set
│   │   Manage metadata on the object.
│   │
│   ├── rename
│   │   Change the friendly name of a registered medium.
│   │
│   ├── retire
│   │   Mark a medium as intentionally no longer in service.
│   │
│   └── forget
│       Remove the medium from inventory after explicit acknowledgement that
│       pkiboo will no longer count its contents toward recoverability.
│
├── paper
│   ├── list
│   │   List registered paper artifacts.
│   │
│   ├── show
│   │   Show what a paper artifact contains and where it is stored.
│   │
│   ├── scan
│   │   Scan/read a printed pkiboo artifact and contribute it to recovery.
│   │
│   ├── import
│   │   Import a generated PDF or scanned file instead of using a live scanner.
│   │
│   ├── verify
│   │   Verify that a paper artifact is readable and internally valid.
│   │
│   ├── set-location
│   │   Record where the paper copy is physically stored.
│   │
│   └── forget
│       Remove a lost/destroyed paper artifact from the inventory.
│
├── remote
│   ├── add
│   │   Register a remote share/backup destination such as S3 or B2.
│   │
│   ├── list
│   │   List configured remote destinations.
│   │
│   ├── show
│   │   Show remote type, failure domain, stored placements, and verification.
│   │
│   ├── test
│   │   Verify authentication and perform a safe write/read/delete test.
│   │
│   ├── verify
│   │   Retrieve and validate stored pkiboo artifacts without reconstructing keys.
│   │
│   ├── set-location
│   │   Associate the remote with a logical custody/failure domain.
│   │
│   └── remove
│       Remove a remote destination after checking what recoverability depends on it.
│
├── split
│   ├── list
│   │   List threshold-recovery splits.
│   │
│   ├── show
│   │   Show threshold, placements, locations, and whether quorum is achievable.
│   │
│   ├── verify
│   │   Verify split metadata and optionally exercise recovery without retaining
│   │   the recovered key.
│   │
│   └── retire
│       Stop counting an old split as an active recovery mechanism.
│
├── recover
│   ├── key
│   │   High-level guided recovery of a specific key.
│   │
│   ├── root
│   │   Recover the private key associated with a root CA, then verify it against
│   │   the public root certificate.
│   │
│   └── database
│       Rebuild pkiboo inventory from removable media, paper artifacts, and
│       remote metadata after loss of the local database.
│
├── intermediate
│   ├── issue
│   │   Validate an intermediate CA CSR, recover/access the offline root key,
│   │   sign it under policy, and emit the intermediate certificate.
│   │
│   ├── list
│   │   List intermediate CA certificates issued by managed roots.
│   │
│   ├── show
│   │   Show issuer, subject, serial, fingerprint, validity, and status.
│   │
│   ├── export
│   │   Export an issued intermediate certificate.
│   │
│   └── revoke
│       Mark an intermediate as revoked/compromised and update the root's
│       issuance/revocation records as appropriate.
│
├── location
│   ├── add
│   │   Define a logical or physical storage/custody location.
│   │
│   ├── list
│   │   List known locations and what pkiboo material is associated with them.
│   │
│   ├── show
│   │   Show media/paper/remotes stored in a location and its failure-domain role.
│   │
│   ├── rename
│   │   Rename a location without changing artifact relationships.
│   │
│   └── remove
│       Remove an unused location after ensuring no live placements still use it.
│
├── audit
│   ├── root
│   │   Audit one root's complete recoverability and operational health.
│   │
│   ├── key
│   │   Audit one private key's backups, splits, and quorum paths.
│   │
│   ├── media
│   │   Find stale, missing, unverified, or retired media.
│   │
│   ├── locations
│   │   Detect correlated-risk problems, such as quorum being satisfiable from
│   │   one building, one person, or one cloud account.
│   │
│   ├── recovery
│   │   Simulate failure scenarios and determine whether each key remains recoverable.
│   │
│   └── all
│       Run the complete pkiboo health and custody audit.
│
├── db
│   ├── info
│   │   Show database version, path, schema, and repository identity.
│   │
│   ├── backup
│   │   Back up the non-secret inventory database.
│   │
│   ├── restore
│   │   Restore a previous inventory database backup.
│   │
│   ├── rebuild
│   │   Reconstruct inventory by scanning available pkiboo artifacts.
│   │
│   └── verify
│       Check database consistency and referential integrity.
│
└── config
    ├── show
    │   Show effective configuration.
    │
    ├── get
    │   Read one configuration value.
    │
    └── set
        Change one configuration value.
