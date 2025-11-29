use thiserror::Error ;



#[derive( Error, Debug )]
pub enum DecoderError {

    #[error( "{0}" )]
    MissingManifestErr( MissingManifestErr ),
    
    #[error( "Failed to Parse Manifest: {0}" )]
    InvalidManifestErr( capnp::Error ),

}

#[derive( Error, Debug )]
pub struct MissingManifestErr {

    error: std::io::Error,
    path: String,

}

impl MissingManifestErr {
 
    pub fn new(    
 
        error: std::io::Error,
        path: String,
 
    ) -> DecoderError { DecoderError::MissingManifestErr( MissingManifestErr { error, path }) }

}

impl std::fmt::Display for MissingManifestErr {
    fn fmt( &self, f: &mut std::fmt::Formatter<'_> ) -> std::fmt::Result {
        write!( f, "Missing Manifest File: At '{}': {}", self.path, self.error )
    }
}