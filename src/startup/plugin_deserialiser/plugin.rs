use std::io::Cursor ;
use capnp::message::{ self, ReaderOptions };
use capnp::serialize::{ self, OwnedSegments };

use crate::capnp::manifest_capnp::plugin_metadata ;
use crate::startup::plugin_discovery::RawPluginData ;
use super::plugin_id::PluginId ;



pub struct Plugin {
    id: PluginId,
    manifest: Vec<u8>,
    raw: RawPluginData,
}
impl<'a> Plugin {
    
    pub fn try_new(
        id: String,
        manifest: Vec<u8>,
        raw: RawPluginData,
    ) -> Result<Self, capnp::Error> {
        // DO NOT REMOVE: Ensures that root exists; .unwrap() in .get_manifest()
        serialize::read_message( Cursor::new( &manifest ), ReaderOptions::new())?
            .get_root::<plugin_metadata::Reader>()?;
        Ok( Self { id, manifest, raw })
    }
    
    pub fn id( &self ) -> &PluginId { &self.id }
    pub fn manifest( &'a mut self ) -> __ManifestReader {
        serialize::read_message( Cursor::new( &self.manifest ), ReaderOptions::new()).unwrap().into()
    }
    pub fn wasm( &self ) -> std::path::PathBuf { self.raw.wasm_root() }

}
impl std::fmt::Debug for Plugin {
    fn fmt( &self, fmt: &mut std::fmt::Formatter<'_> ) -> std::fmt::Result {
        fmt.debug_struct( "Plugin" )
            .field( "id", &self.id )
            // .field( "manifest", format!( "{:?}", self.manifest ))
            .field( "raw", &self.raw )
            .finish()
    }
}

pub struct __ManifestReader ( message::Reader<OwnedSegments> );
impl __ManifestReader {
    pub fn root<'a>( &'a mut self ) -> plugin_metadata::Reader<'a> {
        self.0.get_root::<plugin_metadata::Reader>().unwrap()
    }
}
impl From<message::Reader<OwnedSegments>> for __ManifestReader {
    fn from( reader: message::Reader<OwnedSegments> ) -> Self { Self ( reader )}
}
