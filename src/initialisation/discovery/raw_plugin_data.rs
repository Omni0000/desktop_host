use std::path::PathBuf ;
use std::io::Cursor ;
use capnp::message::ReaderOptions;
use capnp::serialize ;
use thiserror::Error ;

use crate::initialisation::{ PluginId, InterfaceId };
use crate::capnp::manifest::plugin_manifest_capnp::plugin_manifest ;
use super::DiscoveryError ;



pub struct RawPluginData {
    id: PluginId,
    root_path: PathBuf,
    manifest_data: Option<Vec<u8>>,
}
impl std::fmt::Debug for RawPluginData {
    fn fmt( &self, f: &mut std::fmt::Formatter<'_> ) -> std::result::Result<(), std::fmt::Error> {
        f.debug_struct( "RawPluginData" )
            .field( "id", &self.id )
            .field( "root_path", &self.root_path )
            .field( "manifest_data", &match &self.manifest_data {
                Some( data ) => format!( "ManifestData[len={}]", data.len() ),
                Option::None => "None".to_string(),
            })
            .finish()
    }
}

#[derive( Error, Debug )]
pub enum PluginManifestReadError {
    #[error( "Io Error: {0}" )] IoError( #[from] std::io::Error ),
    #[error( "Capnp Error: {0}" )] CapnpError( #[from] capnp::Error ),
}

impl RawPluginData {
    
    const WASM_FILE: &str = "root.wasm" ;
    const MANIFEST_FILE: &str = "manifest.bin" ;
    /* TEST */ const WAT_FILE: &str = "root.wat" ;
    /* TEST */ const MANIFEST_TOML_FILE: &str = "manifest.toml" ;

    pub fn new( source: &PathBuf, id: &PluginId ) -> Result<Self, DiscoveryError> {
        let root_path = Self::root_path( source, id );
        if Self::is_complete( &root_path ) { Ok( Self { id: id.clone(), root_path, manifest_data: None } )}
        else { Err( DiscoveryError::PluginNotInCache( id.clone() )) }
    }

    #[inline] fn root_path( source: &PathBuf, id: &PluginId ) -> PathBuf { source.join( id.to_string() )}
    #[inline] fn is_complete( root_path: &PathBuf ) -> bool {
        Self::_wasm_path( root_path ).is_file()
        && Self::_manifest_path( root_path ).is_file()
    }

    #[inline] pub fn id( &self ) -> &PluginId { &self.id }

    #[inline] fn get_manifest_data( &mut self ) -> Result<&Vec<u8>, std::io::Error> {
        let manifest_data = self.manifest_data.take();
        Ok( self.manifest_data.insert( manifest_data.unwrap_or({
            #[cfg( not( feature = "test" ))] {
                std::fs::read( Self::_manifest_path( &self.root_path ))
            }
            #[cfg( feature = "test" )] {
                let bin_path = self.root_path.join( Self::MANIFEST_FILE );
                if bin_path.is_file() {
                    std::fs::read( Self::_manifest_path( &self.root_path ))
                } else {
                    let text = std::fs::read_to_string( Self::_manifest_path( &self.root_path ))?;
                    let data: test_conversion::DeserialisablePluginManifest = toml::from_str( &text ).expect( "Deserialisation failure" );
                    let mut message = capnp::message::Builder::new_default();
                    capnp_conv::Writable::write( &data, message.init_root());
                    let mut buffer = Vec::new();
                    capnp::serialize::write_message( &mut buffer, &message ).expect( "Serialisation failure" );
                    Ok( buffer )
                }
            }
        }? )))
    }
    #[inline] fn _manifest_path( root_path: &PathBuf ) -> PathBuf {
        #[cfg( not( feature = "test" ))] {
            root_path.join( Self::MANIFEST_FILE )
        }
        #[cfg( feature = "test" )] {
            let bin_path = root_path.join( Self::MANIFEST_FILE );
            if bin_path.is_file() { bin_path }
            else { root_path.join( Self::MANIFEST_TOML_FILE )}
        }
    }

    #[inline] pub fn wasm_path( &self ) -> PathBuf { Self::_wasm_path( &self.root_path )}
    #[inline] fn _wasm_path( root_path: &PathBuf ) -> PathBuf {
        if !cfg!( feature = "test" ) {
            root_path.join( Self::WASM_FILE )
        } else {
            let bin_path = root_path.join( Self::WASM_FILE );
            if bin_path.is_file() { bin_path }
            else { root_path.join( Self::WAT_FILE )}
        }
}

    #[inline] pub fn get_plug( &mut self ) -> Result<InterfaceId, PluginManifestReadError> {
        let manifest = self.get_manifest_data()?;
        let reader = serialize::read_message( Cursor::new( &manifest ), ReaderOptions::new())?;
        let root = reader.get_root::<plugin_manifest::Reader>()?;
        Ok( root.get_plug()?.get_id() )
    }

    #[inline] pub fn get_sockets( &mut self ) -> Result<Vec<InterfaceId>, PluginManifestReadError> {
        let manifest = self.get_manifest_data()?;
        let reader = serialize::read_message( Cursor::new( &manifest ), ReaderOptions::new())?;
        let root = reader.get_root::<plugin_manifest::Reader>()?;
        Ok( root.get_sockets()?.into_iter().map( InterfaceId::from ).collect())
    }

}

#[cfg( feature = "test" )]
mod test_conversion {

    use capnp_conv::capnp_conv ;
    use serde::Deserialize ;

    #[derive( Deserialize )]
    #[capnp_conv( crate::capnp::common::plugin_capnp::plugin_id )]
    pub struct DeserialisablePluginId {
        pub id: String,
    }

    #[derive( Deserialize )]
    #[capnp_conv( crate::capnp::common::interface_capnp::interface_id )]
    pub struct DeserialisableInterfaceId {
        pub id: u64,
    }

    #[derive( Deserialize )]
    #[capnp_conv( crate::capnp::common::version_capnp::version )]
    pub struct DeserialisableVersion {
        pub major: u16,
        pub minor: u16,
        pub patch: u16,
    }

    #[derive( Deserialize )]
    #[capnp_conv( crate::capnp::manifest::plugin_manifest_capnp::plugin_manifest )]
    pub struct DeserialisablePluginManifest {
        id: DeserialisablePluginId,
        version: DeserialisableVersion,
        plug: DeserialisableInterfaceId,
        sockets: Vec<DeserialisableInterfaceId>,
    }

}
