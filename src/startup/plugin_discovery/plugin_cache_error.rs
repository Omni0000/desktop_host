use thiserror::Error ;

#[derive( Error, Debug )]
pub enum PluginCacheError {

    #[error( "IO Error: {0}" )]
    IOError( #[from] std::io::Error ),

    #[error( "Invalid Data Error: {0}" )]
    InvalidDataError( String ),

}