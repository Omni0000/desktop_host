use std::path::PathBuf ;
use std::io::Cursor ;
use capnp::message::ReaderOptions;
use capnp::serialize ;
use thiserror::Error ;

use crate::initialisation::{ PluginId, InterfaceId };
use crate::capnp::manifest::plugin_manifest_capnp::plugin_manifest ;
use super::{ DiscoveryError, PLUGINS_DIR };



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

    pub fn new( id: &PluginId ) -> Result<Self, DiscoveryError> {
        let root_path = Self::root_path( id );
        if Self::is_complete( &root_path ) { Ok( Self { id: id.clone(), root_path, manifest_data: None } )}
        else { Err( DiscoveryError::PluginNotInCache( id.clone() )) }
    }

    #[inline] fn root_path( id: &PluginId ) -> PathBuf { PathBuf::from( PLUGINS_DIR ).join( format!( "{}", id ) )}
    #[inline] fn is_complete( root_path: &PathBuf ) -> bool {
        Self::_wasm_path( root_path ).is_file()
        && Self::_manifest_path( root_path ).is_file()
    }

    #[inline] pub fn id( &self ) -> &PluginId { &self.id }

    #[inline] fn get_manifest_data( &mut self ) -> Result<&Vec<u8>, std::io::Error> {
        let manifest_data = self.manifest_data.take();
        Ok( self.manifest_data.insert( manifest_data.unwrap_or( std::fs::read( Self::_manifest_path( &self.root_path ))? ) ) )
    }
    #[inline] fn _manifest_path( root_path: &PathBuf ) -> PathBuf { root_path.join( "manifest.bin" )}

    #[inline] pub fn wasm_path( &self ) -> PathBuf { Self::_wasm_path( &self.root_path )}
    #[inline] fn _wasm_path( root_path: &PathBuf ) -> PathBuf { root_path.join( "root.wasm" )}

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
