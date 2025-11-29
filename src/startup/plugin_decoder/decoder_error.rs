
use std::path::PathBuf ;
use thiserror::Error ;



#[derive( Error, Debug )]
pub enum DecoderError {

    #[error( "Cannot Read Plugin Directory: {0}" )]
    CannotReadPluginDirectory( std::io::Error ),

    #[error( "Cannot Access an Entry in the Plugin Directory: {0}")]
    InaccesiblePlugin( std::io::Error ),

    #[error( "{0}" )]
    MissingManifestErr( MissingManifestErr ),
    
    #[error( "Failed to Parse Manifest: {0}" )]
    InvalidManifestErr( capnp::Error ),

}

#[derive( Error, Debug )]
pub struct MissingManifestErr {

    path: PathBuf,
    error: std::io::Error,

}

impl MissingManifestErr {
 
    pub fn new(    
 
        path: PathBuf,
        error: std::io::Error,
 
    ) -> DecoderError { DecoderError::MissingManifestErr( MissingManifestErr { path, error }) }

}

impl std::fmt::Display for MissingManifestErr {
    fn fmt( &self, f: &mut std::fmt::Formatter<'_> ) -> std::fmt::Result {
        write!( f, "Missing Manifest File: At '{}': {}", self.path.display(), self.error )
    }
}