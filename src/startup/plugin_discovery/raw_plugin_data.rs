use super::PluginCacheError;



#[derive( Debug )]
pub struct RawPluginData {
    path: std::path::PathBuf,
}

impl RawPluginData {
    pub(super) fn new( plugin_dir: &std::fs::DirEntry ) -> Result<Self, PluginCacheError> {
        let path = plugin_dir.path();
        match path.is_dir() {
            true => Ok( Self { path } ),
            false => Err( PluginCacheError::InvalidDataError( "Expected Plugin Dir, found file".to_owned() )),
        }
    }
    pub fn manifest( &self ) -> Result<Vec<u8>, std::io::Error> {
        std::fs::read( self.path.join( "manifest.bin" ) )
    }
    pub fn wasm_root( &self ) -> Result<Vec<u8>, std::io::Error> {
        std::fs::read( self.path.join( "source.wasm" ))
    }
    pub fn display_path( &self ) -> String {
        self.path.display().to_string()
    }
}