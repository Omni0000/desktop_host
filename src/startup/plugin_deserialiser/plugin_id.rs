
use crate::capnp::manifest_capnp::plugin_id ;



pub type PluginId = String ;

impl<'a> TryFrom<plugin_id::Reader<'a>> for PluginId {

    type Error = capnp::Error;

    fn try_from( reader: plugin_id::Reader<'a> ) -> Result<Self, Self::Error> {
        Ok( reader.get_id()?.to_string()? )
    }

}