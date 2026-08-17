//! The shipped assets, baked into the binary: the default asset source is
//! replaced with an `include_dir` snapshot of `assets/`, so a release is
//! one executable with nothing to install beside it, and the whole class
//! of "assets folder not found next to the binary" failures goes away.
//!
//! Registered before `DefaultPlugins`, because `AssetPlugin` snapshots the
//! source list as it builds; a source registered later is silently unused.

use bevy::asset::io::{
    AssetReader, AssetReaderError, AssetSourceBuilder, AssetSourceId, PathStream, Reader, VecReader,
};
use bevy::prelude::*;
use include_dir::{Dir, include_dir};
use std::path::Path;

static ASSETS: Dir = include_dir!("$CARGO_MANIFEST_DIR/assets");

struct EmbeddedAssets;

impl AssetReader for EmbeddedAssets {
    async fn read<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        ASSETS
            .get_file(path)
            .map(|file| VecReader::new(file.contents().to_vec()))
            .ok_or_else(|| AssetReaderError::NotFound(path.to_path_buf()))
    }

    /// No sidecar `.meta` files are embedded. "Not found" is the answer the
    /// filesystem reader gives when none exists, and Bevy takes defaults.
    async fn read_meta<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        Err::<VecReader, _>(AssetReaderError::NotFound(path.to_path_buf()))
    }

    async fn read_directory<'a>(
        &'a self,
        path: &'a Path,
    ) -> Result<Box<PathStream>, AssetReaderError> {
        // Nothing loads folders; the game names every asset it wants.
        Err(AssetReaderError::NotFound(path.to_path_buf()))
    }

    async fn is_directory<'a>(&'a self, path: &'a Path) -> Result<bool, AssetReaderError> {
        Ok(ASSETS.get_dir(path).is_some())
    }
}

/// Make the embedded snapshot the default asset source.
pub(super) fn register(app: &mut App) {
    app.register_asset_source(
        AssetSourceId::Default,
        AssetSourceBuilder::new(|| Box::new(EmbeddedAssets)),
    );
}
