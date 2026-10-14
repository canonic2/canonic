# Catalog format

A registry is a directory containing `catalog.json` and the artifact files it
lists. Serve the directory locally or under an HTTPS base URL. HTTPS registries
also serve `catalog.sig` and `catalog-key.pub`.

## Version 1 catalog

```json
{
  "schema_version": 1,
  "registry_id": "acme-team",
  "sequence": 4,
  "expires_at": "2027-01-01T00:00:00Z",
  "packs": [
    {
      "id": "acme/git",
      "version": "1.2.0",
      "path": "packs/git-1.2.0.yml",
      "sha256": "REPLACE_WITH_THE_ARTIFACT_SHA256"
    }
  ]
}
```

Replace the digest placeholder with the SHA-256 of the actual file bytes; the
document as shown is illustrative and will be refused. Choose a future expiry
when publishing.

| Field | Required | Rule |
| --- | --- | --- |
| `schema_version` | Always | Integer `1` |
| `packs` | Always | Array of at most 64 release entries; may be empty |
| `registry_id` | HTTPS | Stable non-blank ID, at most 4096 bytes, without NUL |
| `sequence` | HTTPS | Non-negative `u64` publication counter |
| `expires_at` | HTTPS | Valid whole-second UTC `YYYY-MM-DDTHH:MM:SSZ` |

Local catalogs may omit the three trust metadata fields. A present expiry must
still be well formed, but expiry is enforced only for signed sources.
Once a client has admitted a sequenced local catalog, that source must keep
publishing sequences; omitting one is a rollback refusal for that client.
Unknown fields are refused in the document and release entries. A bad release
invalidates the entire catalog.

## Release entries

Every entry requires `id`, `version`, `path` and `sha256`.

- `id` is `publisher/name`. Each segment is 1–64 lowercase ASCII letters,
  digits or hyphens. `acme/git-tools` is valid; `Acme/git_tools` is not.
- `version` is an exact SemVer version, including optional prerelease or build
  metadata. Version ranges belong to dependencies, not catalog entries.
- `path` is relative to the registry root and at most 4096 bytes.
- `sha256` is exactly 64 hex characters without whitespace. Either case is
  accepted and normalized lowercase during verification.

A pack/version pair may appear only once. Several versions of one pack and
several publishers using the same bare name are allowed.

Paths use ASCII letters, digits and `-._~/`. Empty segments, `.` and `..`,
trailing dots and Windows device names are refused. This includes `CON`, `PRN`,
`AUX`, `NUL`, `COM1`–`COM9` and `LPT1`–`LPT9`, case-insensitively and with any
extension. Spaces, backslashes, percent encoding and absolute paths are invalid.

The artifact extension has no meaning to Registry. The consuming application
defines the format and reads dependency metadata from it; the catalog has no
dependency field.

## Signature files

| File | Format |
| --- | --- |
| `catalog-key.pub` | 32-byte Ed25519 public key encoded as 64 hex characters |
| `catalog.sig` | 64-byte Ed25519 signature encoded as 128 hex characters |

Both allow surrounding whitespace and either hex case. The signature covers
the exact `catalog.json` bytes, including formatting and final newline. Signing
one serialization and serving another will fail verification. Each trust file
is bounded to 4 KiB; the catalog is bounded to 1 MiB.

Next: [Hosting and signing](hosting-and-signing.md).

[Documentation index](../README.md)
