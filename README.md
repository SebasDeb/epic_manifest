# epic_manifest

A small Rust library that reads the binary `.manifest` files used by Epic
Games' BuildPatchServices (the format behind the Epic launcher and Unreal
builds). It was written from scratch as a learning project, checked against
EpicManifestParser (C#), egs-api-rs and legendary.

## Usage

```rust
let manifest = epic_manifest::Manifest::from_file("game.manifest")?;
println!("{} {}", manifest.meta.app_name, manifest.meta.build_version);
```

To print a summary of a manifest (app name, build version, file count, total
size and the first 20 files):

```
cargo run --example dump -- path/to/game.manifest
```

The library logs through the [`log`](https://docs.rs/log) facade and never
chooses a logger itself. The example installs a colored `env_logger`; set
`RUST_LOG=debug` to see each parsing step.

`Manifest::chunk_url(&chunk, base_url)` builds the download URL of a chunk as
`{base_url}/{ChunksVn}/{group:02}/{hash:016X}_{guid}.chunk`. The directory comes
from the meta feature level (`Chunks` to `ChunksV5`). The base URL is the
caller's to provide.

Real manifests are not part of this repository. If you want to run the
example against one, a good place to keep it is `tests/fixtures/`, which is
git-ignored.

## What it reads

- The header, including the extra fields that appear from version 22.
- zlib-compressed bodies, with the SHA-1 of the body checked against the header.
- The meta, chunk list, file list and custom fields sections. Each section is
  skipped to its declared end, so newer manifests with extra fields still parse.

## What it does not do yet

- JSON manifests.
- Encrypted manifests: the parser returns `Error::Encrypted`.
- Downloading chunks or rebuilding files.
- The encrypted-data section of version 23 and later.
- Computing a build id when the manifest does not store one.
- The base64 chunk path with a `plain`/secret directory that Legendary uses for
  version 22 and later. The hex path below is used for every version instead.
- Oodle, async or a command line beyond the example.

## Roadmap

The order below is the order I plan to work in. Nothing here is started yet.

1. Compute the build id when the manifest does not store one.
2. Check the parser against real manifests from more games and versions.
3. JSON manifests, which older builds use. This needs `serde_json`.
4. Encrypted manifests (AES-GCM).
5. Downloading chunks and rebuilding the files on disk, verifying the hashes
   on the way.

## Development

```
cargo test
cargo clippy --all-targets
```
