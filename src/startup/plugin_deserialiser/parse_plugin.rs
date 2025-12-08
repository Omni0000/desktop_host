use std::io::Cursor ;
use capnp::message::ReaderOptions ;
use capnp::serialize ;

use crate::capnp::manifest_capnp::plugin_metadata ;
use crate::startup::plugin_discovery::RawPluginData ;
use super::MissingManifestErr ;
use super::plugin::Plugin ;
use super::plugin_id::PluginId ;
use super::DecoderError ;



pub fn parse_plugin(

    plugin_data: RawPluginData,

) -> Result<Plugin, DecoderError> {

    let manifest_data = plugin_data.manifest().map_err(| err | MissingManifestErr::new( err, plugin_data.display_path() ) )?;

    let reader = serialize::read_message( Cursor::new( &manifest_data ), ReaderOptions::new())
        .map_err(| err | DecoderError::InvalidManifestErr( err ))?;
    let root = reader.get_root::<plugin_metadata::Reader>()
        .map_err(| err | DecoderError::InvalidManifestErr( err ))?;

    let plugin_id = PluginId::try_from( root.get_id().map_err(| err | DecoderError::InvalidManifestErr( err ))? )
        .map_err(| err | DecoderError::InvalidManifestErr( err ))?;

    Plugin::try_new(
        plugin_id,
        manifest_data,
        plugin_data,
    ).map_err(| err | DecoderError::InvalidManifestErr( err ))

}