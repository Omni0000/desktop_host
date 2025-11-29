use crate::manifest_capnp::plugin_metadata ;
use crate::startup::plugin_discovery::RawPluginData ;
use super::plugin_id::PluginId ;

pub struct Plugin {
    id: PluginId,
    manifest: capnp::message::Reader<capnp::serialize::OwnedSegments>,
    raw: RawPluginData,
}
impl<'a> Plugin {
    
    pub fn try_new(
        id: String,
        manifest: capnp::message::Reader<capnp::serialize::OwnedSegments>,
        raw: RawPluginData,
    ) -> Result<Self, capnp::Error> {
        // DO NOT REMOVE: Ensures that root exists; .unwrap() in .get_manifest()
        manifest.get_root::<plugin_metadata::Reader>()?;
        Ok( Self { id, manifest, raw })
    }
    
    pub fn id( &self ) -> &PluginId { &self.id }
    pub fn manifest( &'a self ) -> plugin_metadata::Reader<'a> {
        self.manifest.get_root::<plugin_metadata::Reader>().unwrap()
    }
    pub fn wasm( &self ) -> std::path::PathBuf { self.raw.wasm_root() }

}
impl std::fmt::Debug for Plugin {
    fn fmt( &self, fmt: &mut std::fmt::Formatter<'_> ) -> std::fmt::Result {
        fmt.debug_struct( "Plugin" )
            .field( "id", &self.id )
            .field( "manifest", &self.manifest() )
            .field( "raw", &self.raw )
            .finish()
    }
}