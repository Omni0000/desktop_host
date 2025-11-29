
use std::path::Path ;
use std::io::Cursor ;
use std::fs ;
use capnp::message::ReaderOptions ;
use capnp::serialize ;

use crate::manifest_capnp ;
use super::MissingManifestErr ;
use super::plugin::Plugin ;
use super::plugin_id::PluginId ;
use super::DecoderError ;



pub fn get_manifest(

    plugin_dir: &Path,

) -> Result<Plugin, DecoderError> {

    let manifest_path = plugin_dir.join( "manifest.bin" );
    let wasm_path = plugin_dir.join( "source.wasm" );

    let file_contents = fs::read( &manifest_path )
        .map_err(| err | MissingManifestErr::new( plugin_dir.into(), err ) )?;
    let data = Cursor::new( file_contents );

    let reader = serialize::read_message( data, ReaderOptions::new())
        .map_err(| err | DecoderError::InvalidManifestErr( err ))?;
    let root = reader.get_root::<manifest_capnp::plugin_metadata::Reader>()
        .map_err(| err | DecoderError::InvalidManifestErr( err ))?;

    let plugin_id = PluginId::try_from( root.get_id().map_err(| err | DecoderError::InvalidManifestErr( err ))? )
        .map_err(| err | DecoderError::InvalidManifestErr( err ))?;

    Plugin::try_new(
        plugin_id,
        reader,
        wasm_path,
    ).map_err(| err | DecoderError::InvalidManifestErr( err ))

}